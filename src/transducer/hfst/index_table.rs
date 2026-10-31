use std::fmt;
use std::mem;

use crate::constants::INDEX_TABLE_SIZE;
use crate::transducer::dhfst::{u16_at, u32_at};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::Map;
use std::sync::Arc;

pub struct MappedIndexTable {
    pub(crate) size: TransitionTableIndex,
    pub(crate) mmap: Arc<Map>,
    pub(crate) offset: usize,
    pub(crate) len: usize,
}

impl fmt::Debug for MappedIndexTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Index table index: {}", self.size)?;
        Ok(())
    }
}

#[allow(clippy::len_without_is_empty)]
impl MappedIndexTable {
    pub fn new(
        buf: Arc<Map>,
        offset: usize,
        len: usize,
        size: TransitionTableIndex,
    ) -> MappedIndexTable {
        MappedIndexTable {
            size,
            mmap: buf,
            offset,
            len,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len - self.offset
    }

    /// Entry `i` as stored, its input symbol and its target, read with one
    /// bounds check, or `None` past the end of the table.
    #[inline(always)]
    pub(crate) fn entry(&self, i: TransitionTableIndex) -> Option<(u16, u32)> {
        if i >= self.size {
            return None;
        }
        let at = self.offset + INDEX_TABLE_SIZE * i.0 as usize;
        let bytes: &[u8; INDEX_TABLE_SIZE] = self.mmap.get(at..)?.first_chunk()?;
        Some((
            u16::from_le_bytes([bytes[0], bytes[1]]),
            u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]),
        ))
    }

    #[inline(always)]
    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + INDEX_TABLE_SIZE * i.0 as usize;

        let input_symbol = SymbolNumber(u16_at(&self.mmap, index));

        if input_symbol == SymbolNumber::MAX {
            None
        } else {
            Some(input_symbol)
        }
    }

    #[inline(always)]
    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + INDEX_TABLE_SIZE * i.0 as usize;
        let target =
            TransitionTableIndex(u32_at(&self.mmap, index + mem::size_of::<SymbolNumber>()));

        if target == TransitionTableIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    // Final weight reads from the same position as target, but for a different tuple
    // This can probably be abstracted out more nicely
    #[inline(always)]
    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = self.offset + INDEX_TABLE_SIZE * i.0 as usize;
        let weight = Weight(f32::from_bits(u32_at(
            &self.mmap,
            index + mem::size_of::<SymbolNumber>(),
        )));

        Some(weight)
    }

    #[inline(always)]
    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
