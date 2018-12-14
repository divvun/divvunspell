use byteorder::{LittleEndian, ReadBytesExt};
use std::fmt;
use std::io::Cursor;
use std::mem;
use std::ptr;
use std::{u16, u32};

use crate::constants::TRANS_INDEX_SIZE;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use memmap::Mmap;
use std::sync::Arc;

pub struct IndexTable {
    size: TransitionTableIndex,
    mmap: Arc<Mmap>,
    offset: usize,
    len: usize,
}

impl fmt::Debug for IndexTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Index table index: {}", self.size)?;
        Ok(())
    }
}

impl IndexTable {
    pub fn new(
        buf: Arc<Mmap>,
        offset: usize,
        len: usize,
        size: TransitionTableIndex,
    ) -> IndexTable {
        IndexTable {
            size: size,
            mmap: buf,
            offset,
            len,
        }
    }

    fn make_cursor<'a>(&'a self) -> Cursor<&'a [u8]> {
        Cursor::new(&self.mmap)
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + TRANS_INDEX_SIZE * i as usize;

        let input_symbol: SymbolNumber = if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
            let mut cursor = self.make_cursor();
            cursor.set_position(index as u64);
            cursor.read_u16::<LittleEndian>().unwrap()
        } else {
            unsafe { ptr::read(self.mmap.as_ptr().offset(index as isize) as *const _) }
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
        let target: TransitionTableIndex = if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
            let mut cursor = self.make_cursor();
            cursor.set_position((index + mem::size_of::<SymbolNumber>()) as u64);
            cursor.read_u32::<LittleEndian>().unwrap()
        } else {
            unsafe { ptr::read(self.mmap.as_ptr().offset((index + 2) as isize) as *const _) }
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
        let weight: Weight = if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
            let mut cursor = self.make_cursor();
            cursor.set_position((index + mem::size_of::<SymbolNumber>()) as u64);
            cursor.read_f32::<LittleEndian>().unwrap()
        } else {
            unsafe { ptr::read(self.mmap.as_ptr().offset((index + 2) as isize) as *const _) }
        };

        Some(weight)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
