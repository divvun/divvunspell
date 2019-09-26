use memmap::{Mmap, MmapOptions};
use std::io::{Read, Result};
use std::path::Path;

pub trait Filesystem {
    type File: File;

    fn open<P: AsRef<Path>>(&self, path: P) -> Result<Self::File>;
}

pub trait ToMemmap {
    unsafe fn map(&self) -> Result<Mmap>;
}

pub trait File: Read {}

impl File for std::fs::File {}
impl ToMemmap for std::fs::File {
    unsafe fn map(&self) -> Result<Mmap> {
        MmapOptions::new().map(self)
    }
}

pub struct Fs;

impl Filesystem for Fs {
    type File = std::fs::File;

    #[inline(always)]
    fn open<P: AsRef<Path>>(&self, path: P) -> Result<Self::File> {
        std::fs::File::open(path)
    }
}

pub(crate) mod boxf {
    use box_format::{BoxFileReader, BoxPath};
    use std::io::{Read, Result};
    use std::path::Path;

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

    impl super::ToMemmap for File {
        unsafe fn map(&self) -> Result<memmap::Mmap> {
            memmap::MmapOptions::new()
                .offset(self.offset)
                .len(self.len)
                .map(&self.file)
        }
    }

    impl<'a> super::File for File {}

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
            let record = self.0.metadata().records().iter().find_map(|r| {
                if r.path() == &boxpath {
                    r.as_file()
                } else {
                    None
                }
            });

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
