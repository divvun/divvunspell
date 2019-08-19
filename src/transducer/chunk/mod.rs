#![allow(clippy::cast_ptr_alignment)] // FIXME: This at least needs a comment

use std::fs::File;
use std::mem;
use std::ptr;
use std::{u16, u32};

use crate::constants::TARGET_TABLE;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use memmap::Mmap;
use serde_derive::{Deserialize, Serialize};
use smol_str::SmolStr;

mod alphabet;

use self::alphabet::TransducerAlphabetParser;
use super::TransducerAlphabet;
use crate::transducer::Transducer;

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
    pub raw_alphabet: Vec<String>,
}

impl MetaRecord {
    pub fn serialize(&self, target_dir: &std::path::Path) {
        use std::io::Write;

        let s = serde_json::to_string_pretty(self).unwrap();
        let mut f = std::fs::File::create(target_dir.join("meta")).unwrap();
        writeln!(f, "{}", s).unwrap();
    }
}

struct IndexTable {
    buf: Mmap,
    size: u32,
}

const INDEX_TABLE_SIZE: usize = 8;

impl IndexTable {
    pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let buf = unsafe { Mmap::map(&file)? };
        let size = (buf.len() / INDEX_TABLE_SIZE) as u32;
        Ok(IndexTable { buf, size })
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = INDEX_TABLE_SIZE * i as usize;

        let input_symbol: SymbolNumber =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        if input_symbol == u16::MAX {
            None
        } else {
            Some(input_symbol)
        }
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index = (INDEX_TABLE_SIZE * i as usize) + 4;
        let target: TransitionTableIndex =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        if target == u32::MAX {
            None
        } else {
            Some(target)
        }
    }

    // Final weight reads from the same position as target, but for a different tuple
    // This can probably be abstracted out more nicely
    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = (INDEX_TABLE_SIZE * i as usize) + 4;
        let weight: Weight =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        Some(weight)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}

struct TransitionTable {
    buf: Mmap,
    size: u32,
}

const TRANS_TABLE_SIZE: usize = 12;

impl TransitionTable {
    pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let buf = unsafe { Mmap::map(&file)? };
        let size = (buf.len() / TRANS_TABLE_SIZE) as u32;
        Ok(TransitionTable { buf, size })
    }

    #[inline]
    fn read_symbol_from_cursor(&self, index: usize) -> Option<SymbolNumber> {
        let x = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };
        if x == u16::MAX {
            None
        } else {
            Some(x)
        }
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = TRANS_TABLE_SIZE as usize * i as usize;
        let sym = self.read_symbol_from_cursor(index);
        sym
    }

    pub fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = ((TRANS_TABLE_SIZE * i as usize) + mem::size_of::<SymbolNumber>()) as usize;
        self.read_symbol_from_cursor(index)
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index = (TRANS_TABLE_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>());

        let x: TransitionTableIndex =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };
        if x == u32::MAX {
            None
        } else {
            Some(x)
        }
    }

    pub fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = (TRANS_TABLE_SIZE * i as usize) + 8;

        let x: Weight = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        Some(x)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.output_symbol(i) == None && self.target(i) == Some(1)
    }

    pub fn symbol_transition(&self, i: TransitionTableIndex) -> SymbolTransition {
        SymbolTransition::new(self.target(i), self.output_symbol(i), self.weight(i))
    }
}

pub struct ChfstTransducer {
    // meta: MetaRecord,
    index_tables: Vec<IndexTable>,
    indexes_per_chunk: u32,
    transition_tables: Vec<TransitionTable>,
    transitions_per_chunk: u32,
    alphabet: TransducerAlphabet,
}

