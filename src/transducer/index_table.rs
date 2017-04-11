use byteorder::{LittleEndian, BigEndian, ReadBytesExt};
use std::io::Cursor;
use std::mem;

use types::{TransitionTableIndex, SymbolNumber};
use constants::TRANS_INDEX_SIZE;

#[derive(Debug)]
pub struct IndexTable<'a> {
    buf: &'a [u8],
    size: TransitionTableIndex
}

impl<'a> IndexTable<'a> {
    pub fn new(buf: &[u8], size: TransitionTableIndex) -> IndexTable {
        IndexTable {
            buf: buf,
            size: size
        }
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let mut rdr = Cursor::new(self.buf);
        let index: u64 = TRANS_INDEX_SIZE as u64 * i as u64;

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
        let index: u64 = (TRANS_INDEX_SIZE * (i as usize) + mem::size_of::<SymbolNumber>()) as u64;

        rdr.set_position(index);
        let trans = rdr.read_u32::<LittleEndian>().unwrap();

        if trans == TransitionTableIndex::max_value() {
            return None;
        }

        Some(trans)
    }

    /*
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let mut rdr = Cursor::new(self.buf);
        let index: u64 = (TRANS_INDEX_SIZE() * (i as usize) + mem::size_of::<SymbolNumber>()) as u64;

        rdr.set_position(index);
        let weight = rdr.read_Weight::<LittleEndian>().unwrap();

        Some(weight)
    }
    */

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
