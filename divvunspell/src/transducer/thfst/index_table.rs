use std::ptr;

use memmap::Mmap;

use crate::transducer::TransducerError;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::util::{self, Filesystem, ToMemmap};

#[doc(hidden)]
pub struct IndexTable {
    buf: Mmap,
    pub(crate) size: u32,
}

const INDEX_TABLE_SIZE: usize = 8;

impl IndexTable {
    pub fn from_path<P, FS, F>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
        F: util::File + ToMemmap,
    {
        let file = fs.open(path).map_err(TransducerError::Io)?;
        let buf = unsafe { file.memory_map().map_err(TransducerError::Memmap)? };
        let size = (buf.len() / INDEX_TABLE_SIZE) as u32;
        Ok(IndexTable { buf, size })
    }

    pub fn from_path_partial<P, FS, F>(
        fs: &FS,
        path: P,
        chunk: u64,
        total: u64,
    ) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
        F: util::File + ToMemmap,
    {
        let file = fs.open(path).map_err(TransducerError::Io)?;
        let len = file.len().map_err(TransducerError::Io)? / total;
        let buf = unsafe {
            file.partial_memory_map(chunk * len, len as usize)
                .map_err(TransducerError::Memmap)?
        };
        let size = (buf.len() / INDEX_TABLE_SIZE) as u32;
        Ok(IndexTable { buf, size })
    }

    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = INDEX_TABLE_SIZE * i as usize;

        let input_symbol: SymbolNumber =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        if input_symbol == std::u16::MAX {
            None
        } else {
            Some(input_symbol)
        }
    }

    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index = (INDEX_TABLE_SIZE * i as usize) + 4;
        let target: TransitionTableIndex =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        if target == std::u32::MAX {
            None
        } else {
            Some(target)
        }
    }

    // Final weight reads from the same position as target, but for a different tuple
    // This can probably be abstracted out more nicely
    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = (INDEX_TABLE_SIZE * i as usize) + 4;
        let weight: Weight = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        Some(weight)
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i) == None && self.target(i) != None
    }
}
