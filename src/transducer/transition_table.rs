use byteorder::{LittleEndian, ReadBytesExt};
use memmap::Mmap;
use std::fmt;
use std::io::Cursor;
use std::ptr;
use std::sync::Arc;
use std::{mem, u16, u32};

use crate::constants::TRANS_SIZE;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

pub struct TransitionTable {
    size: TransitionTableIndex,
    mmap: Arc<Mmap>,
    offset: usize,
    len: usize,
}

impl fmt::Debug for TransitionTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Transition table index: {}", self.size)?;
        Ok(())
    }
}

impl TransitionTable {
    pub fn new(mmap: Arc<Mmap>, offset: usize, len: usize, size: u32) -> TransitionTable {
        TransitionTable {
            size: size,
            mmap,
            offset,
            len,
        }
    }

    fn make_cursor(&self) -> Cursor<&[u8]> {
        Cursor::new(&self.mmap)
    }

    #[inline]
    fn read_symbol_from_cursor(&self, index: usize) -> Option<SymbolNumber> {
        let index = self.offset + index;
        let x: SymbolNumber = if cfg!(target_arch = "arm") {
            let mut cursor = self.make_cursor();
            cursor.set_position(index as u64);
            cursor.read_u16::<LittleEndian>().unwrap()
        } else {
            unsafe { ptr::read(self.mmap.as_ptr().offset(index as isize) as *const _) }
        };
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

        let index = TRANS_SIZE as usize * i as usize;
        let sym = self.read_symbol_from_cursor(index);
        sym
    }

    pub fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = ((TRANS_SIZE * i as usize) + mem::size_of::<SymbolNumber>()) as usize;
        self.read_symbol_from_cursor(index)
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index =
            self.offset + ((TRANS_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>()));

        let x: TransitionTableIndex = if cfg!(target_arch = "arm") {
            let mut cursor = self.make_cursor();
            cursor.set_position(index as u64);
            cursor.read_u32::<LittleEndian>().unwrap()
        } else {
            unsafe { ptr::read(self.mmap.as_ptr().offset(index as isize) as *const _) }
        };
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

        let index = self.offset
            + ((TRANS_SIZE * i as usize)
                + (2 * mem::size_of::<SymbolNumber>())
                + mem::size_of::<TransitionTableIndex>());

        let x: Weight = if cfg!(target_arch = "arm") {
            let mut cursor = self.make_cursor();
            cursor.set_position(index as u64);
            cursor.read_f32::<LittleEndian>().unwrap()
        } else {
            unsafe { ptr::read(self.mmap.as_ptr().offset(index as isize) as *const _) }
        };
        Some(x)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.output_symbol(i) == None && self.target(i) == Some(1)
    }

    pub fn symbol_transition(&self, i: TransitionTableIndex) -> SymbolTransition {
        SymbolTransition::new(self.target(i), self.output_symbol(i), self.weight(i))
    }
}
