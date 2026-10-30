//! Files held in memory.
//!
//! [`Filesystem`] serves named files out of byte buffers the caller owns,
//! either put in one by one or read out of a box archive held in memory, so
//! every transducer and archive format loads without a host file system.

mod boxf;
#[cfg(test)]
mod format_tests;

#[cfg(test)]
pub(crate) use boxf::v0_archive;

use std::fmt;
use std::io::{self, Read, Result};
use std::ops::{Deref, Range};
use std::path::{Component, Path};
use std::sync::Arc;

use hashbrown::HashMap;

/// Bytes on the heap: a window onto a buffer that every clone shares.
#[derive(Clone)]
pub struct Bytes {
    data: Arc<Vec<u8>>,
    start: usize,
    len: usize,
}

impl Bytes {
    /// The window `range` of these bytes, or `None` if it runs past their
    /// end.
    pub fn slice(&self, range: Range<usize>) -> Option<Bytes> {
        if range.start > range.end || range.end > self.len {
            return None;
        }
        Some(Bytes {
            data: Arc::clone(&self.data),
            start: self.start + range.start,
            len: range.end - range.start,
        })
    }
}

impl Deref for Bytes {
    type Target = [u8];

    #[inline(always)]
    fn deref(&self) -> &[u8] {
        &self.data[self.start..self.start + self.len]
    }
}

impl AsRef<[u8]> for Bytes {
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(data: Vec<u8>) -> Bytes {
        let len = data.len();
        Bytes {
            data: Arc::new(data),
            start: 0,
            len,
        }
    }
}

impl From<&[u8]> for Bytes {
    fn from(data: &[u8]) -> Bytes {
        Bytes::from(data.to_vec())
    }
}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes({} bytes)", self.len)
    }
}

/// A file held in memory.
#[derive(Clone, Debug)]
pub struct File {
    bytes: Bytes,
    /// Where the `Read` impl reads next; the positioned reads ignore it.
    read_pos: usize,
}

impl File {
    /// A file holding `bytes`, read from its start.
    pub fn new(bytes: impl Into<Bytes>) -> File {
        File {
            bytes: bytes.into(),
            read_pos: 0,
        }
    }

    /// The bytes from `offset` to the end.
    #[cfg(any(unix, feature = "fs"))]
    fn at(&self, offset: u64) -> Result<&[u8]> {
        usize::try_from(offset)
            .ok()
            .and_then(|offset| self.bytes.get(offset..))
            .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let rest = self.bytes.get(self.read_pos..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.read_pos += n;
        Ok(n)
    }
}

impl super::File for File {
    fn len(&self) -> Result<u64> {
        Ok(self.bytes.len() as u64)
    }

    fn is_empty(&self) -> Result<bool> {
        Ok(self.bytes.is_empty())
    }

    #[cfg(unix)]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize> {
        let rest = self.at(offset)?;
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        Ok(n)
    }

    #[cfg(unix)]
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        let rest = self.at(offset)?;
        let wanted = rest
            .get(..buf.len())
            .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
        buf.copy_from_slice(wanted);
        Ok(())
    }

    fn contents(&self) -> Result<Bytes> {
        Ok(self.bytes.clone())
    }

    /// A copy of the file in an anonymous memory map, for the readers that
    /// read a memory map in place.
    #[cfg(feature = "fs")]
    unsafe fn memory_map(&self) -> Result<memmap2::Mmap> {
        super::copy_to_map(&self.bytes)
    }

    #[cfg(feature = "fs")]
    unsafe fn partial_memory_map(&self, offset: u64, len: usize) -> Result<memmap2::Mmap> {
        let rest = self.at(offset)?;
        super::copy_to_map(&rest[..len.min(rest.len())])
    }
}

/// Named files held in memory.
///
/// A path names a file by its normal components, so `a/b`, `./a/b` and
/// `a/c/../b` are one file. Directories are not stored: a file's path
/// implies them.
#[derive(Clone, Debug, Default)]
pub struct Filesystem {
    files: HashMap<String, Entry>,
}

#[derive(Clone, Debug)]
enum Entry {
    Stored(Bytes),
    /// A box archive member stored compressed, which cannot be read in place;
    /// the text names the compression.
    Compressed(String),
}

