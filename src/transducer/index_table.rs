use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;
use std::{mem, u16, u32};
use std::fmt;

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
        //let o: Vec<i8> = buf[0..16].iter().map(|x| *x as i8).collect();
        // debug!("IndexTable: {:?}", &buf[0..32]);

        IndexTable {
            size: size,
            mmap: buf,
            offset,
            len,
        }
    }

    fn make_cursor<'a>(&'a self) -> Cursor<&'a [u8]> {
        Cursor::new(&self.mmap[self.offset..self.len])
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }
        let index = TRANS_INDEX_SIZE * i as usize;
        // let mut cursor = self.cursor.clone();
        let mut cursor = self.make_cursor();
        cursor.set_position(index as u64);
        let x = cursor.read_u16::<LittleEndian>().unwrap();
        if x == u16::MAX {
            None
        } else {
            Some(x)
        }
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index: u64 = (TRANS_INDEX_SIZE * (i as usize) + mem::size_of::<SymbolNumber>()) as u64;
        // let mut cursor = self.cursor.clone();
        let mut cursor = self.make_cursor();
        cursor.set_position(index);
        let x = cursor.read_u32::<LittleEndian>().unwrap();
        if x == u32::MAX {
            None
        } else {
            Some(x)
        }
    }

    /* In weighted transducers, transition entries are suffixed with a 4-byte IEEE 754 float representing weight. For final transitions, this must be

static_cast<float>(UINT_MAX)

Final indices also have a weight in place of their target index, denoting an additional final weight for that index.

*/

    // Final weight reads from the same position as target, but for a different tuple
    // This can probably be abstracted out more nicely
    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index: u64 = (TRANS_INDEX_SIZE * (i as usize) + mem::size_of::<SymbolNumber>()) as u64;
        // let mut cursor = self.cursor.clone();
        let mut cursor = self.make_cursor();
        cursor.set_position(index);
        Some(cursor.read_f32::<LittleEndian>().unwrap())
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
