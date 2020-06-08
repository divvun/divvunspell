use std::ptr;

use memmap::Mmap;

use crate::transducer::TransducerError;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

#[derive(Debug)]
pub struct MemmapIndexTable<F> {
    buf: Mmap,
    pub(crate) size: u32,
    _file: std::marker::PhantomData<F>,
}

const INDEX_TABLE_SIZE: usize = 8;

impl<F: vfs::File> MemmapIndexTable<F> {
    pub fn from_path_partial<P, FS>(
        fs: &FS,
        path: P,
        chunk: u64,
        total: u64,
    ) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        let file = fs.open(path).map_err(TransducerError::Io)?;
        let len = file.len().map_err(TransducerError::Io)? / total;
        let buf = unsafe {
            file.partial_memory_map(chunk * len, len as usize)
                .map_err(TransducerError::Memmap)?
        };
        let size = (buf.len() / INDEX_TABLE_SIZE) as u32;
        Ok(MemmapIndexTable {
            buf,
            size,
            _file: std::marker::PhantomData::<F>,
        })
    }
}

impl<F: vfs::File> crate::transducer::IndexTable<F> for MemmapIndexTable<F> {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        let file = fs.open(path).map_err(TransducerError::Io)?;
        let buf = unsafe { file.memory_map().map_err(TransducerError::Memmap)? };
        let size = (buf.len() / INDEX_TABLE_SIZE) as u32;
        Ok(MemmapIndexTable {
            buf,
            size,
            _file: std::marker::PhantomData::<F>,
        })
    }

    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
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

    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
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

    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = (INDEX_TABLE_SIZE * i as usize) + 4;
        let weight: Weight = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        Some(weight)
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    use crate::transducer::IndexTable;
    use crate::transducer::TransducerError;
    use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
    use crate::vfs::{self, Filesystem};

    pub struct FileIndexTable<F: vfs::File> {
        file: F,
        size: u32,
    }

    impl<F: vfs::File> FileIndexTable<F> {
        #[inline(always)]
        fn read_u16_at(&self, index: u64) -> u16 {
            let mut buf = [0u8; 2];
            self.file
                .read_exact_at(&mut buf, index)
                .expect("failed to read u16");
            u16::from_le_bytes(buf)
        }

        #[inline(always)]
        fn read_u32_at(&self, index: u64) -> u32 {
            let mut buf = [0u8; 4];
            self.file
                .read_exact_at(&mut buf, index)
                .expect("failed to read u32");
            u32::from_le_bytes(buf)
        }
    }

    impl<F: vfs::File> IndexTable<F> for FileIndexTable<F> {
        fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
        where
            P: AsRef<std::path::Path>,
            FS: Filesystem<File = F>,
        {
            let file = fs.open(path).map_err(TransducerError::Io)?;
            Ok(FileIndexTable {
                size: file.len().map_err(TransducerError::Io)? as u32,
                file,
            })
        }

        fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
            if i >= self.size {
                return None;
            }

            let index = INDEX_TABLE_SIZE * i as usize;

            let input_symbol: SymbolNumber = self.read_u16_at(index as u64);

            if input_symbol == std::u16::MAX {
                None
            } else {
                Some(input_symbol)
            }
        }

        fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
            if i >= self.size {
                return None;
            }

            let index = (INDEX_TABLE_SIZE * i as usize) + 4;
            let target: TransitionTableIndex = self.read_u32_at(index as u64);

            if target == std::u32::MAX {
                None
            } else {
                Some(target)
            }
        }

        fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
            if i >= self.size {
                return None;
            }

            let index = (INDEX_TABLE_SIZE * i as usize) + 4;
            let x = self.read_u32_at(index as u64);
            let weight: Weight = f32::from_bits(x);

            Some(weight)
        }
    }
}

#[cfg(unix)]
pub use self::unix::FileIndexTable;