impl Filesystem {
    /// An empty file system.
    pub fn new() -> Filesystem {
        Filesystem::default()
    }

    /// Hold `bytes` as the file at `path`, replacing any file there.
    pub fn insert(&mut self, path: impl AsRef<Path>, bytes: impl Into<Bytes>) {
        self.files
            .insert(key(path.as_ref()), Entry::Stored(bytes.into()));
    }

    /// The files of the box archive `bytes`, each a window onto the archive.
    ///
    /// Reads both versions of the box format. A member stored compressed
    /// cannot be read in place and fails when it is opened.
    pub fn from_box(bytes: impl Into<Bytes>) -> Result<Filesystem> {
        let bytes = bytes.into();
        let mut files = HashMap::new();
        for member in boxf::members(&bytes)? {
            let entry = match member.compression {
                None => {
                    let window = member
                        .offset
                        .checked_add(member.len)
                        .and_then(|end| bytes.slice(member.offset..end))
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "box member '{}' runs past the end of the archive",
                                    member.path
                                ),
                            )
                        })?;
                    Entry::Stored(window)
                }
                Some(compression) => Entry::Compressed(compression),
            };
            files.insert(member.path, entry);
        }
        Ok(Filesystem { files })
    }
}

impl Entry {
    fn bytes(&self, name: &str) -> Result<&Bytes> {
        match self {
            Entry::Stored(bytes) => Ok(bytes),
            Entry::Compressed(compression) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "'{name}' is stored {compression}-compressed; a box archive read from memory must store its members uncompressed"
                ),
            )),
        }
    }
}

impl super::Filesystem for Filesystem {
    type File = File;

    fn open_file<P: AsRef<Path>>(&self, path: P) -> Result<File> {
        let name = key(path.as_ref());
        match self.files.get(&name) {
            Some(entry) => entry.bytes(&name).map(|bytes| File::new(bytes.clone())),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
        }
    }

    /// The file or directory at `path`, written out into a new temporary
    /// directory under its own name.
    #[cfg(feature = "fs")]
    fn copy_to_temp_dir<P: AsRef<Path>>(&self, path: P) -> Result<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        let wanted = key(path.as_ref());
        let base = wanted.rsplit_once('/').map_or("", |(parent, _)| parent);
        for (name, entry) in &self.files {
            let inside = name == &wanted
                || name
                    .strip_prefix(wanted.as_str())
                    .is_some_and(|rest| rest.starts_with('/'));
            if !inside {
                continue;
            }
            let bytes = entry.bytes(name)?;
            let relative = name
                .strip_prefix(base)
                .map_or(name.as_str(), |rest| rest.trim_start_matches('/'));
            let target = dir.path().join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, &bytes[..])?;
        }
        Ok(dir)
    }
}

/// `path` by its normal components, joined with `/`.
fn key(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::super::{File as _, Filesystem as _};
    use super::*;

    #[test]
    fn a_window_reads_its_own_bytes() {
        let bytes = Bytes::from(b"abcdef".to_vec());
        let window = bytes.slice(2..5).expect("in range");
        assert_eq!(&window[..], b"cde");
        assert_eq!(&window.slice(1..3).expect("in range")[..], b"de");
        assert!(window.slice(2..4).is_none());
    }

    #[test]
    fn paths_name_files_by_their_normal_components() {
        let mut fs = Filesystem::new();
        fs.insert("dir/file", b"x".to_vec());
        for path in ["dir/file", "./dir/file", "dir/other/../file", "/dir/file"] {
            assert!(fs.open_file(path).is_ok(), "{path}");
        }
        assert_eq!(
            fs.open_file("dir").map(|_| ()).map_err(|e| e.kind()),
            Err(io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn a_file_reads_from_the_start_and_hands_over_its_bytes() {
        let mut file = File::new(b"hello".to_vec());
        let mut text = String::new();
        file.read_to_string(&mut text).expect("reads");
        assert_eq!(text, "hello");
        assert_eq!(&file.contents().expect("contents")[..], b"hello");
        assert_eq!(file.len().expect("len"), 5);
    }
}
