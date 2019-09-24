use std::ptr;

use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use memmap::Mmap;

pub(super) struct IndexTable {
    buf: Mmap,
    size: u32,
}

const INDEX_TABLE_SIZE: usize = 8;

impl IndexTable {
    // pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
    //     let file = File::open(path)?;
    //     let buf = unsafe { Mmap::map(&file)? };
    //     let size = (buf.len() / INDEX_TABLE_SIZE) as u32;
    //     Ok(IndexTable { buf, size })
    // }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = INDEX_TABLE_SIZE * i as usize;

        let input_symbol: SymbolNumber =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        if input_symbol == std::u16::MAX {
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

        if target == std::u32::MAX {
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
        let weight: Weight = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        Some(weight)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
