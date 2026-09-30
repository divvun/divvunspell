//! Transducer is a Finite-State Automaton with two tapes / two symbols per
//! transition.
//!
//! Transducer in divvunspell is modeled after the C++ transducer in the
//! hfst-ospell library. It may contain some complex optimisations and
//! specifics to underlying finite-state systems and lot of this is
//! pretty hacky.
pub mod dhfst;
pub mod hfst;
pub mod thfst;

mod alphabet;
pub(crate) mod heuristic;
pub(crate) mod symbol_transition;
pub(crate) mod tree_node;

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use memmap2::Mmap;

use crate::transducer::alphabet::TransducerAlphabet;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

/// Error with transducer reading or processing.
///
/// Every variant names the file or path involved and preserves its underlying
/// cause via `#[source]`, so the full chain is walkable with
/// [`std::error::Error::source`] (or via `anyhow::Error`'s `Debug` renderer).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransducerError {
    /// Opening the transducer file failed.
    #[error("failed to open transducer file '{}'", path.display())]
    Io {
        /// file that failed to open
        path: PathBuf,
        /// underlying I/O error
        #[source]
        source: std::io::Error,
    },

    /// Memory-mapping the transducer file failed.
    #[error("failed to memory-map transducer file '{}'", path.display())]
    Memmap {
        /// file that failed to memory-map
        path: PathBuf,
        /// underlying I/O error
        #[source]
        source: std::io::Error,
    },

    /// A required component (alphabet, index, transition) was not present
    /// inside the transducer's directory.
    #[error("required transducer component '{component}' missing in '{}'", path.display())]
    MissingComponent {
        /// directory or archive path being loaded
        path: PathBuf,
        /// the component that could not be located
        component: &'static str,
        /// underlying I/O error
        #[source]
        source: std::io::Error,
    },

    /// The alphabet file could not be parsed as JSON.
    #[error("failed to parse alphabet file '{}' as JSON", path.display())]
    AlphabetJson {
        /// file being parsed
        path: PathBuf,
        /// JSON parse error with a source-snippet at the failure location
        #[source]
        source: crate::util::JsonParseError,
    },

    /// The alphabet is syntactically parseable but semantically invalid.
    #[error("alphabet in '{}' is malformed: {detail}", path.display())]
    AlphabetMalformed {
        /// file containing the malformed alphabet
        path: PathBuf,
        /// human-readable explanation
        detail: Cow<'static, str>,
    },

    /// The transducer header is truncated or contains invalid field values.
    #[error("transducer header in '{}' is truncated or corrupt at offset {offset}", path.display())]
    CorruptHeader {
        /// file containing the corrupt header
        path: PathBuf,
        /// byte offset at which parsing failed
        offset: usize,
    },

    /// The transducer's index or transition tables are truncated or do not
    /// match the sizes declared by the header.
    #[error("transducer tables in '{}' are truncated or corrupt ({detail})", path.display())]
    CorruptTables {
        /// file containing the corrupt tables
        path: PathBuf,
        /// human-readable explanation
        detail: Cow<'static, str>,
    },

    /// The file does not start with the header of any transducer format this
    /// reader knows, or names a version of one that it does not.
    #[error("'{}' is not a transducer this reader can load ({detail})", path.display())]
    UnrecognisedFormat {
        /// file whose header was not recognised
        path: PathBuf,
        /// what the header held, and what was expected
        detail: Cow<'static, str>,
    },
}

/// A transducer's on-disk format, as its first bytes declare it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransducerFormat {
    /// HFST optimized-lookup, starting `HFST\0`.
    Hfst,
    /// The compact error-model format, starting `DHFST` and a version byte.
    Dhfst {
        /// the format version the file declares
        version: u8,
    },
}

