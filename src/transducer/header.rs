use byteorder::{LittleEndian, BigEndian, ReadBytesExt};
use std::io::Cursor;

use types::{SymbolNumber, TransitionTableIndex, HeaderFlag};

#[derive(Debug)]
pub struct TransducerHeader {
    symbols: SymbolNumber,
    input_symbols: SymbolNumber,
    trans_index_table: usize,
    trans_target_table: usize,
    states: TransitionTableIndex,
    transitions: TransitionTableIndex,

    properties: [bool; 9],
    alphabet_offset: usize
}

impl TransducerHeader {
    pub fn new(buf: &[u8]) -> TransducerHeader {
        let mut rdr = Cursor::new(buf);

        println!("Loading transducer");

        // Skip HFST string
        rdr.set_position(5);

        let header_len = rdr.read_u16::<LittleEndian>().unwrap() as u64;

        rdr.set_position(8);

        //println!("{:?}", header_len);
        let pos = rdr.position() + header_len;
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

        println!("{:?} {:?} {:?} {:?} {:?} {:?} {:?}", input_symbols, symbols, trans_index_table, trans_target_table, states, transitions, props);

        TransducerHeader {
            symbols: symbols,
            input_symbols: input_symbols,
            trans_index_table: trans_index_table,
            trans_target_table: trans_target_table,
            states: states,
            transitions: transitions,
            properties: props,

            alphabet_offset: rdr.position() as usize
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

    pub fn alphabet_offset(&self) -> usize {
        self.alphabet_offset
    }
}