use crate::vfs::Map;
use std::fmt;
use std::mem;
use std::sync::Arc;

use crate::constants::TRANS_TABLE_SIZE;
use crate::transducer::dhfst::{u16_at, u32_at};
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

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
