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

use crate::transducer::alphabet::TransducerAlphabet;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

/// Error with transducer reading or processing.
#[derive(Debug, thiserror::Error)]
pub enum TransducerError {
    /// Error with mmapping
    #[error("Failed to memory map transducer file")]
    Memmap(#[source] std::io::Error),
    /// Error with input/output.
    #[error("I/O error while reading transducer")]
    Io(#[source] std::io::Error),
    /// Error with FSA alphabets.
    #[error("Failed to process transducer alphabet")]
    Alphabet(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl TransducerError {
    /// Wrap into i/o error.
    pub fn into_io_error(self) -> std::io::Error {
        match self {
            TransducerError::Memmap(v) => v,
            TransducerError::Io(v) => v,
            TransducerError::Alphabet(v) => {
                std::io::Error::new(std::io::ErrorKind::Other, format!("{}", v))
            }
        }
    }
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
