use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

use crate::types::{HeaderFlag, SymbolNumber, TransitionTableIndex};

#[derive(Debug)]
pub struct TransducerHeader {
    symbols: SymbolNumber,
    input_symbols: SymbolNumber,
    trans_index_table: usize,
    trans_target_table: usize,
    states: TransitionTableIndex,
    transitions: TransitionTableIndex,

    properties: [bool; 9],
    string_content_size: u16,
    header_size: usize,
}

impl TransducerHeader {
    pub fn new(buf: &[u8]) -> TransducerHeader {
        let mut rdr = Cursor::new(buf);

        // Skip HFST string
        rdr.set_position(5);

        let header_len = rdr.read_u16::<LittleEndian>().unwrap();

        rdr.set_position(8);

        let pos = rdr.position() + header_len as u64;
        rdr.set_position(pos);

        let input_symbols = rdr.read_u16::<LittleEndian>().unwrap();
        let symbols = rdr.read_u16::<LittleEndian>().unwrap();
        let trans_index_table = rdr.read_u32::<LittleEndian>().unwrap() as usize;
        let trans_target_table = rdr.read_u32::<LittleEndian>().unwrap() as usize;
        let states = rdr.read_u32::<LittleEndian>().unwrap();
        let transitions = rdr.read_u32::<LittleEndian>().unwrap();

        let mut props = [false; 9];

        for i in 0..props.len() {
            let v = rdr.read_u32::<LittleEndian>().unwrap();
            props[i] = v != 0
        }

        TransducerHeader {
            symbols: symbols,
            input_symbols: input_symbols,
            trans_index_table: trans_index_table,
            trans_target_table: trans_target_table,
            states: states,
            transitions: transitions,
            properties: props,

            string_content_size: header_len,
            header_size: rdr.position() as usize,
        }
    }

    pub fn symbol_count(&self) -> SymbolNumber {
        self.symbols
    }

    pub fn input_symbol_count(&self) -> SymbolNumber {
        self.input_symbols
    }

    pub fn index_table_size(&self) -> usize {
        self.trans_index_table
    }

    pub fn target_table_size(&self) -> usize {
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
