use memmap::{Mmap, MmapOptions};
use std::fmt::Debug;
use std::io::{Read, Result};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

pub trait Filesystem {
    type File: File;

    fn open<P: AsRef<Path>>(&self, path: P) -> Result<Self::File>;
}

pub trait File: Read + Debug {
    fn len(&self) -> Result<u64>;
    fn is_empty(&self) -> Result<bool>;
    #[cfg(unix)]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize>;
    #[cfg(unix)]
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()>;
    unsafe fn memory_map(&self) -> Result<Mmap>;
    unsafe fn partial_memory_map(&self, offset: u64, len: usize) -> Result<Mmap>;
}

impl File for std::fs::File {
    fn len(&self) -> Result<u64> {
        self.metadata().map(|m| m.len())
    }

    fn is_empty(&self) -> Result<bool> {
        self.len().map(|x| x == 0)
    }

    #[cfg(unix)]
    #[inline(always)]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize> {
        FileExt::read_at(self, buf, offset)
    }

    #[cfg(unix)]
    #[inline(always)]
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        FileExt::read_exact_at(self, buf, offset)
    }

    unsafe fn memory_map(&self) -> Result<Mmap> {
        MmapOptions::new().map(self)
    }

    unsafe fn partial_memory_map(&self, offset: u64, len: usize) -> Result<Mmap> {
        MmapOptions::new().offset(offset).len(len).map(self)
    }
}

pub struct Fs;

impl Filesystem for Fs {
    type File = std::fs::File;

    #[inline(always)]
    fn open<P: AsRef<Path>>(&self, path: P) -> Result<Self::File> {
        std::fs::File::open(&path)
    }
}

pub mod boxf {
    use box_format::{BoxFileReader, BoxPath};
    use std::io::{Read, Result};
    use std::path::Path;

    #[derive(Debug)]
    pub struct File {
        offset: u64,
        len: usize,
        file: std::fs::File,
        reader: std::io::Take<std::fs::File>,
    }

    impl Read for File {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            self.reader.read(buf)
        }
    }

    impl<'a> super::File for File {
        fn len(&self) -> Result<u64> {
            Ok(self.len as u64)
        }

        fn is_empty(&self) -> Result<bool> {
            Ok(self.len == 0)
        }

        #[cfg(unix)]
        #[inline(always)]
        fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize> {
            self.file.read_at(buf, self.offset + offset)
        }

        #[cfg(unix)]
        #[inline(always)]
        fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
            self.file.read_exact_at(buf, self.offset + offset)
        }

        unsafe fn memory_map(&self) -> Result<memmap::Mmap> {
            memmap::MmapOptions::new()
                .offset(self.offset)
                .len(self.len)
                .map(&self.file)
        }

        unsafe fn partial_memory_map(&self, offset: u64, len: usize) -> Result<memmap::Mmap> {
            memmap::MmapOptions::new()
                .offset(self.offset + offset)
                .len(std::cmp::min(self.len - offset as usize, len))
                .map(&self.file)
        }
    }

    pub struct Filesystem<'a>(&'a BoxFileReader);

    impl<'a> Filesystem<'a> {
        pub fn new(reader: &'a BoxFileReader) -> Filesystem<'a> {
            Filesystem(reader)
        }
    }

    impl<'a> super::Filesystem for Filesystem<'a> {
        type File = File;

        #[inline(always)]
        fn open<P: AsRef<Path>>(&self, path: P) -> Result<Self::File> {
            let boxpath = BoxPath::new(path).map_err(|e| e.as_io_error())?;
            let meta = self.0.metadata();
            let record = meta
                .inode(&boxpath)
                .and_then(|x| meta.record(x))
                .and_then(|r| r.as_file());

            let file = std::fs::File::open(self.0.path())?;

            match record {
                Some(v) => self.0.read_bytes(v).map(|reader| File {
                    offset: v.data.get(),
                    len: v.length as usize,
                    file,
                    reader,
                }),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                )),
            }
        }
    }
}
