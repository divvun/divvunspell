use crate::vfs::Map;
use std::fmt;
use std::mem;
use std::sync::Arc;

use crate::constants::TRANS_TABLE_SIZE;
use crate::transducer::dhfst::{u16_at, u32_at};
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

/// One transition record, every field as stored: a symbol or target of all
/// ones bits is none.
#[derive(Clone, Copy)]
pub(crate) struct Record {
    pub(crate) input: u16,
    pub(crate) output: u16,
    pub(crate) target: u32,
    pub(crate) weight: f32,
}

pub struct MappedTransitionTable {
    pub(crate) size: TransitionTableIndex,
    pub(crate) mmap: Arc<Map>,
    pub(crate) offset: usize,
}

impl fmt::Debug for MappedTransitionTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Transition table index: {}", self.size)?;
        Ok(())
    }
}

impl MappedTransitionTable {
    #[inline(always)]
    pub fn new(mmap: Arc<Map>, offset: usize, size: TransitionTableIndex) -> MappedTransitionTable {
        MappedTransitionTable { size, mmap, offset }
    }

    /// Record `i` whole, read with one bounds check, or `None` past the end
    /// of the table.
    #[inline(always)]
    pub(crate) fn record(&self, i: TransitionTableIndex) -> Option<Record> {
        if i >= self.size {
            return None;
        }
        let at = self.offset + TRANS_TABLE_SIZE * i.0 as usize;
        let bytes: &[u8; TRANS_TABLE_SIZE] = self.mmap.get(at..)?.first_chunk()?;
        Some(Record {
            input: u16::from_le_bytes([bytes[0], bytes[1]]),
            output: u16::from_le_bytes([bytes[2], bytes[3]]),
            target: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            weight: f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        })
    }

    #[inline(always)]
    fn read_symbol(&self, index: usize) -> Option<SymbolNumber> {
        let index = self.offset + index;
        let x = SymbolNumber(u16_at(&self.mmap, index));
        if x == SymbolNumber::MAX {
            None
        } else {
            Some(x)
        }
    }

    #[inline(always)]
    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = TRANS_TABLE_SIZE as usize * i.0 as usize;
        self.read_symbol(index)
    }

    #[inline(always)]
    pub fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = ((TRANS_TABLE_SIZE * i.0 as usize) + mem::size_of::<SymbolNumber>()) as usize;
        self.read_symbol(index)
    }

    #[inline(always)]
    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index = self.offset
            + ((TRANS_TABLE_SIZE * i.0 as usize) + (2 * mem::size_of::<SymbolNumber>()));

        let x = TransitionTableIndex(u32_at(&self.mmap, index));
        if x == TransitionTableIndex::MAX {
            None
        } else {
            Some(x)
        }
    }

    #[inline(always)]
    pub fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = self.offset
            + ((TRANS_TABLE_SIZE * i.0 as usize)
                + (2 * mem::size_of::<SymbolNumber>())
                + mem::size_of::<TransitionTableIndex>());

        Some(Weight(f32::from_bits(u32_at(&self.mmap, index))))
    }

    #[inline(always)]
    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None
            && self.output_symbol(i) == None
            && self.target(i) == Some(TransitionTableIndex::ONE)
    }

    #[inline(always)]
    pub fn symbol_transition(&self, i: TransitionTableIndex) -> SymbolTransition {
        SymbolTransition::new(self.target(i), self.output_symbol(i), self.weight(i))
    }
}
