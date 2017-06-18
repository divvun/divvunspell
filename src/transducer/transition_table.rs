use byteorder::{LittleEndian, BigEndian, ReadBytesExt};
use std::io::Cursor;
use std::{mem, u16, u32};
use std::cell::{RefCell, RefMut};

use types::{TransitionTableIndex, SymbolNumber, Weight};
use constants::{TRANS_SIZE, TRANS_INDEX_SIZE};

use transducer::symbol_transition::SymbolTransition;

#[derive(Debug, Clone)]
pub struct TransitionTable<'a> {
    size: TransitionTableIndex,
    cursor: RefCell<Cursor<&'a [u8]>>
}

impl<'a> TransitionTable<'a> {
    pub fn new(buf: &[u8], size: u32) -> TransitionTable {
        TransitionTable {
            size: size,
            cursor: RefCell::new(Cursor::new(buf))
        }
    }

    fn read_symbol_from_cursor(&self, index: u64) -> Option<SymbolNumber> {
        let mut cursor = self.cursor.borrow_mut();
        cursor.set_position(index);
        let x = cursor.read_u16::<LittleEndian>().unwrap();
        if x == u16::MAX { None } else { Some(x) }
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index: u64 = TRANS_SIZE as u64 * i as u64;
        self.read_symbol_from_cursor(index)
    }

    pub fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index: u64 = ((TRANS_SIZE * i as usize) + mem::size_of::<SymbolNumber>()) as u64;
        self.read_symbol_from_cursor(index)
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index: u64 = ((TRANS_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>())) as u64;
        let mut cursor = self.cursor.borrow_mut();
        cursor.set_position(index);
        let x = cursor.read_u32::<LittleEndian>().unwrap();
        if x == u32::MAX { None } else { Some(x) }
    }

    pub fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index: u64 = ((TRANS_SIZE * i as usize) +
            (2 * mem::size_of::<SymbolNumber>()) +
            mem::size_of::<TransitionTableIndex>()) as u64;

        let mut cursor = self.cursor.borrow_mut();
        cursor.set_position(index);
        Some(cursor.read_f32::<LittleEndian>().unwrap())
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None &&
            self.output_symbol(i) == None &&
            self.target(i) == Some(1)
    }

    pub fn symbol_transition(&self, i: TransitionTableIndex) -> SymbolTransition {
        SymbolTransition::new(self.target(i), self.output_symbol(i), self.weight(i))
    }
}