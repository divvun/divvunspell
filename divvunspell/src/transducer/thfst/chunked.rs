use std::path::Path;

use crate::constants::TARGET_TABLE;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

use super::index_table::IndexTable;
use super::transition_table::TransitionTable;
use crate::transducer::{Transducer, TransducerAlphabet};
use crate::util::{self, Filesystem, ToMemmap};

/// Tromsø-Helsinki Finite State Transducer format
pub struct ThfstChunkedTransducer {
    // meta: MetaRecord,
    index_tables: Vec<IndexTable>,
    indexes_per_chunk: u32,
    transition_tables: Vec<TransitionTable>,
    transitions_per_chunk: u32,
    alphabet: TransducerAlphabet,
}

macro_rules! transition_rel_index {
    ($self:expr, $x:expr) => {{
        let index_page = $x / $self.transitions_per_chunk;
        let relative_index = $x - ($self.transitions_per_chunk * index_page);
        (index_page as usize, relative_index)
    }};
}

macro_rules! index_rel_index {
    ($self:expr, $x:expr) => {{
        let index_page = $x / $self.indexes_per_chunk;
        let relative_index = $x - ($self.indexes_per_chunk * index_page);
        (index_page as usize, relative_index)
    }};
}

macro_rules! error {
    ($path:path, $name:expr) => {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "`{}` not found in transducer path, looked for {}",
                $name,
                $path.join($name).display()
            ),
        )
    };
}

impl ThfstChunkedTransducer {
    pub fn from_path<P, FS, F>(fs: &FS, path: P) -> Result<Self, std::io::Error>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
        F: util::File + ToMemmap,
    {
        let path = path.as_ref();
        let alphabet_file = fs
            .open(&path.join("alphabet"))
            .map_err(|_| error!(path, "alphabet"))?;

        let alphabet: TransducerAlphabet = serde_json::from_reader(alphabet_file)?;

        let index_table =
            IndexTable::from_path(fs, path.join("index")).map_err(|_| error!(path, "index"))?;
        let transition_table = TransitionTable::from_path(fs, path.join("transition"))
            .map_err(|_| error!(path, "transition"))?;

        Ok(ThfstChunkedTransducer {
            indexes_per_chunk: index_table.size,
            transitions_per_chunk: transition_table.size,
            index_tables: vec![index_table],
            transition_tables: vec![transition_table],
            alphabet,
        })
    }
}

impl Transducer for ThfstChunkedTransducer {
    // type Alphabet = TransducerAlphabet;
    const FILE_EXT: &'static str = "thfst";

    #[inline(always)]
    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    #[inline(always)]
    fn mut_alphabet(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }

    #[inline(always)]
    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        let (page, index) = transition_rel_index!(self, i);
        self.transition_tables[page].input_symbol(index)
    }

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let (page, index) = transition_rel_index!(self, i - TARGET_TABLE);
            self.transition_tables[page].is_final(index)
        } else {
            let (page, index) = index_rel_index!(self, i);
            self.index_tables[page].is_final(index)
        }
    }

    #[inline(always)]
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            let (page, index) = transition_rel_index!(self, i - TARGET_TABLE);
            self.transition_tables[page].weight(index)
        } else {
            let (page, index) = index_rel_index!(self, i);
            self.index_tables[page].final_weight(index)
        }
    }

    #[inline(always)]
    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        let sym = match s {
            Some(v) => v,
            None => return false,
        };

        if i >= TARGET_TABLE {
            let (page, index) = transition_rel_index!(self, i - TARGET_TABLE);
            match self.transition_tables[page].input_symbol(index) {
                Some(res) => sym == res,
                None => false,
            }
        } else {
            let (page, index) = index_rel_index!(self, i + u32::from(sym));
            match self.index_tables[page].input_symbol(index) {
                Some(res) => sym == res,
                None => false,
            }
        }
    }

    #[inline(always)]
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let (page, index) = transition_rel_index!(self, i - TARGET_TABLE);
            match self.transition_tables[page].input_symbol(index) {
                Some(sym) => sym == 0 || self.alphabet.is_flag(sym),
                None => false,
            }
        } else {
            let (page, index) = index_rel_index!(self, i);
            if let Some(0) = self.index_tables[page].input_symbol(index) {
                true
            } else {
                false
            }
        }
    }

    #[inline(always)]
    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = transition_rel_index!(self, i);

        if let Some(0) = self.transition_tables[page].input_symbol(index) {
            Some(self.transition_tables[page].symbol_transition(index))
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = transition_rel_index!(self, i);

        if let Some(sym) = self.transition_tables[page].input_symbol(index) {
            if sym != 0 && !self.alphabet.is_flag(sym) {
                None
            } else {
                Some(self.transition_tables[page].symbol_transition(index))
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
        let (page, index) = transition_rel_index!(self, i);
        if let Some(input_sym) = self.transition_tables[page].input_symbol(index) {
            if input_sym != symbol {
                None
            } else {
                Some(self.transition_tables[page].symbol_transition(index))
            }
        } else {
            None
        }
    }

    #[inline(always)]
    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + 1)
        } else {
            let (page, index) = index_rel_index!(self, i + 1 + u32::from(symbol));

            if let Some(v) = self.index_tables[page].target(index) {
                Some(v - TARGET_TABLE)
            } else {
                None
            }
        }
    }
}
