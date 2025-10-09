use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

use crate::types::{HeaderFlag, SymbolNumber, TransitionTableIndex};

#[derive(Debug)]
pub struct TransducerHeader {
    symbols: SymbolNumber,
    input_symbols: SymbolNumber,
    trans_index_table: TransitionTableIndex,
    trans_target_table: TransitionTableIndex,
    states: TransitionTableIndex,
    transitions: TransitionTableIndex,

    properties: [bool; 9],
    header_size: usize,
}

#[allow(clippy::len_without_is_empty)]
impl TransducerHeader {
    pub fn new(buf: &[u8]) -> TransducerHeader {
        let mut rdr = Cursor::new(buf);

        // Skip HFST string
        rdr.set_position(5);

        let header_len = rdr.read_u16::<LittleEndian>().unwrap();

        rdr.set_position(8);

        let pos = rdr.position() + u64::from(header_len);
        rdr.set_position(pos);

        let input_symbols = SymbolNumber(rdr.read_u16::<LittleEndian>().unwrap());
        let symbols = SymbolNumber(rdr.read_u16::<LittleEndian>().unwrap());
        let trans_index_table = TransitionTableIndex(rdr.read_u32::<LittleEndian>().unwrap());
        let trans_target_table = TransitionTableIndex(rdr.read_u32::<LittleEndian>().unwrap());
        let states = TransitionTableIndex(rdr.read_u32::<LittleEndian>().unwrap());
        let transitions = TransitionTableIndex(rdr.read_u32::<LittleEndian>().unwrap());

        let mut props = [false; 9];

        for prop in props.iter_mut() {
            let v = rdr.read_u32::<LittleEndian>().unwrap();
            *prop = v != 0
        }

        TransducerHeader {
            symbols,
            input_symbols,
            trans_index_table,
            trans_target_table,
            states,
            transitions,
            properties: props,
            header_size: rdr.position() as usize,
        }
    }

    pub fn symbol_count(&self) -> SymbolNumber {
        self.symbols
    }

    pub fn input_symbol_count(&self) -> SymbolNumber {
        self.input_symbols
    }

    pub fn index_table_size(&self) -> TransitionTableIndex {
        self.trans_index_table
    }

    pub fn target_table_size(&self) -> TransitionTableIndex {
        self.trans_target_table
    }

    pub fn has_flag(&self, flag: HeaderFlag) -> bool {
        self.properties[flag as usize]
    }

    pub fn states(&self) -> TransitionTableIndex {
        self.states
    }

    pub fn transitions(&self) -> TransitionTableIndex {
        self.transitions
    }

    pub fn properties(&self) -> &[bool; 9] {
        &self.properties
    }

    pub fn len(&self) -> usize {
        self.header_size as usize
    }
}
