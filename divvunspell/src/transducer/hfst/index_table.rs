// We manually ensure alignment of reads in this file.
#![allow(clippy::cast_ptr_alignment)]

use byteorder::{LittleEndian, ReadBytesExt};
use std::fmt;
use std::io::Cursor;
use std::mem;
use std::ptr;
use std::{u16, u32};

use crate::constants::INDEX_TABLE_SIZE;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use memmap::Mmap;
use std::sync::Arc;

pub struct MappedIndexTable {
    pub(crate) size: TransitionTableIndex,
    pub(crate) mmap: Arc<Mmap>,
    pub(crate) offset: usize,
    pub(crate) len: usize,
}

impl fmt::Debug for MappedIndexTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Index table index: {}", self.size)?;
        Ok(())
    }
}

#[allow(clippy::len_without_is_empty)]
impl MappedIndexTable {
    pub fn new(
        buf: Arc<Mmap>,
        offset: usize,
        len: usize,
        size: TransitionTableIndex,
    ) -> MappedIndexTable {
        MappedIndexTable {
            size,
            mmap: buf,
            offset,
            len,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len - self.offset
    }

    #[inline(always)]
    fn make_cursor<'a>(&'a self) -> Cursor<&'a [u8]> {
        Cursor::new(&self.mmap)
    }

    #[inline(always)]
    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + INDEX_TABLE_SIZE * i as usize;

        let input_symbol: SymbolNumber =
            if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
                let mut cursor = self.make_cursor();
                cursor.set_position(index as u64);
                cursor.read_u16::<LittleEndian>().unwrap()
            } else {
                unsafe { ptr::read(self.mmap.as_ptr().add(index) as *const _) }
            };

        if input_symbol == u16::MAX {
            None
        } else {
            Some(input_symbol)
        }
    }

    #[inline(always)]
    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + INDEX_TABLE_SIZE * i as usize;
        let target: TransitionTableIndex =
            if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
                let mut cursor = self.make_cursor();
                cursor.set_position((index + mem::size_of::<SymbolNumber>()) as u64);
                cursor.read_u32::<LittleEndian>().unwrap()
            } else {
                unsafe { ptr::read(self.mmap.as_ptr().add(index + 2) as *const _) }
            };

        if target == u32::MAX {
            None
        } else {
            Some(target)
        }
    }

    // Final weight reads from the same position as target, but for a different tuple
    // This can probably be abstracted out more nicely
    #[inline(always)]
    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + INDEX_TABLE_SIZE * i as usize;
        let weight: Weight = {
            let mut cursor = self.make_cursor();
            cursor.set_position((index + mem::size_of::<SymbolNumber>()) as u64);
            cursor.read_f32::<LittleEndian>().unwrap()
        };

        Some(weight)
    }

    #[inline(always)]
    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