impl TransducerFormat {
    /// Tell the format from a file's first bytes.
    ///
    /// Anything that is neither `HFST\0` nor `DHFST` followed by a version this
    /// reader supports is refused with a [`TransducerError::UnrecognisedFormat`]
    /// naming what was found, so that no file is ever read in the wrong layout.
    pub fn detect(bytes: &[u8], path: &Path) -> Result<TransducerFormat, TransducerError> {
        if bytes.starts_with(hfst::header::MAGIC) {
            return Ok(TransducerFormat::Hfst);
        }

        if bytes.starts_with(dhfst::MAGIC) {
            let version = bytes.get(dhfst::MAGIC.len()).copied().ok_or_else(|| {
                TransducerError::UnrecognisedFormat {
                    path: path.to_path_buf(),
                    detail: Cow::Borrowed("DHFST header is truncated before its version byte"),
                }
            })?;
            if version != dhfst::VERSION {
                return Err(TransducerError::UnrecognisedFormat {
                    path: path.to_path_buf(),
                    detail: Cow::Owned(format!(
                        "DHFST version {version}; this reader supports version {}",
                        dhfst::VERSION
                    )),
                });
            }
            return Ok(TransducerFormat::Dhfst { version });
        }

        let shown: String = bytes
            .iter()
            .take(8)
            .map(|b| {
                if b.is_ascii_graphic() {
                    (*b as char).to_string()
                } else {
                    format!("\\x{b:02x}")
                }
            })
            .collect();
        Err(TransducerError::UnrecognisedFormat {
            path: path.to_path_buf(),
            detail: Cow::Owned(format!(
                "header starts \"{shown}\"; expected \"HFST\\0\" or \"DHFST\""
            )),
        })
    }
}

/// An error model in either of the formats an archive can carry it in.
///
/// Loading goes by the file's header rather than its name, so a member that
/// holds the compact format can never be read as optimized lookup, or the
/// other way round.
pub enum ErrorModel {
    /// HFST optimized lookup.
    Hfst(hfst::HfstTransducer),
    /// The compact DHFST format.
    Dhfst(dhfst::DhfstTransducer),
}

impl ErrorModel {
    /// Load an error model from a mapped file, in whichever format its header
    /// declares.
    pub fn from_mapped_memory(
        buf: Arc<Mmap>,
        path: impl Into<PathBuf>,
    ) -> Result<ErrorModel, TransducerError> {
        let path = path.into();
        match TransducerFormat::detect(&buf, &path)? {
            TransducerFormat::Hfst => {
                hfst::HfstTransducer::from_mapped_memory(buf, path).map(ErrorModel::Hfst)
            }
            TransducerFormat::Dhfst { .. } => {
                dhfst::DhfstTransducer::from_mapped_memory(buf, path).map(ErrorModel::Dhfst)
            }
        }
    }

    /// Load an error model from a file, in whichever format its header
    /// declares.
    pub fn from_path<FS, F>(fs: &FS, path: impl AsRef<Path>) -> Result<ErrorModel, TransducerError>
    where
        FS: Filesystem<File = F>,
        F: vfs::File,
    {
        let path = path.as_ref();
        let mut header = [0u8; 8];
        let mut filled = 0;
        {
            let mut file = fs.open_file(path).map_err(|source| TransducerError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            while filled < header.len() {
                let read =
                    file.read(&mut header[filled..])
                        .map_err(|source| TransducerError::Io {
                            path: path.to_path_buf(),
                            source,
                        })?;
                if read == 0 {
                    break;
                }
                filled += read;
            }
        }

        match TransducerFormat::detect(&header[..filled], path)? {
            TransducerFormat::Hfst => {
                <hfst::HfstTransducer as TransducerLoader<F>>::from_path(fs, path)
                    .map(ErrorModel::Hfst)
            }
            TransducerFormat::Dhfst { .. } => {
                <dhfst::DhfstTransducer as TransducerLoader<F>>::from_path(fs, path)
                    .map(ErrorModel::Dhfst)
            }
        }
    }

    /// The format the model was read in.
    pub fn format(&self) -> TransducerFormat {
        match self {
            ErrorModel::Hfst(_) => TransducerFormat::Hfst,
            ErrorModel::Dhfst(_) => TransducerFormat::Dhfst {
                version: dhfst::VERSION,
            },
        }
    }
}

impl std::fmt::Display for TransducerFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransducerFormat::Hfst => write!(f, "HFST optimized lookup"),
            TransducerFormat::Dhfst { version } => write!(f, "DHFST version {version}"),
        }
    }
}

