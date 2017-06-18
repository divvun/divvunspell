use byteorder::{LittleEndian, BigEndian, ReadBytesExt};
use std::io::Cursor;
use std::{mem, u16, u32};

use types::{TransitionTableIndex, SymbolNumber, Weight};
use constants::TRANS_INDEX_SIZE;
use std::cell::{RefCell, RefMut};

#[derive(Debug)]
pub struct IndexTable<'a> {
    size: TransitionTableIndex,
    cursor: RefCell<Cursor<&'a [u8]>>
}

impl<'a> IndexTable<'a> {
    pub fn new(buf: &[u8], size: TransitionTableIndex) -> IndexTable {
        IndexTable {
            size: size,
            cursor: RefCell::new(Cursor::new(buf))
        }
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let mut cursor = self.cursor.borrow_mut();
        cursor.set_position(TRANS_INDEX_SIZE as u64 * i as u64);
        let x = cursor.read_u16::<LittleEndian>().unwrap();
        if x == u16::MAX { None } else { Some(x) }
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index: u64 = (TRANS_INDEX_SIZE * (i as usize) + mem::size_of::<SymbolNumber>()) as u64;
        let mut cursor = self.cursor.borrow_mut();
        cursor.set_position(index);
        let x = cursor.read_u32::<LittleEndian>().unwrap();
        if x == u32::MAX { None } else { Some(x) }
    }

    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        // BUG: see below
        // TODO: this is the same as target, and therefore probably a bug
        
        let index: u64 = (TRANS_INDEX_SIZE * (i as usize) + mem::size_of::<SymbolNumber>()) as u64;
        let mut cursor = self.cursor.borrow_mut();
        cursor.set_position(index);
        Some(cursor.read_f32::<LittleEndian>().unwrap())
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
