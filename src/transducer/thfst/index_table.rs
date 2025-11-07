//! Index table for THFST transducers.
//!
//! The index table provides a sparse array structure for fast state lookup
//! in finite-state transducers.

use crate::transducer::TransducerError;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem, Memory};

/// Index table for THFST transducers, generic over memory access pattern.
///
/// The index table is a compact representation of FST states, supporting both
/// memory-mapped and file-based access patterns.
#[derive(Debug)]
pub struct IndexTable<M: Memory> {
    memory: M,
    pub(crate) size: TransitionTableIndex,
}

const INDEX_TABLE_SIZE: usize = 8;

impl<M: Memory> IndexTable<M> {
    /// Create an index table from pre-existing memory.
    ///
    /// This is used when the memory has already been mapped or loaded.
    pub fn new(memory: M, byte_size: usize) -> Self {
        let size = TransitionTableIndex((byte_size / INDEX_TABLE_SIZE) as u32);
        IndexTable { memory, size }
    }

    /// Get the size of the index table in entries.
    #[inline(always)]
    pub fn len(&self) -> TransitionTableIndex {
        self.size
    }

    /// Check if the index table is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.size.0 == 0
    }

    /// Read input symbol at given index.
    #[inline(always)]
    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let offset = INDEX_TABLE_SIZE * i.0 as usize;
        let input_symbol = SymbolNumber(self.memory.read_u16_at(offset));

        if input_symbol == SymbolNumber::MAX {
            None
        } else {
            Some(input_symbol)
        }
    }

    /// Read target state at given index.
    #[inline(always)]
    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let offset = (INDEX_TABLE_SIZE * i.0 as usize) + 4;
        let target = TransitionTableIndex(self.memory.read_u32_at(offset));

        if target == TransitionTableIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    /// Read final weight at given index.
    #[inline(always)]
    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let offset = (INDEX_TABLE_SIZE * i.0 as usize) + 4;
        let weight = Weight(self.memory.read_f32_at(offset));

        Some(weight)
    }

    /// Check if state at given index is final.
    #[inline(always)]
    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i).is_none() && self.target(i).is_some()
    }
}

/// Helper functions to create memory-mapped index tables from files.
impl IndexTable<memmap2::Mmap> {
    pub fn from_path<P, FS, F>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
        F: vfs::File,
    {
        let file = fs.open_file(path).map_err(TransducerError::Io)?;
        let mmap = unsafe { file.memory_map().map_err(TransducerError::Memmap)? };
        let size = mmap.len();
        Ok(IndexTable::new(mmap, size))
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
        F: vfs::File,
    {
        let file = fs.open_file(path).map_err(TransducerError::Io)?;
        let len = file.len().map_err(TransducerError::Io)? / total;
        let mmap = unsafe {
            file.partial_memory_map(chunk * len, len as usize)
                .map_err(TransducerError::Memmap)?
        };
        let size = mmap.len();
        Ok(IndexTable::new(mmap, size))
    }
}

/// Helper function to create a file-based index table (Unix only).
///
/// This uses syscalls for each access and is slower than mmap, but can be
/// useful when memory mapping is unavailable.
#[cfg(unix)]
impl<F: vfs::File> IndexTable<F>
where
    F: Memory,
{
    pub fn from_path_file<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        let file = fs.open_file(path).map_err(TransducerError::Io)?;
        let byte_size = file.len().map_err(TransducerError::Io)? as usize;
        Ok(IndexTable::new(file, byte_size))
    }
}

// Implement the trait for compatibility with existing code
impl<F: vfs::File> crate::transducer::IndexTable<F> for IndexTable<memmap2::Mmap> {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        IndexTable::from_path(fs, path)
    }

    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.input_symbol(i)
    }

    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        self.target(i)
    }

    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        self.final_weight(i)
    }
}

#[cfg(unix)]
impl<F: vfs::File> crate::transducer::IndexTable<F> for IndexTable<F>
where
    F: Memory,
{
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        IndexTable::from_path_file(fs, path)
    }

    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.input_symbol(i)
    }

    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        self.target(i)
    }

    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        self.final_weight(i)
    }
}