/// A set of symbols, as a bitset indexed by symbol number.
#[derive(Clone, Copy, Debug)]
pub struct SymbolSet<'a> {
    words: &'a [u64],
}

impl<'a> SymbolSet<'a> {
    /// A set over the given bitset words: symbol `s` is word `s / 64`, bit
    /// `s % 64`.
    #[inline(always)]
    pub fn new(words: &'a [u64]) -> SymbolSet<'a> {
        SymbolSet { words }
    }

    /// Whether `symbol` is in the set.
    #[inline(always)]
    pub fn contains(&self, symbol: SymbolNumber) -> bool {
        let s = symbol.0 as usize;
        self.words
            .get(s / 64)
            .is_some_and(|word| word & (1u64 << (s % 64)) != 0)
    }

    /// Whether the set is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    /// How many symbols the set holds.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// The symbols in ascending order.
    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = SymbolNumber> + 'a {
        self.words.iter().enumerate().flat_map(|(index, word)| {
            let mut bits = *word;
            std::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                Some(SymbolNumber((index * 64 + bit) as u16))
            })
        })
    }
}

/// Arcs leaving one state on one input symbol, grouped the way a transducer
/// may store them.
#[derive(Clone, Copy, Debug)]
pub enum ArcGroup<'a> {
    /// A single arc.
    One {
        /// output symbol
        output: SymbolNumber,
        /// target state
        target: TransitionTableIndex,
        /// arc weight
        weight: Weight,
    },
    /// One arc per symbol in `outputs`, every one of them to `target` at
    /// `weight`: a default arc, which says "any of these outputs" rather than
    /// naming one.
    ///
    /// The outputs are symbols of the transducer's own alphabet. A consumer
    /// that composes this transducer with another one should intersect the
    /// set with what the other can continue with, rather than spell out every
    /// member.
    Each {
        /// the output symbols, never empty
        outputs: SymbolSet<'a>,
        /// target state shared by every arc of the group
        target: TransitionTableIndex,
        /// weight shared by every arc of the group
        weight: Weight,
    },
}

/// A finite-state transducer.
///
/// This trait defines the interface for finite-state transducers used for spell-checking
/// and morphological analysis. All traversal and query operations are defined here.
///
/// Implementors can provide custom transducer formats beyond the built-in HFST and THFST formats.
pub trait Transducer: Sized {
    /// file extension.
    const FILE_EXT: &'static str;

    /// get transducer's alphabet.
    fn alphabet(&self) -> &TransducerAlphabet;
    /// get transducer's alphabet as mutable reference.
    fn alphabet_mut(&mut self) -> &mut TransducerAlphabet;

