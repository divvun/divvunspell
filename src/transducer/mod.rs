pub mod alphabet;
pub mod header;
pub mod index_table;
pub mod symbol_transition;
pub mod transition_table;
pub mod tree_node;
pub mod realign;

use std::fmt;
use memmap::Mmap;
use std::sync::Arc;

use crate::types::{TransitionTableIndex, Weight, SymbolNumber, HeaderFlag};
use crate::constants::{TRANS_INDEX_SIZE, TRANS_SIZE, TARGET_TABLE};

use self::header::TransducerHeader;
use self::alphabet::TransducerAlphabet;
use self::index_table::IndexTable;
use self::transition_table::TransitionTable;
use self::symbol_transition::SymbolTransition;

pub struct Transducer {
    buf: Arc<Mmap>,
    header: TransducerHeader,
    alphabet: TransducerAlphabet,
    index_table: IndexTable,
    transition_table: TransitionTable,
}

impl fmt::Debug for Transducer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:?}", self.header)?;
        writeln!(f, "{:?}", self.alphabet)?;
        writeln!(f, "{:?}", self.index_table)?;
        writeln!(f, "{:?}", self.transition_table)?;
        Ok(())
    }
}

impl Transducer {
    pub fn from_mapped_memory(buf: Mmap) -> Transducer {
        let buf = Arc::new(buf);
        let header = TransducerHeader::new(&buf);
        let alphabet_offset = header.len();
        let alphabet =
            TransducerAlphabet::new(&buf[alphabet_offset..buf.len()], header.symbol_count());

        let index_table_offset = alphabet_offset + alphabet.len();
        println!("Index table start offset: {}", index_table_offset);

        let index_table_end = index_table_offset + TRANS_INDEX_SIZE * header.index_table_size();
        println!("Index table end: {}", index_table_end);
        let index_table = IndexTable::new(buf.clone(),
            index_table_offset,
            index_table_end,
            header.index_table_size() as u32,
        );

        println!("Trans table start offset: {}", index_table_end);
        let trans_table_end = index_table_end + TRANS_SIZE * header.target_table_size();
        let trans_table = TransitionTable::new(buf.clone(),
            index_table_end,
            trans_table_end,
            header.target_table_size() as u32,
        );

        Transducer {
            buf: buf,
            header: header,
            alphabet: alphabet,
            index_table: index_table,
            transition_table: trans_table,
        }
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    pub fn index_table(&self) -> &IndexTable {
        &self.index_table
    }

    pub fn transition_table(&self) -> &TransitionTable {
        &self.transition_table
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            self.transition_table.is_final(i - TARGET_TABLE)
        } else {
            self.index_table.is_final(i)
        }
    }

    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            self.transition_table.weight(i - TARGET_TABLE)
        } else {
            self.index_table.final_weight(i)
        }
    }

    pub fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        if s.is_none() {
            return false;
        }

        let sym = s.unwrap();

        if i >= TARGET_TABLE {
            match self.transition_table.input_symbol(i - TARGET_TABLE) {
                Some(res) => sym == res,
                None => false,
            }
        } else {
            match self.index_table.input_symbol(i + sym as u32) {
                Some(res) => sym == res,
                None => false,
            }
        }
    }

    pub fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
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

    pub fn has_non_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            match self.transition_table.input_symbol(i - TARGET_TABLE) {
                Some(res) => res != 0 && !self.alphabet().is_flag(res),
                None => false,
            }
        } else {
            let total = self.alphabet.key_table().len() as u16;

            for j in 1..total {
                let res = self.index_table.input_symbol(i + j as u32);

                if res.is_none() {
                    continue;
                }

                if res.unwrap() == j {
                    return true;
                }
            }

            false
        }
    }

    pub fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        if let Some(0) = self.transition_table.input_symbol(i) {
            Some(self.transition_table.symbol_transition(i))
        } else {
            None
        }
    }

    pub fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        if let Some(sym) = self.transition_table.input_symbol(i) {
            if sym != 0 && !self.alphabet.is_flag(sym) {
                return None;
            } else {
                return Some(self.transition_table.symbol_transition(i));
            }
        } else {
            return None;
        }
    }

    pub fn take_non_epsilons(
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

    pub fn next(
        &self,
        i: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + 1)
        } else if let Some(v) = self.index_table.target(i + 1 + symbol as u32) {
            Some(v - TARGET_TABLE)
        } else {
            None
        }
    }

    pub fn header(&self) -> &TransducerHeader {
        &self.header
    }

    pub fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    pub fn mut_alphabet(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }

    pub fn is_weighted(&self) -> bool {
        self.header.has_flag(HeaderFlag::Weighted)
    }
}
