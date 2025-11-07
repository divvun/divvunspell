//! Transition table for THFST transducers.
//!
//! The transition table stores FST arcs with input/output symbols, target states, and weights.

use std::mem;

use crate::transducer::TransducerError;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem, Memory};

/// Transition table for THFST transducers, generic over memory access pattern.
///
/// The transition table stores the arcs (transitions) of the finite-state transducer,
/// supporting both memory-mapped and file-based access patterns.
#[derive(Debug)]
pub struct TransitionTable<M: Memory> {
    memory: M,
    pub(crate) size: TransitionTableIndex,
}

const TRANS_TABLE_SIZE: usize = 12;

impl<M: Memory> TransitionTable<M> {
    /// Create a transition table from pre-existing memory.
    ///
    /// This is used when the memory has already been mapped or loaded.
    pub fn new(memory: M, byte_size: usize) -> Self {
        let size = TransitionTableIndex((byte_size / TRANS_TABLE_SIZE) as u32);
        TransitionTable { memory, size }
    }

    /// Get the size of the transition table in entries.
    #[inline(always)]
    pub fn len(&self) -> TransitionTableIndex {
        self.size
    }

    /// Check if the transition table is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.size.0 == 0
    }

    /// Read input symbol at given transition index.
    #[inline(always)]
    fn read_symbol_at(&self, offset: usize) -> Option<SymbolNumber> {
        let symbol = SymbolNumber(self.memory.read_u16_at(offset));
        if symbol == SymbolNumber::MAX {
            None
        } else {
            Some(symbol)
        }
    }

    /// Read input symbol at given transition index.
    #[inline(always)]
    pub fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }
        let offset = TRANS_TABLE_SIZE * i.0 as usize;
        self.read_symbol_at(offset)
    }

    /// Read output symbol at given transition index.
    #[inline(always)]
    pub fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }
        let offset = (TRANS_TABLE_SIZE * i.0 as usize) + mem::size_of::<SymbolNumber>();
        self.read_symbol_at(offset)
    }

    /// Read target state at given transition index.
    #[inline(always)]
    pub fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }
        let offset = (TRANS_TABLE_SIZE * i.0 as usize) + (2 * mem::size_of::<SymbolNumber>());
        let target = TransitionTableIndex(self.memory.read_u32_at(offset));

        if target == TransitionTableIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    /// Read weight at given transition index.
    #[inline(always)]
    pub fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }
        let offset = (TRANS_TABLE_SIZE * i.0 as usize)
            + (2 * mem::size_of::<SymbolNumber>())
            + mem::size_of::<TransitionTableIndex>();

        let weight = Weight(self.memory.read_f32_at(offset));
        Some(weight)
    }

    /// Check if state at given index is final.
    #[inline(always)]
    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.input_symbol(i).is_none()
            && self.output_symbol(i).is_none()
            && self.target(i) == Some(TransitionTableIndex(1))
    }

    /// Get a symbol transition record combining target, output, and weight.
    #[inline(always)]
    pub fn symbol_transition(
        &self,
        i: TransitionTableIndex,
    ) -> crate::transducer::symbol_transition::SymbolTransition {
        crate::transducer::symbol_transition::SymbolTransition::new(
            self.target(i),
            self.output_symbol(i),
            self.weight(i),
        )
    }
}

/// Helper functions to create memory-mapped transition tables from files.
impl TransitionTable<memmap2::Mmap> {
    pub fn from_path<P, FS, F>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
        F: vfs::File,
    {
        let file = fs.open_file(path).map_err(TransducerError::Io)?;
        let mmap = unsafe { file.memory_map().map_err(TransducerError::Memmap)? };
        let size = mmap.len();
        Ok(TransitionTable::new(mmap, size))
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
        Ok(TransitionTable::new(mmap, size))
    }
}

/// Helper function to create a file-based transition table (Unix only).
///
/// This uses syscalls for each access and is slower than mmap, but can be
/// useful when memory mapping is unavailable.
#[cfg(unix)]
impl<F: vfs::File> TransitionTable<F>
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
        Ok(TransitionTable::new(file, byte_size))
    }
}

// Implement the trait for compatibility with existing code
impl<F: vfs::File> crate::transducer::TransitionTable<F> for TransitionTable<memmap2::Mmap> {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        TransitionTable::from_path(fs, path)
    }

    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.input_symbol(i)
    }

    fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.output_symbol(i)
    }

    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        self.target(i)
    }

    fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        self.weight(i)
    }
}

#[cfg(unix)]
impl<F: vfs::File> crate::transducer::TransitionTable<F> for TransitionTable<F>
where
    F: Memory,
{
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        TransitionTable::from_path_file(fs, path)
    }

    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.input_symbol(i)
    }

    fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.output_symbol(i)
    }

    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        self.target(i)
    }

    fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        self.weight(i)
    }
}
