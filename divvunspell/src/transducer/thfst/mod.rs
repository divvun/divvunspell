// We manually ensure alignment of reads in this file.
#![allow(clippy::cast_ptr_alignment)]

use std::path::Path;
use std::{u16, u32};

use crate::constants::TARGET_TABLE;
use crate::transducer::{TransducerError, symbol_transition::SymbolTransition};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use serde::{Deserialize, Serialize};

mod alphabet;
mod chunked;
mod index_table;
mod transition_table;

pub use self::chunked::ThfstChunkedTransducer;
pub use self::index_table::IndexTable;
pub use self::transition_table::TransitionTable;

use crate::transducer::{Transducer, TransducerAlphabet};
use crate::util::{self, Filesystem, ToMemmap};

#[repr(C)]
pub union WeightOrTarget {
    target: u32,
    weight: f32,
}

#[repr(C)]
pub struct IndexTableRecord {
    input_symbol: u16,
    #[doc(hidden)]
    __padding: u16,
    weight_or_target: WeightOrTarget,
}

#[repr(C)]
pub struct TransitionTableRecord {
    input_symbol: u16,
    output_symbol: u16,
    weight_or_target: WeightOrTarget,
}

#[derive(Serialize, Deserialize)]
pub struct MetaRecord {
    pub index_table_count: usize,
    pub transition_table_count: usize,
    pub chunk_size: usize,
    pub alphabet: TransducerAlphabet,
}

pub struct ThfstTransducer {
    index_table: IndexTable,
    transition_table: TransitionTable,
    alphabet: TransducerAlphabet,
}

macro_rules! error {
    ($path:path, $name:expr) => {
        TransducerError::Io(
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "`{}` not found in transducer path, looked for {}",
                    $name,
                    $path.join($name).display()
                ),
            )
        )
    };
}

impl Transducer for ThfstTransducer {
    const FILE_EXT: &'static str = "thfst";

    fn from_path<P, FS, F>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
        F: util::File + ToMemmap,
    {
        let path = path.as_ref();
        let alphabet_file = fs
            .open(&path.join("alphabet"))
            .map_err(|_| error!(path, "alphabet"))?;

        let alphabet: TransducerAlphabet = serde_json::from_reader(alphabet_file)
            .map_err(|e| TransducerError::Alphabet(Box::new(e)))?;

        let index_table = IndexTable::from_path(fs, path.join("index"))
            .map_err(|_| error!(path, "index"))?;
        let transition_table = TransitionTable::from_path(fs, path.join("transition"))
            .map_err(|_| error!(path, "transition"))?;

        Ok(ThfstTransducer {
            index_table,
            transition_table,
            alphabet,
        })
    }

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            self.transition_table.is_final(i - TARGET_TABLE)
        } else {
            self.index_table.is_final(i)
        }
    }

    #[inline(always)]
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            self.transition_table.weight(i - TARGET_TABLE)
        } else {
            self.index_table.final_weight(i)
        }
    }

    #[inline(always)]
    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        let sym = match s {
            Some(v) => v,
            None => return false,
        };

        if i >= TARGET_TABLE {
            match self.transition_table.input_symbol(i - TARGET_TABLE) {
                Some(res) => sym == res,
                None => false,
            }
        } else {
            match self.index_table.input_symbol(i + u32::from(sym)) {
                Some(res) => sym == res,
                None => false,
            }
        }
    }

    #[inline(always)]
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            match self.transition_table.input_symbol(i - TARGET_TABLE) {
                Some(sym) => sym == 0 || self.alphabet.is_flag(sym),
                None => false,
            }
        } else if let Some(0) = self.index_table.input_symbol(i) {
            true
        } else {
            false
        }
    }

    #[inline(always)]
    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        if let Some(0) = self.transition_table.input_symbol(i) {
            Some(self.transition_table.symbol_transition(i))
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        if let Some(sym) = self.transition_table.input_symbol(i) {
            if sym != 0 && !self.alphabet.is_flag(sym) {
                None
            } else {
                Some(self.transition_table.symbol_transition(i))
            }
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_non_epsilons(
        &self,
        i: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<SymbolTransition> {
        if let Some(input_sym) = self.transition_table.input_symbol(i) {
            if input_sym != symbol {
                None
            } else {
                Some(self.transition_table.symbol_transition(i))
            }
        } else {
            None
        }
    }

    #[inline(always)]
    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + 1)
        } else if let Some(v) = self.index_table.target(i + 1 + u32::from(symbol)) {
            Some(v - TARGET_TABLE)
        } else {
            None
        }
    }

    #[inline(always)]
    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.transition_table.input_symbol(i)
    }

    #[inline(always)]
    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    #[inline(always)]
    fn mut_alphabet(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }
}
