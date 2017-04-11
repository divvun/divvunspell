use byteorder::{LittleEndian, BigEndian, ReadBytesExt};
use std::io::Cursor;
use std::mem;

use types::{TransitionTableIndex, SymbolNumber, Weight};
use constants::{TRANS_SIZE, TRANS_INDEX_SIZE};

#[derive(Debug)]
pub struct TransitionTable<'a> {
    buf: &'a [u8],
    size: TransitionTableIndex
}

impl<'a> TransitionTable<'a> {
    pub fn new(buf: &[u8], size: u32) -> TransitionTable {
        TransitionTable {
            buf: buf,
            size: size
        }
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let mut rdr = Cursor::new(self.buf);
        let index: u64 = TRANS_SIZE as u64 * i as u64;

        rdr.set_position(index);
        let symbol = rdr.read_u16::<LittleEndian>().unwrap();

        if symbol == SymbolNumber::max_value() {
            return None;
        }

        Some(symbol)
    }

    pub fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let mut rdr = Cursor::new(self.buf);
        let index: u64 = ((TRANS_SIZE * i as usize) + mem::size_of::<SymbolNumber>()) as u64;

        rdr.set_position(index);
        let symbol = rdr.read_u16::<LittleEndian>().unwrap();

        if symbol == SymbolNumber::max_value() {
            return None;
        }

        Some(symbol)
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let mut rdr = Cursor::new(self.buf);
        let index: u64 = ((TRANS_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>())) as u64;

        rdr.set_position(index);
        let symbol = rdr.read_u32::<LittleEndian>().unwrap();

        if symbol == TransitionTableIndex::max_value() {
            return None;
        }

        Some(symbol)
    }

    pub fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let mut rdr = Cursor::new(self.buf);
        let index: u64 = ((TRANS_SIZE * i as usize) +
            (2 * mem::size_of::<SymbolNumber>()) +
            mem::size_of::<TransitionTableIndex>()) as u64;

        rdr.set_position(index);
        let weight = rdr.read_f32::<LittleEndian>().unwrap();

        // TODO: NONE CASE

        Some(weight)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None &&
            self.output_symbol(i) == None &&
            self.target(i) == Some(1)
    }
}