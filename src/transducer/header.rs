use byteorder::{LittleEndian, BigEndian, ReadBytesExt};
use std::io::Cursor;

use types::{SymbolNumber, TransitionTableIndex};

#[derive(Debug)]
pub struct TransducerHeader {
    pub symbols: SymbolNumber,
    pub input_symbols: SymbolNumber,
    pub trans_index_table: usize,
    pub trans_target_table: usize,
    pub states: TransitionTableIndex,
    pub transitions: TransitionTableIndex,

    properties: [bool; 9],
    pub alphabet_offset: usize
}

impl TransducerHeader {
    pub fn new(buf: &[u8]) -> TransducerHeader {
        let mut rdr = Cursor::new(buf);

        // Skip HFST string
        rdr.set_position(5);

        let header_len = rdr.read_u16::<LittleEndian>().unwrap() as u64;

        rdr.set_position(8);

        println!("{:?}", header_len);
        let pos = rdr.position() + header_len;
        rdr.set_position(pos);

        let input_symbols = rdr.read_u16::<LittleEndian>().unwrap();
        let symbols = rdr.read_u16::<LittleEndian>().unwrap() + 1;
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
}