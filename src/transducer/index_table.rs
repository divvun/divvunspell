#![allow(clippy::cast_ptr_alignment)] // FIXME: This at least needs a comment

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::cmp;
use std::fmt;
use std::io::Cursor;
use std::mem;
use std::ptr;
use std::{u16, u32};

use crate::constants::INDEX_TABLE_SIZE;
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

    pub fn serialize(&self, chunk_size: usize, target_dir: &std::path::Path) -> Result<usize, ()> {
        eprintln!(
            "size: {}, len: {}, offset: {}",
            self.size, self.len, self.offset
        );

        if chunk_size % 8 != 0 {
            panic!("Chunk size must be divisible by 8");
        }

        // Size is the number of indexes, and that multiplied by TRANS_TABLE_SIZE is the total byte size
        let real_total_bytes = self.len - self.offset;

        // We're converting this from 6 byte width to 8, so we need to multiply our output
        let total_bytes = real_total_bytes / 6 * 8;

        // How many indexes can we get per chunk size?
        let max_index_per_iter = chunk_size / 8usize;

        // Divide the chunks
        let has_excess = total_bytes % chunk_size != 0;
        let chunk_count = total_bytes / chunk_size + (if has_excess { 1 } else { 0 });
        eprintln!(
            "Chunk count: {} max index per iter: {} total bytes: {}",
            chunk_count, max_index_per_iter, total_bytes
        );

        for i in 1usize..=chunk_count {
            eprintln!("Writing chunk: {}", i);

            let filename = format!("index-{:02}", i - 1);
            let mut file = std::fs::File::create(target_dir.join(filename)).unwrap();

            let begin = (max_index_per_iter * (i - 1usize)) as u32;
            let end = cmp::min(max_index_per_iter * i, self.size as usize) as u32;

            eprintln!("Chunk {}: {}..{}", i, begin, end);

            for index in begin..end {
                let input_symbol = self.input_symbol(index).unwrap_or(u16::MAX);
                let targetish = self.target(index).unwrap_or(u32::MAX);

                file.write_u16::<LittleEndian>(input_symbol).unwrap();
                file.write_u16::<LittleEndian>(0).unwrap();
                file.write_u32::<LittleEndian>(targetish).unwrap();
            }
        }

        Ok(chunk_count)
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
        let weight: Weight = if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
            let mut cursor = self.make_cursor();
            cursor.set_position((index + mem::size_of::<SymbolNumber>()) as u64);
            cursor.read_f32::<LittleEndian>().unwrap()
        } else {
            unsafe { ptr::read(self.mmap.as_ptr().add(index + 2) as *const _) }
        };

        Some(weight)
    }

    #[inline(always)]
    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
