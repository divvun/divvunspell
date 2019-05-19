pub mod alphabet;
pub mod chunk;
pub mod header;
pub mod index_table;
pub mod symbol_transition;
pub mod transition_table;
pub mod tree_node;

use memmap::Mmap;
use std::fmt;
use std::sync::Arc;

use crate::constants::{INDEX_TABLE_SIZE, TARGET_TABLE, TRANS_TABLE_SIZE};
use crate::types::{HeaderFlag, SymbolNumber, TransitionTableIndex, Weight};

use self::alphabet::TransducerAlphabet;
use self::header::TransducerHeader;
use self::index_table::IndexTable;
use self::symbol_transition::SymbolTransition;
use self::transition_table::TransitionTable;

pub trait Transducer {
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
pub struct HfstTransducer {
    buf: Arc<Mmap>,
    header: TransducerHeader,
    alphabet: TransducerAlphabet,
    index_table: IndexTable,
    transition_table: TransitionTable,
}

impl fmt::Debug for HfstTransducer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:?}", self.header)?;
        writeln!(f, "{:?}", self.alphabet)?;
        writeln!(f, "{:?}", self.index_table)?;
        writeln!(f, "{:?}", self.transition_table)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum TransducerSerializeError {
    InvalidChunkSize,
}

pub struct TransducerSerializeReport {
    pub index_table_chunks: usize,
    pub transition_table_chunks: usize,
}

impl HfstTransducer {
    pub fn from_mapped_memory(buf: Arc<Mmap>) -> HfstTransducer {
        let header = TransducerHeader::new(&buf);
        let alphabet_offset = header.len();
        let alphabet =
            TransducerAlphabet::new(&buf[alphabet_offset..buf.len()], header.symbol_count());

        let index_table_offset = alphabet_offset + alphabet.len();

        let index_table_end = index_table_offset + INDEX_TABLE_SIZE * header.index_table_size();
        let index_table = IndexTable::new(
            buf.clone(),
            index_table_offset,
            index_table_end,
            header.index_table_size() as u32,
        );

        let trans_table_end = index_table_end + TRANS_TABLE_SIZE * header.target_table_size();
        let trans_table = TransitionTable::new(
            buf.clone(),
            index_table_end,
            trans_table_end,
            header.target_table_size() as u32,
        );

        HfstTransducer {
            buf: buf,
            header: header,
            alphabet: alphabet,
            index_table: index_table,
            transition_table: trans_table,
        }
    }

    pub fn serialize(
        &self,
        chunk_size: usize,
        target_dir: &std::path::Path,
    ) -> Result<(), TransducerSerializeError> {
        if chunk_size % 8 != 0 {
            return Err(TransducerSerializeError::InvalidChunkSize);
        }

        // Ensure target path exists
        if !target_dir.exists() {
            eprintln!("Creating directory: {:?}", target_dir);
            std::fs::create_dir_all(target_dir).expect("create target dir");
        }

        // Write index table chunks
        eprintln!(
            "Writing index table... (Size: {})",
            self.index_table().len()
        );
        let index_table_count = self
            .index_table()
            .serialize(chunk_size, target_dir)
            .unwrap();

        // Write transition table chunks
        eprintln!("Writing transition table...");
        let transition_table_count = self
            .transition_table()
            .serialize(chunk_size, target_dir)
            .unwrap();

        // Write header + meta index
        let meta = self::chunk::MetaRecord {
            index_table_count,
            transition_table_count,
            chunk_size,
            raw_alphabet: self.alphabet().key_table().to_owned(),
        };

        eprintln!("Writing meta index...");
        meta.serialize(target_dir);

        Ok(())
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

    pub fn is_weighted(&self) -> bool {
        self.header.has_flag(HeaderFlag::Weighted)
    }

    pub fn header(&self) -> &TransducerHeader {
        &self.header
    }
}

impl Transducer for HfstTransducer {
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            self.transition_table.is_final(i - TARGET_TABLE)
        } else {
            self.index_table.is_final(i)
        }
    }

    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            self.transition_table.weight(i - TARGET_TABLE)
        } else {
            self.index_table.final_weight(i)
        }
    }

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
            match self.index_table.input_symbol(i + sym as u32) {
                Some(res) => sym == res,
                None => false,
            }
        }
    }

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

    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        if let Some(0) = self.transition_table.input_symbol(i) {
            Some(self.transition_table.symbol_transition(i))
        } else {
            None
        }
    }

    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
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

    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + 1)
        } else if let Some(v) = self.index_table.target(i + 1 + symbol as u32) {
            Some(v - TARGET_TABLE)
        } else {
            None
        }
    }

    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.transition_table().input_symbol(i)
    }

    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    fn mut_alphabet(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }
}
