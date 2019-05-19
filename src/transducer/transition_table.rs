use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use memmap::Mmap;
use std::fmt;
use std::io::Cursor;
use std::ptr;
use std::sync::Arc;
use std::{mem, u16, u32, cmp};

use crate::constants::TRANS_TABLE_SIZE;
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
    
    pub fn serialize(&self, chunk_size: usize, target_dir: &std::path::Path) -> Result<usize, ()> {
        eprintln!("size: {}, len: {}, offset: {}", self.size, self.len, self.offset);

        if chunk_size % 12 != 0 {
            panic!("Chunk size must be divisible by 12");
        }

        // Size is the number of indexes, and that multiplied by TRANS_TABLE_SIZE is the total byte size
        let total_bytes = self.len - self.offset;

        // How many indexes can we get per chunk size?
        let max_index_per_iter = chunk_size / TRANS_TABLE_SIZE as usize;

        // Divide the chunks
        let has_excess = total_bytes % chunk_size != 0;
        let chunk_count = total_bytes / chunk_size + (if has_excess { 1 } else { 0 });
        eprintln!("Chunk count: {} max index per iter: {} total bytes: {}", chunk_count, max_index_per_iter, total_bytes);

        for i in 1usize..=chunk_count {
            eprintln!("Writing chunk: {}", i);

            let filename = format!("transition-{:02}", i - 1);
            let mut file = std::fs::File::create(target_dir.join(filename)).unwrap();
            
            // TODO: Check these aren't off by one
            let begin = (max_index_per_iter * (i-1usize)) as u32;
            let end = cmp::min(max_index_per_iter * i, self.size as usize) as u32;

            eprintln!("Chunk {}: {}..{}", i, begin, end);

            for index in begin..end {
                let input_symbol = self.input_symbol(index).unwrap_or(u16::MAX);
                let output_symbol = self.output_symbol(index).unwrap_or(u16::MAX);
                let target = self.target(index).unwrap_or(u32::MAX);
                let weight = self.weight(index).unwrap();

                file.write_u16::<LittleEndian>(input_symbol).unwrap();
                file.write_u16::<LittleEndian>(output_symbol).unwrap();
                file.write_u32::<LittleEndian>(target).unwrap();
                file.write_u32::<LittleEndian>(unsafe { std::mem::transmute::<f32, u32>(weight) }).unwrap();
            }
        }

        eprintln!("Done transition serialize.");

        Ok(chunk_count as usize)
    }

    fn make_cursor(&self) -> Cursor<&[u8]> {
        Cursor::new(&self.mmap)
    }

    #[inline]
    fn read_symbol_from_cursor(&self, index: usize) -> Option<SymbolNumber> {
        let index = self.offset + index;
        let x: SymbolNumber = if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
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

        let index =
            self.offset + ((TRANS_TABLE_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>()));

        let x: TransitionTableIndex = if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
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
            + ((TRANS_TABLE_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>()) + mem::size_of::<TransitionTableIndex>());

        let x: Weight = if cfg!(all(target_arch = "arm", target_pointer_width = "32")) {
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
