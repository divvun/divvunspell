use std::{mem, ptr};

use crate::transducer::TransducerError;
use crate::transducer::TransitionTable;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::util::{self, Filesystem, ToMemmap};
use memmap::Mmap;

#[doc(hidden)]
pub struct MemmapTransitionTable<F> {
    buf: Mmap,
    pub(crate) size: u32,
    _file: std::marker::PhantomData<F>,
}

const TRANS_TABLE_SIZE: usize = 12;

impl<F: util::File + ToMemmap> MemmapTransitionTable<F> {
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
        let file = fs.open(path).map_err(|e| TransducerError::Io(e))?;
        let len = file.len().map_err(TransducerError::Io)? / total;
        let buf = unsafe {
            file.partial_memory_map(chunk * len, len as usize)
                .map_err(TransducerError::Memmap)?
        };
        let size = (buf.len() / TRANS_TABLE_SIZE) as u32;
        Ok(MemmapTransitionTable {
            buf,
            size,
            _file: std::marker::PhantomData::<F>,
        })
    }

    #[inline]
    fn read_symbol_from_cursor(&self, index: usize) -> Option<SymbolNumber> {
        let x = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };
        if x == std::u16::MAX {
            None
        } else {
            Some(x)
        }
    }
}

impl<F: util::File + ToMemmap> TransitionTable<F> for MemmapTransitionTable<F> {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<std::path::Path>,
        FS: Filesystem<File = F>,
    {
        let file = fs.open(path).map_err(|e| TransducerError::Io(e))?;
        let buf = unsafe { file.memory_map() }.map_err(|e| TransducerError::Memmap(e))?;
        let size = (buf.len() / TRANS_TABLE_SIZE) as u32;
        Ok(MemmapTransitionTable {
            buf,
            size,
            _file: std::marker::PhantomData::<F>,
        })
    }

    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = TRANS_TABLE_SIZE as usize * i as usize;
        self.read_symbol_from_cursor(index)
    }

    fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        if i >= self.size {
            return None;
        }

        let index = ((TRANS_TABLE_SIZE * i as usize) + mem::size_of::<SymbolNumber>()) as usize;
        self.read_symbol_from_cursor(index)
    }

    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        if i >= self.size {
            return None;
        }

        let index = (TRANS_TABLE_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>());

        let x: TransitionTableIndex =
            unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };
        if x == std::u32::MAX {
            None
        } else {
            Some(x)
        }
    }

    fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= self.size {
            return None;
        }

        let index = (TRANS_TABLE_SIZE * i as usize)
            + (2 * mem::size_of::<SymbolNumber>())
            + mem::size_of::<TransitionTableIndex>();

        let x: Weight = unsafe { ptr::read(self.buf.as_ptr().add(index) as *const _) };

        Some(x)
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    use crate::transducer::TransducerError;
    use crate::transducer::TransitionTable;
    use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
    use crate::util::{self, Filesystem, ToMemmap};

    pub struct FileTransitionTable<F: util::File + util::ToMemmap> {
        file: F,
        size: u32,
    }

    impl<F: util::File + util::ToMemmap> FileTransitionTable<F> {
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

    impl<F: util::File + ToMemmap> TransitionTable<F> for FileTransitionTable<F> {
        fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
        where
            P: AsRef<std::path::Path>,
            FS: Filesystem<File = F>,
        {
            let file = fs.open(path).map_err(TransducerError::Io)?;
            Ok(FileTransitionTable {
                size: file.len().map_err(TransducerError::Io)? as u32,
                file,
            })
        }

        #[inline(always)]
        fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
            if i >= self.size {
                return None;
            }

            let index = TRANS_TABLE_SIZE as usize * i as usize;
            let x = self.read_u16_at(index as u64);
            if x == std::u16::MAX {
                None
            } else {
                Some(x)
            }
        }

        #[inline(always)]
        fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
            if i >= self.size {
                return None;
            }

            let index = ((TRANS_TABLE_SIZE * i as usize) + mem::size_of::<SymbolNumber>()) as usize;
            let x = self.read_u16_at(index as u64);
            if x == std::u16::MAX {
                None
            } else {
                Some(x)
            }
        }

        #[inline(always)]
        fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
            if i >= self.size {
                return None;
            }

            let index = (TRANS_TABLE_SIZE * i as usize) + (2 * mem::size_of::<SymbolNumber>());

            let x = self.read_u32_at(index as u64);
            if x == std::u32::MAX {
                None
            } else {
                Some(x)
            }
        }

        #[inline(always)]
        fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
            if i >= self.size {
                return None;
            }

            let index = (TRANS_TABLE_SIZE * i as usize)
                + (2 * mem::size_of::<SymbolNumber>())
                + mem::size_of::<TransitionTableIndex>();
            let x = self.read_u32_at(index as u64);
            let x = unsafe { std::mem::transmute::<u32, f32>(x) };
            Some(x)
        }
    }
}

#[cfg(unix)]
pub use self::unix::FileTransitionTable;
