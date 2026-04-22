use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{self, Cursor};
use std::path::Path;

use crate::transducer::TransducerError;
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
    /// Parse a HFST header from the start of the buffer.
    ///
    /// Returns a [`TransducerError::CorruptHeader`] (wrapping `path`) if the
    /// buffer is too short to contain a complete HFST header, or if any read
    /// falls off the end of the slice.
    pub fn parse(buf: &[u8], path: &Path) -> Result<TransducerHeader, TransducerError> {
        let mut rdr = Cursor::new(buf);

        // Skip "HFST\0" magic string
        rdr.set_position(5);

        let header_len = read_u16(&mut rdr, path)?;

        rdr.set_position(8);

        let pos = rdr.position() + u64::from(header_len);
        rdr.set_position(pos);

        let input_symbols = SymbolNumber(read_u16(&mut rdr, path)?);
        let symbols = SymbolNumber(read_u16(&mut rdr, path)?);
        let trans_index_table = TransitionTableIndex(read_u32(&mut rdr, path)?);
        let trans_target_table = TransitionTableIndex(read_u32(&mut rdr, path)?);
        let states = TransitionTableIndex(read_u32(&mut rdr, path)?);
        let transitions = TransitionTableIndex(read_u32(&mut rdr, path)?);

        let mut props = [false; 9];

        for prop in props.iter_mut() {
            let v = read_u32(&mut rdr, path)?;
            *prop = v != 0;
        }

        Ok(TransducerHeader {
            symbols,
            input_symbols,
            trans_index_table,
            trans_target_table,
            states,
            transitions,
            properties: props,
            header_size: rdr.position() as usize,
        })
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
        self.header_size
    }
}

#[inline]
fn read_u16(rdr: &mut Cursor<&[u8]>, path: &Path) -> Result<u16, TransducerError> {
    let offset = rdr.position() as usize;
    rdr.read_u16::<LittleEndian>()
        .map_err(|e| header_error(path, offset, e))
}

#[inline]
fn read_u32(rdr: &mut Cursor<&[u8]>, path: &Path) -> Result<u32, TransducerError> {
    let offset = rdr.position() as usize;
    rdr.read_u32::<LittleEndian>()
        .map_err(|e| header_error(path, offset, e))
}

#[inline]
fn header_error(path: &Path, offset: usize, _e: io::Error) -> TransducerError {
    TransducerError::CorruptHeader {
        path: path.to_path_buf(),
        offset,
    }
}
