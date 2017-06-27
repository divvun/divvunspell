pub mod meta;

use memmap::{Mmap, Protection};
use zip::ZipArchive;
use std::io::prelude::*;
use std::io::{Cursor, Seek};
use std::slice;

use self::meta::SpellerMetadata;
use transducer::Transducer;
use speller::Speller;

pub struct SpellerArchive<'data> {
    #[allow(dead_code)]
    handle: Mmap,
    metadata: SpellerMetadata,
    speller: Speller<'data>,
}

#[inline]
fn partial_slice(slice: &[u8], start: usize, offset: usize) -> &[u8] {
    let end = start + offset;
    &slice[start..end]
}

fn slice_by_name<'data, R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    slice: &'data [u8],
    name: &str,
) -> &'data [u8] {
    let index = archive.by_name(name).unwrap();

    if index.compressed_size() != index.size() {
        // Unzip to a tmp dir and mmap into space
        panic!("This is a compressed archive, and is not supported.");
    }

    partial_slice(&slice, index.data_start() as usize, index.size() as usize)
}

impl<'data> SpellerArchive<'data> {
    pub fn new(file_path: &str) -> SpellerArchive {
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

        let speller = Speller::new(errmodel, acceptor);

        SpellerArchive {
            handle: mmap,
            metadata: metadata,
            speller: speller,
        }
    }

    pub fn speller<'a>(&'a self) -> &'a Speller<'data>
    where
        'data: 'a,
    {
        &self.speller
    }

    pub fn metadata(&self) -> &SpellerMetadata {
        &self.metadata
    }
}

#[test]
fn test_load_zhfst() {
    let zhfst = SpellerArchive::new("./se-store.zhfst");
    let two = zhfst.speller();
    let res = two.suggest_one("nuvviDspeller");
    println!("{:?}", res);
}