    /// get input symbol number of given transition arc.
    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    /// check if there are transitions at given index.
    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool;
    /// get next transition with a symbol.
    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex>;
    /// check if there are free transitions at index.
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool;
    /// follow free transitions.
    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition>;
    /// follow epsilon transitions.
    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition>;
    /// follow transitions with given symbol.
    fn take_non_epsilons(
        &self,
        i: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<SymbolTransition>;
    /// check if given index is an end state.
    fn is_final(&self, i: TransitionTableIndex) -> bool;
    /// get end state weight of a state.
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight>;

    /// Lower bound on the weight of any path from state `i` to a final state,
    /// final weight included.
    ///
    /// This is the admissible heuristic the suggestion search orders its queue
    /// by: a partial path standing at `i` cannot possibly finish for less than
    /// this, so `path weight + distance_to_final` never overestimates the
    /// cheapest completion. [`Weight::INFINITE`] means no final state is
    /// reachable from `i` at all.
    ///
    /// The default implementation answers [`Weight::ZERO`], which is a valid
    /// (if uninformative) lower bound for every state — a backend that does not
    /// implement it degrades the search to plain best-first order rather than
    /// changing its results.
    fn distance_to_final(&self, _i: TransitionTableIndex) -> Weight {
        Weight::ZERO
    }

    /// Hand `visit` every arc leaving `state` on `input`, as `(output, target,
    /// weight)`.
    ///
    /// `state` is a state, not the cursor one past it that
    /// [`has_transitions`](Self::has_transitions) takes. An `input` of epsilon
    /// visits the epsilon-input arcs; flag diacritic arcs are never visited.
    ///
    /// The default implementation walks the cursor API, exactly as the
    /// suggestion search always has; a format without a flat run of arcs per
    /// input overrides it.
    #[inline(always)]
    fn for_each_arc<V>(&self, state: TransitionTableIndex, input: SymbolNumber, mut visit: V)
    where
        V: FnMut(SymbolNumber, TransitionTableIndex, Weight),
    {
        if !self.has_transitions(state.incr(), Some(input)) {
            return;
        }
        let Some(mut next) = self.next(state, input) else {
            return;
        };

        loop {
            let transition = if input == SymbolNumber::ZERO {
                self.take_epsilons(next)
            } else {
                self.take_non_epsilons(next, input)
            };
            let Some(transition) = transition else {
                break;
            };

            if let (Some(output), Some(target), Some(weight)) = (
                transition.symbol(),
                transition.target(),
                transition.weight(),
            ) {
                visit(output, target, weight);
            }

            next = next.incr();
        }
    }

    /// Like [`for_each_arc`](Self::for_each_arc), but a group of arcs that
    /// differ only in their output symbol may arrive as one
    /// [`ArcGroup::Each`].
    ///
    /// The default implementation has no groups and hands over one arc at a
    /// time.
    #[inline(always)]
    fn for_each_arc_group<V>(&self, state: TransitionTableIndex, input: SymbolNumber, mut visit: V)
    where
        V: FnMut(ArcGroup<'_>),
    {
        self.for_each_arc(state, input, |output, target, weight| {
            visit(ArcGroup::One {
                output,
                target,
                weight,
            })
        });
    }
}

/// Trait for loading transducers from files.
///
/// This trait is separate from `Transducer` because the file type parameter is only
/// needed during construction, not for runtime traversal operations.
pub trait TransducerLoader<F: vfs::File>: Transducer {
    /// read a transducer from a file.
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>;
}

/// Transition table contains the arcs of the automaton (and states).
pub trait TransitionTableTrait: Sized {
    /// number of records in the table.
    fn len(&self) -> TransitionTableIndex;
    /// whether the table holds no records.
    fn is_empty(&self) -> bool {
        self.len() == TransitionTableIndex(0)
    }
    /// get input symbol of a transition.
    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    /// get output symbol of a transition.
    fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    /// get the target state in the index.
    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex>;
    /// get the weight of the transition.
    fn weight(&self, i: TransitionTableIndex) -> Option<Weight>;

    /// check if the state is a final state.
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None
            && self.output_symbol(i) == None
            && self.target(i) == Some(TransitionTableIndex(1))
    }

    /// ???
    fn symbol_transition(&self, i: TransitionTableIndex) -> SymbolTransition {
        SymbolTransition::new(self.target(i), self.output_symbol(i), self.weight(i))
    }
}

/// Trait for loading transition tables from files.
pub trait TransitionTableLoader<F: vfs::File>: TransitionTableTrait {
    /// read transition table from a file.
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>;
}

/// Index table contains something.
pub trait IndexTableTrait: Sized {
    /// number of entries in the table.
    fn len(&self) -> TransitionTableIndex;
    /// whether the table holds no entries.
    fn is_empty(&self) -> bool {
        self.len() == TransitionTableIndex(0)
    }
    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex>;
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight>;

    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}

/// Trait for loading index tables from files.
pub trait IndexTableLoader<F: vfs::File>: IndexTableTrait {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>;
}

// Keep old trait names for backwards compatibility
#[deprecated(
    since = "0.1.0",
    note = "use TransitionTableTrait and TransitionTableLoader instead"
)]
pub trait TransitionTable<F: vfs::File>: TransitionTableTrait + TransitionTableLoader<F> {}

#[deprecated(
    since = "0.1.0",
    note = "use IndexTableTrait and IndexTableLoader instead"
)]
pub trait IndexTable<F: vfs::File>: IndexTableTrait + IndexTableLoader<F> {}

#[doc(hidden)]
// This is not a public API.
pub mod convert;
