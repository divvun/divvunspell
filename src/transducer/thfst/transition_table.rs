use std::{mem, ptr};

use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use memmap::Mmap;

pub(super) struct TransitionTable {
    buf: Mmap,
    size: u32,
}

const TRANS_TABLE_SIZE: usize = 12;

impl TransitionTable {
    // pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
    //     let file = File::open(path)?;
    //     let buf = unsafe { Mmap::map(&file)? };
    //     let size = (buf.len() / TRANS_TABLE_SIZE) as u32;
    //     Ok(TransitionTable { buf, size })
    // }

    #[inline]
    fn read_symbol_from_cursor(&self, index: usize) -> Option<SymbolNumber> {
        let x = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };
        if x == std::u16::MAX {
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
        if x == std::u32::MAX {
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