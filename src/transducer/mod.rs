//! Transducer is a Finite-State Automaton with two tapes / two symbols per
//! transition.
//!
//! Transducer in divvunspell is modeled after the C++ transducer in the
//! hfst-ospell library. It may contain some complex optimisations and
//! specifics to underlying finite-state systems and lot of this is
//! pretty hacky.
pub mod hfst;
pub mod thfst;

mod alphabet;
mod symbol_transition;
pub(crate) mod tree_node;

use std::borrow::Cow;
use std::path::PathBuf;

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
