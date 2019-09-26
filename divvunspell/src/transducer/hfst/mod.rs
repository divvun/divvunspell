pub mod alphabet;
pub mod header;
pub mod index_table;
pub mod transition_table;

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use memmap::Mmap;

pub use self::index_table::IndexTable;
pub use self::transition_table::TransitionTable;
use super::alphabet::TransducerAlphabet;
use super::symbol_transition::SymbolTransition;
use super::{Transducer, TransducerError};
use crate::constants::{INDEX_TABLE_SIZE, TARGET_TABLE, TRANS_TABLE_SIZE};
use crate::types::{HeaderFlag, SymbolNumber, TransitionTableIndex, Weight};
use crate::util::{self, Filesystem, ToMemmap};
use self::alphabet::TransducerAlphabetParser;
use self::header::TransducerHeader;

pub struct HfstTransducer {
    buf: Arc<Mmap>,
    header: TransducerHeader,
    alphabet: TransducerAlphabet,
    pub(crate) index_table: IndexTable,
    pub(crate) transition_table: TransitionTable,
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

impl HfstTransducer {
    #[inline(always)]
    pub fn from_mapped_memory(buf: Arc<Mmap>) -> HfstTransducer {
        let header = TransducerHeader::new(&buf);
        let alphabet_offset = header.len();
        let alphabet = TransducerAlphabetParser::parse(
            &buf[alphabet_offset..buf.len()],
            header.symbol_count(),
        );

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
            buf,
            header,
            alphabet,
            index_table,
            transition_table: trans_table,
        }
    }

    #[inline(always)]
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    #[inline(always)]
    pub fn is_weighted(&self) -> bool {
        self.header.has_flag(HeaderFlag::Weighted)
    }

    #[inline(always)]
    pub fn header(&self) -> &TransducerHeader {
        &self.header
    }
}

impl Transducer for HfstTransducer {
    const FILE_EXT: &'static str = "hfst";

    fn from_path<P, FS, F>(fs: &FS, path: P) -> Result<HfstTransducer, TransducerError>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
        F: util::File + ToMemmap,
    {
        let file = fs.open(path).map_err(|e| TransducerError::Io(e))?;
        let mmap = unsafe { file.memory_map() }.map_err(|e| TransducerError::Memmap(e))?;
        Ok(HfstTransducer::from_mapped_memory(Arc::new(mmap)))
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
