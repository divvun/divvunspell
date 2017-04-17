pub mod meta;

use memmap::{Mmap, Protection};
use zip::ZipArchive;
use std::io::prelude::*;
use std::io::{Cursor, Seek};
use std::slice;

use self::meta::SpellerMetadata;
use transducer::Transducer;
use speller::Speller;

pub struct SpellerArchive<'a> {
    #[allow(dead_code)]
    handle: Mmap,
    metadata: SpellerMetadata,
    speller: Speller<'a>
}

#[inline]
fn partial_slice(slice: &[u8], start: usize, offset: usize) -> &[u8] {
    let end = start + offset;
    &slice[start..end]
}

fn slice_by_name<'a, R: Read + Seek>(archive: &mut ZipArchive<R>, slice: &'a [u8], name: &str) -> &'a [u8] {
    let index = archive.by_name(name).unwrap();

    if index.compressed_size() != index.size() {
        // Unzip to a tmp dir and mmap into space
        panic!("This is a compressed archive, and is not supported.");
    }

    partial_slice(&slice, index.data_start() as usize, index.size() as usize)
}

impl<'a> SpellerArchive<'a> {
    pub fn new(file_path: &str) -> SpellerArchive<'a> {
        let mmap = Mmap::open_path(file_path, Protection::Read).unwrap();
        let slice = unsafe { slice::from_raw_parts(mmap.ptr(), mmap.len()) };

        let reader = Cursor::new(&slice);
        let mut archive = ZipArchive::new(reader).unwrap();

        let data = slice_by_name(&mut archive, &slice, "index.xml");
        let metadata = SpellerMetadata::from_bytes(&data).unwrap();

        // Load transducers
        let acceptor_data = slice_by_name(&mut archive, &slice, &metadata.acceptor.id);
        let errmodel_data = slice_by_name(&mut archive, &slice, &metadata.errmodel.id);
        
        let acceptor = Transducer::from_bytes(&acceptor_data);
        let errmodel = Transducer::from_bytes(&errmodel_data);

        let speller = Speller::new(acceptor, errmodel);

        SpellerArchive {
            handle: mmap,
            metadata: metadata,
            speller: speller
        }
    }

    pub fn speller(&self) -> &Speller {
        return &self.speller
    }

    pub fn metadata(&self) -> &SpellerMetadata {
        return &self.metadata
    }
}

#[test]
fn test_whatever() {
    let a = SpellerArchive::new("./sma-store.zip");
    println!("{:?}", a.metadata());
    println!("{:?}", a.speller().lexicon().alphabet());
}