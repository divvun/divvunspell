//! Virtual file system abstractions.
//!
//! This module provides traits for abstracting file system operations and memory access,
//! allowing the codebase to work with different storage backends (real files, archives,
//! memory-mapped regions, bytes in memory) through a unified interface.
//!
//! `Fs` and `boxf` read files on the host and need the `fs` feature;
//! [`bytes`] holds files in memory and is always there, which is how a build
//! without a host file system, such as `wasm32-unknown-unknown`, reads them.

pub mod bytes;
pub mod memory;

pub use bytes::Bytes;
pub use memory::Memory;

#[cfg(feature = "fs")]
use fs_extra::dir::CopyOptions;
#[cfg(feature = "fs")]
use memmap2::{Mmap, MmapOptions};
use std::fmt::Debug;
use std::io::{Read, Result};
use std::path::Path;
#[cfg(feature = "fs")]
use tempfile::TempDir;

#[cfg(all(unix, feature = "fs"))]
use std::os::unix::fs::FileExt;

/// What the HFST optimized-lookup and DHFST readers read in place: a memory
/// map with the `fs` feature, bytes on the heap without it.
#[cfg(feature = "fs")]
pub(crate) type Map = Mmap;
/// What the HFST optimized-lookup and DHFST readers read in place: a memory
/// map with the `fs` feature, bytes on the heap without it.
#[cfg(not(feature = "fs"))]
pub(crate) type Map = Bytes;

/// A copy of `bytes` as a [`Map`]: an anonymous memory map with the `fs`
/// feature, bytes on the heap without it.
#[cfg(feature = "fs")]
pub(crate) fn copy_to_map(bytes: &[u8]) -> Result<Map> {
    let mut map = memmap2::MmapMut::map_anon(bytes.len())?;
    map.copy_from_slice(bytes);
    map.make_read_only()
}

/// A copy of `bytes` as a [`Map`]: an anonymous memory map with the `fs`
/// feature, bytes on the heap without it.
#[cfg(not(feature = "fs"))]
pub(crate) fn copy_to_map(bytes: &[u8]) -> Result<Map> {
    Ok(Bytes::from(bytes))
}

/// `bytes` as a [`Map`]: copied into an anonymous memory map with the `fs`
/// feature, and handed over as they are without it.
#[cfg(feature = "fs")]
pub(crate) fn into_map(bytes: Bytes) -> Result<Map> {
    copy_to_map(&bytes)
}

/// `bytes` as a [`Map`]: copied into an anonymous memory map with the `fs`
/// feature, and handed over as they are without it.
#[cfg(not(feature = "fs"))]
pub(crate) fn into_map(bytes: Bytes) -> Result<Map> {
    Ok(bytes)
}

pub trait Filesystem {
    type File: File;

    fn open_file<P: AsRef<Path>>(&self, path: P) -> Result<Self::File>;
    #[cfg(feature = "fs")]
    fn copy_to_temp_dir<P: AsRef<Path>>(&self, path: P) -> Result<TempDir>;
}

pub trait File: Read + Debug {
    fn len(&self) -> Result<u64>;
    fn is_empty(&self) -> Result<bool>;
    #[cfg(unix)]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize>;
    #[cfg(unix)]
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()>;
    /// The whole file as bytes on the heap: the bytes themselves for a file
    /// held in memory, a copy for any other.
    fn contents(&self) -> Result<Bytes>;
    #[cfg(feature = "fs")]
    unsafe fn memory_map(&self) -> Result<Mmap>;
    #[cfg(feature = "fs")]
    unsafe fn partial_memory_map(&self, offset: u64, len: usize) -> Result<Mmap>;
}

#[cfg(feature = "fs")]
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

    fn contents(&self) -> Result<Bytes> {
        use std::io::{Seek, SeekFrom};
        let mut file = self;
        let mut data = Vec::with_capacity(File::len(self)? as usize);
        file.seek(SeekFrom::Start(0))?;
        file.read_to_end(&mut data)?;
        Ok(Bytes::from(data))
    }

    unsafe fn memory_map(&self) -> Result<Mmap> {
        unsafe { MmapOptions::new().map(self) }
    }

    unsafe fn partial_memory_map(&self, offset: u64, len: usize) -> Result<Mmap> {
        unsafe { MmapOptions::new().offset(offset).len(len).map(self) }
    }
}

/// File system.
#[cfg(feature = "fs")]
pub struct Fs;

#[cfg(feature = "fs")]
impl Filesystem for Fs {
    type File = std::fs::File;

    #[inline(always)]
    fn open_file<P: AsRef<Path>>(&self, path: P) -> Result<Self::File> {
        std::fs::File::open(&path)
    }

    fn copy_to_temp_dir<P: AsRef<Path>>(&self, path: P) -> Result<TempDir> {
        let dir = tempfile::tempdir()?;
        fs_extra::copy_items(&[path.as_ref()], &dir, &CopyOptions::new())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(dir)
    }
}

/// Box file.
#[cfg(feature = "fs")]
pub mod boxf {
    use box_format::{BoxPath, sync::BoxReader as BoxFileReader};
    use std::io::{Read, Result};
    use std::path::Path;
    use tempfile::TempDir;

    #[derive(Debug)]
    pub struct File {
        offset: u64,
        len: usize,
        file: std::fs::File,
        segment: mmap_io::segment::Segment,
        /// Read cursor used by the `Read` impl. Not used by the
        /// position-indexed accessors (`read_at`, `memory_map`, etc.).
        read_pos: usize,
    }

    impl Read for File {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            let slice = self.segment.as_slice().map_err(std::io::Error::other)?;
            if self.read_pos >= slice.len() {
                return Ok(0);
            }
            let n = (slice.len() - self.read_pos).min(buf.len());
            buf[..n].copy_from_slice(&slice[self.read_pos..self.read_pos + n]);
            self.read_pos += n;
            Ok(n)
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

        fn contents(&self) -> Result<super::Bytes> {
            let slice = self.segment.as_slice().map_err(std::io::Error::other)?;
            Ok(super::Bytes::from(slice))
        }

        unsafe fn memory_map(&self) -> Result<memmap2::Mmap> {
            unsafe {
                memmap2::MmapOptions::new()
                    .offset(self.offset)
                    .len(self.len)
                    .map(&self.file)
            }
        }

        unsafe fn partial_memory_map(&self, offset: u64, len: usize) -> Result<memmap2::Mmap> {
            unsafe {
                memmap2::MmapOptions::new()
                    .offset(self.offset + offset)
                    .len(std::cmp::min(self.len - offset as usize, len))
                    .map(&self.file)
            }
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
        fn open_file<P: AsRef<Path>>(&self, path: P) -> Result<Self::File> {
            let boxpath = BoxPath::new(path).map_err(|e| e.as_io_error())?;
            let meta = self.0.metadata();
            let record = meta
                .index(&boxpath)
                .and_then(|x| meta.record(x))
                .and_then(|r| r.as_file());

            let file = std::fs::File::open(self.0.path())?;

            match record {
                Some(v) => self.0.memory_map(&v).map(|segment| File {
                    offset: v.data.get(),
                    len: v.length as usize,
                    file,
                    segment,
                    read_pos: 0,
                }),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                )),
            }
        }

        fn copy_to_temp_dir<P: AsRef<Path>>(&self, path: P) -> Result<TempDir> {
            let dir = tempfile::tempdir()?;
            let box_path = BoxPath::new(path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            self.0
                .extract_recursive(&box_path, &dir)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(dir)
        }
    }
}