impl ChfstTransducer {
    pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
        // Load meta
        let meta_file = File::open(path.join("meta")).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("`meta` not found in transducer path, looked for {}", path.join("meta").display()),
            )
        })?;
        let meta: MetaRecord = serde_json::from_reader(meta_file)?;

        let mut index_tables = vec![];
        for i in 0..meta.index_table_count {
            let filename = format!("index-{:02}", i);
            let fpath = path.join(&filename);
            let index_table = IndexTable::from_path(&fpath).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    &*format!("{} not found in transducer path", &filename),
                )
            })?;
            index_tables.push(index_table);
        }

        let indexes_per_chunk = meta.chunk_size as u32 / 8u32;

        let mut transition_tables = vec![];
        for i in 0..meta.transition_table_count {
            let filename = format!("transition-{:02}", i);
            let fpath = path.join(&filename);
            let transition_table = TransitionTable::from_path(&fpath).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    &*format!("{} not found in transducer path", &filename),
                )
            })?;
            transition_tables.push(transition_table);
        }

        let transitions_per_chunk = meta.chunk_size as u32 / 12u32;

        let alphabet = TransducerAlphabetParser::parse(&meta.raw_alphabet);

        Ok(ChfstTransducer {
            // meta,
            index_tables,
            indexes_per_chunk,
            transition_tables,
            transitions_per_chunk,
            alphabet,
        })
    }

    #[inline]
    fn transition_rel_index(&self, x: TransitionTableIndex) -> (usize, TransitionTableIndex) {
        let index_page = x / self.transitions_per_chunk;
        let relative_index = x - (self.transitions_per_chunk * index_page);
        (index_page as usize, relative_index)
    }

    #[inline]
    fn index_rel_index(&self, x: TransitionTableIndex) -> (usize, TransitionTableIndex) {
        let index_page = x / self.indexes_per_chunk;
        let relative_index = x - (self.indexes_per_chunk * index_page);
        (index_page as usize, relative_index)
    }
}

impl Transducer for ChfstTransducer {
    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    fn mut_alphabet(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }

    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        let (page, index) = self.transition_rel_index(i);
        self.transition_tables[page].input_symbol(index)
    }

    fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            self.transition_tables[page].is_final(index)
        } else {
            let (page, index) = self.index_rel_index(i);
            self.index_tables[page].is_final(index)
        }
    }

    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            self.transition_tables[page].weight(index)
        } else {
            let (page, index) = self.index_rel_index(i);
            self.index_tables[page].final_weight(index)
        }
    }

    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        let sym = match s {
            Some(v) => v,
            None => return false,
        };

        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            match self.transition_tables[page].input_symbol(index) {
                Some(res) => sym == res,
                None => false,
            }
        } else {
            let (page, index) = self.index_rel_index(i + u32::from(sym));
            match self.index_tables[page].input_symbol(index) {
                Some(res) => sym == res,
                None => false,
            }
        }
    }

    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            match self.transition_tables[page].input_symbol(index) {
                Some(sym) => sym == 0 || self.alphabet.is_flag(sym),
                None => false,
            }
        } else {
            let (page, index) = self.index_rel_index(i);
            if let Some(0) = self.index_tables[page].input_symbol(index) {
                true
            } else {
                false
            }
        }
    }

    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = self.transition_rel_index(i);

        if let Some(0) = self.transition_tables[page].input_symbol(index) {
            Some(self.transition_tables[page].symbol_transition(index))
        } else {
            None
        }
    }

    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = self.transition_rel_index(i);

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

    fn take_non_epsilons(
        &self,
        i: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<SymbolTransition> {
        let (page, index) = self.transition_rel_index(i);
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

    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + 1)
        } else {
            let (page, index) = self.index_rel_index(i + 1 + u32::from(symbol));

            if let Some(v) = self.index_tables[page].target(index) {
                Some(v - TARGET_TABLE)
            } else {
                None
            }
        }
    }
}

use crate::speller::Speller;
use std::sync::Arc;

pub struct ChfstBundle {
    pub lexicon: ChfstTransducer,
    pub mutator: ChfstTransducer,
}

impl ChfstBundle {
    pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let lexicon = ChfstTransducer::from_path(&path.join("lexicon"))?;
        let mutator = ChfstTransducer::from_path(&path.join("mutator"))?;

        Ok(ChfstBundle { lexicon, mutator })
    }

    pub fn speller(self) -> Arc<Speller<ChfstTransducer>> {
        Speller::new(self.mutator, self.lexicon)
    }
}
