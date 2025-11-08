use std::path::Path;

use crate::constants::TARGET_TABLE;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

use super::index_table::IndexTable;
use super::transition_table::TransitionTable;
use crate::transducer::{Transducer, TransducerAlphabet, TransducerError, TransducerLoader};
use crate::vfs::{self, Filesystem};

/// Tromsø-Helsinki Finite State Transducer format with chunked memory mapping.
///
/// This variant loads transducers in multiple chunks, which is useful for very large
/// transducers that might exceed system memory mapping limits or address space.
#[derive(Debug)]
pub struct ThfstChunkedTransducer {
    // meta: MetaRecord,
    index_tables: Vec<IndexTable<memmap2::Mmap>>,
    indexes_per_chunk: TransitionTableIndex,
    transition_tables: Vec<TransitionTable<memmap2::Mmap>>,
    transitions_per_chunk: TransitionTableIndex,
    alphabet: TransducerAlphabet,
}

/// Type alias for memory-mapped chunked THFST transducer.
pub type MmapThfstChunkedTransducer = ThfstChunkedTransducer;

macro_rules! transition_rel_index {
    ($self:expr_2021, $x:expr_2021) => {{
        let index_page = $x / $self.transitions_per_chunk;
        let relative_index = $x - ($self.transitions_per_chunk * index_page);
        (index_page.0 as usize, relative_index)
    }};
}

macro_rules! index_rel_index {
    ($self:expr_2021, $x:expr_2021) => {{
        let index_page = $x / $self.indexes_per_chunk;
        let relative_index = $x - ($self.indexes_per_chunk * index_page);
        (index_page.0 as usize, relative_index)
    }};
}

macro_rules! error {
    ($path:path, $name:expr_2021) => {
        TransducerError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "`{}` not found in transducer path, looked for {}",
                $name,
                $path.join($name).display()
            ),
        ))
    };
}

impl Transducer for ThfstChunkedTransducer {
    const FILE_EXT: &'static str = "thfst";

    #[inline(always)]
    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    #[inline(always)]
    fn alphabet_mut(&mut self) -> &mut TransducerAlphabet {
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
            if page >= self.transition_tables.len() {
                return false;
            }
            match self.transition_tables[page].input_symbol(index) {
                Some(res) => sym == res,
                None => false,
            }
        } else {
            tracing::trace!("has_transitions: i:{} s:{:?}", i, s);
            let (page, index) = index_rel_index!(self, i + TransitionTableIndex(sym.0 as u32));
            tracing::trace!("has_transitions: page:{} index:{:?}", page, index);
            if page >= self.index_tables.len() {
                return false;
            }
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
                Some(sym) => sym == SymbolNumber::ZERO || self.alphabet.is_flag(sym),
                None => false,
            }
        } else {
            let (page, index) = index_rel_index!(self, i);
            if let Some(SymbolNumber::ZERO) = self.index_tables[page].input_symbol(index) {
                true
            } else {
                false
            }
        }
    }

    #[inline(always)]
    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = transition_rel_index!(self, i);

        if let Some(SymbolNumber::ZERO) = self.transition_tables[page].input_symbol(index) {
            Some(self.transition_tables[page].symbol_transition(index))
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = transition_rel_index!(self, i);

        if let Some(sym) = self.transition_tables[page].input_symbol(index) {
            if sym != SymbolNumber::ZERO && !self.alphabet.is_flag(sym) {
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
            Some(i - TARGET_TABLE + TransitionTableIndex(1))
        } else {
            let (page, index) =
                index_rel_index!(self, i + TransitionTableIndex(symbol.0 as u32 + 1));

            if let Some(v) = self.index_tables[page].target(index) {
                Some(v - TARGET_TABLE)
            } else {
                None
            }
        }
    }
}

impl<F: vfs::File> TransducerLoader<F> for ThfstChunkedTransducer {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
    {
        let path = path.as_ref();
        let alphabet_file = fs
            .open_file(&path.join("alphabet"))
            .map_err(|_| error!(path, "alphabet"))?;

        let alphabet: TransducerAlphabet = serde_json::from_reader(alphabet_file)
            .map_err(|e| TransducerError::Alphabet(Box::new(e)))?;

        let mut index_chunk_count = 1;
        let index_tables;

        loop {
            let index_path = path.join("index");
            let indexes = (0..index_chunk_count)
                .map(|i| IndexTable::from_path_partial(fs, &index_path, i, index_chunk_count))
                .collect::<Result<Vec<_>, _>>();

            match indexes {
                Ok(v) => {
                    index_tables = v;
                    break;
                }
                Err(TransducerError::Memmap(_)) => {
                    index_chunk_count *= 2;

                    if index_chunk_count > 16 {
                        return Err(TransducerError::Memmap(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Could not memory map index table in 16 chunks",
                        )));
                    }
                }
                Err(e) => return Err(e),
            }
        }

        let mut trans_chunk_count = 1;
        let transition_tables;

        loop {
            let trans_path = path.join("transition");
            let tables = (0..trans_chunk_count)
                .map(|i| TransitionTable::from_path_partial(fs, &trans_path, i, trans_chunk_count))
                .collect::<Result<Vec<_>, _>>();

            match tables {
                Ok(v) => {
                    transition_tables = v;
                    break;
                }
                Err(TransducerError::Memmap(_)) => {
                    trans_chunk_count *= 2;

                    if trans_chunk_count > 16 {
                        return Err(TransducerError::Memmap(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Could not memory map transition table in 16 chunks",
                        )));
                    }
                }
                Err(e) => return Err(e),
            }
        }

        let transducer = ThfstChunkedTransducer {
            indexes_per_chunk: index_tables[0].size,
            transitions_per_chunk: transition_tables[0].size,
            index_tables,
            transition_tables,
            alphabet,
        };

        tracing::debug!("{:#?}", transducer);

        Ok(transducer)
    }
}
