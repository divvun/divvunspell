pub mod hfst;
pub mod thfst;

mod alphabet;
mod symbol_transition;
pub(crate) mod tree_node;

pub(crate) use self::alphabet::TransducerAlphabet;

use self::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

#[derive(Debug)]
pub enum TransducerError {
    Memmap(std::io::Error),
    Io(std::io::Error),
    Alphabet(Box<dyn std::error::Error>),
}

impl TransducerError {
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

pub trait Transducer<F: vfs::File>: Sized {
    const FILE_EXT: &'static str;

    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>;

    fn alphabet(&self) -> &TransducerAlphabet;
    fn mut_alphabet(&mut self) -> &mut TransducerAlphabet;

    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool;
    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex>;
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool;
    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition>;
    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition>;
    fn take_non_epsilons(
        &self,
        i: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<SymbolTransition>;
    fn is_final(&self, i: TransitionTableIndex) -> bool;
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight>;
}

pub trait TransitionTable<F: vfs::File>: Sized {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>;
    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex>;
    fn weight(&self, i: TransitionTableIndex) -> Option<Weight>;

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.output_symbol(i) == None && self.target(i) == Some(1)
    }

    #[inline(always)]
    fn symbol_transition(&self, i: TransitionTableIndex) -> SymbolTransition {
        SymbolTransition::new(self.target(i), self.output_symbol(i), self.weight(i))
    }
}

pub trait IndexTable<F: vfs::File>: Sized {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>;
    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber>;
    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex>;
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight>;

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}

#[cfg(feature = "internal_convert")]
pub mod convert;
