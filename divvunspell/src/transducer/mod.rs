mod alphabet;
pub mod hfst;
mod symbol_transition;
pub mod thfst;
pub mod tree_node;

use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

pub use self::alphabet::TransducerAlphabet;
use self::symbol_transition::SymbolTransition;

use crate::util::{self, Filesystem, ToMemmap};

pub trait Transducer: Sized {
    const FILE_EXT: &'static str;

    fn from_path<P, FS, F>(fs: &FS, path: P) -> Result<Self, std::io::Error>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
        F: util::File + ToMemmap;

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

// pub trait Alphabet
// where
//     Self: Sized,
// {
//     // fn new(buf: &[u8], symbols: SymbolNumber) -> Self;
//     fn key_table(&self) -> &Vec<SmolStr>;
//     fn state_size(&self) -> SymbolNumber;
//     fn operations(&self) -> &OperationsMap;
//     fn string_to_symbol(&self) -> &HashMap<SmolStr, SymbolNumber>;
//     fn is_flag(&self, symbol: SymbolNumber) -> bool;
//     fn add_symbol(&mut self, string: &str);
//     fn identity(&self) -> Option<SymbolNumber>;
//     fn unknown(&self) -> Option<SymbolNumber>;
//     fn initial_symbol_count(&self) -> SymbolNumber;
//     fn len(&self) -> usize;
//     fn is_empty(&self) -> bool;
//     fn create_translator_from<T: Transducer<Alphabet = Self>>(
//         &mut self,
//         mutator: &T,
//     ) -> Vec<SymbolNumber>;
// }

#[cfg(feature = "convert")]
pub mod convert;
