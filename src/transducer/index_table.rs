use std::{u16, u32};
use std::fmt;
use std::ptr;

use memmap::Mmap;
use std::sync::Arc;
use crate::types::{TransitionTableIndex, SymbolNumber, Weight};
use crate::constants::TRANS_INDEX_SIZE;

pub struct IndexTable {
    size: TransitionTableIndex,
    mmap: Arc<Mmap>,
    offset: usize,
    len: usize
}

impl fmt::Debug for IndexTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Index table index: {}", self.size)?;
        Ok(())
    }
}

impl IndexTable {
    pub fn new(buf: Arc<Mmap>, offset: usize, len: usize, size: TransitionTableIndex) -> IndexTable {
        IndexTable {
            size: size,
            mmap: buf,
            offset,
            len
        }
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + TRANS_INDEX_SIZE * i as usize;
        let input_symbol: SymbolNumber = unsafe {
            ptr::read(self.mmap.as_ptr().offset(index as isize) as *const _)
        };

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
        
        let index = self.offset + TRANS_INDEX_SIZE * i as usize;
        let target: TransitionTableIndex = unsafe {
            ptr::read(self.mmap.as_ptr().offset((index + 2) as isize) as *const _)
        };

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

        let index = self.offset + TRANS_INDEX_SIZE * i as usize;
        let weight: Weight = unsafe {
            ptr::read(self.mmap.as_ptr().offset((index + 2) as isize) as *const _)
        };
        
        Some(weight)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
