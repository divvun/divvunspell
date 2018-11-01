pub mod meta;

use memmap::{Mmap, MmapOptions};
use std::fs::File;
use zip::ZipArchive;
use std::io::prelude::*;
use std::io::{Cursor, Seek};
use std::slice;

use self::meta::SpellerMetadata;
use crate::transducer::Transducer;
use crate::speller::Speller;

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

fn slice_by_name<'a, R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    slice: &'a [u8],
    name: &str,
) -> &'a [u8] {
    let index = archive.by_name(name).unwrap();

    if index.compressed_size() != index.size() {
        // Unzip to a tmp dir and mmap into space
        panic!("This is a compressed archive, and is not supported.");
    }

    partial_slice(&slice, index.data_start() as usize, index.size() as usize)
}

#[derive(Debug)]
pub enum SpellerArchiveError {
    Io(::std::io::Error)
}

impl<'data> SpellerArchive<'data> {
    pub fn new(file_path: &str) -> Result<SpellerArchive, SpellerArchiveError> {
        let file = File::open(file_path)
            .map_err(|err| SpellerArchiveError::Io(err))?;

        let mmap = unsafe { MmapOptions::new().map(&file) }
            .map_err(|err| SpellerArchiveError::Io(err))?;

        let slice = unsafe { slice::from_raw_parts(mmap.as_ptr(), mmap.len()) };

        let reader = Cursor::new(&mmap);
        let mut archive = ZipArchive::new(reader).unwrap();

        let data = slice_by_name(&mut archive, &slice, "index.xml");
        let metadata = SpellerMetadata::from_bytes(&data).unwrap();

        // Load transducers
        let acceptor_data = slice_by_name(&mut archive, &slice, &metadata.acceptor.id);
        let errmodel_data = slice_by_name(&mut archive, &slice, &metadata.errmodel.id);

        let acceptor = Transducer::from_bytes(&acceptor_data);
        let errmodel = Transducer::from_bytes(&errmodel_data);

        let speller = Speller::new(errmodel, acceptor);

        Ok(SpellerArchive {
            handle: mmap,
            metadata: metadata,
            speller: speller,
        })
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
    let zhfst = SpellerArchive::new("./se-store.zhfst").unwrap();
    let two = zhfst.speller();
    let res = two.suggest("nuvviDspeller");
    println!("{:?}", res);
}
