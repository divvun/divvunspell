pub mod meta;

use memmap::{Mmap, MmapOptions};
use std::fs::File;
use zip::ZipArchive;
use std::io::prelude::*;
use std::io::{Cursor, Seek};
use std::sync::Arc;

use self::meta::SpellerMetadata;
use crate::transducer::Transducer;
use crate::speller::Speller;

pub struct SpellerArchive {
    #[allow(dead_code)]
    handle: Mmap,
    metadata: SpellerMetadata,
    speller: Arc<Speller>,
}

fn slice_by_name<'a, R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<(u64, usize), SpellerArchiveError> {
    let index = archive.by_name(name).unwrap();

    if index.compressed_size() != index.size() {
        // Unzip to a tmp dir and mmap into space
        return Err(SpellerArchiveError::UnsupportedCompressed);
    }

    Ok((index.data_start(), index.size() as usize))

    // Ok(partial_slice(&slice,))
}

#[derive(Debug)]
pub enum SpellerArchiveError {
    Io(::std::io::Error),
    UnsupportedCompressed
}

impl SpellerArchive {
    pub fn new(file_path: &str) -> Result<SpellerArchive, SpellerArchiveError> {
        let file = File::open(file_path)
            .map_err(|err| SpellerArchiveError::Io(err))?;

        let mmap = unsafe { MmapOptions::new().map(&file) }
            .map_err(|err| SpellerArchiveError::Io(err))?;

        // let slice = unsafe { slice::from_raw_parts(mmap.as_ptr(), mmap.len()) };

        let reader = Cursor::new(&mmap);
        let mut archive = ZipArchive::new(reader).unwrap();

        let data = slice_by_name(&mut archive, "index.xml")?;
        let metadata_mmap = unsafe {
            MmapOptions::new()
                .offset(data.0)
                .len(data.1)
                .map(&file)
        }.map_err(|err| SpellerArchiveError::Io(err))?; 
        let metadata = SpellerMetadata::from_bytes(&metadata_mmap).unwrap();

        // Load transducers
        let acceptor_range = slice_by_name(&mut archive, &metadata.acceptor.id)?;
        let acceptor_mmap = unsafe {
            MmapOptions::new()
                .offset(acceptor_range.0)
                .len(acceptor_range.1)
                .map(&file)
        }.map_err(|err| SpellerArchiveError::Io(err))?; 
        let acceptor = Transducer::from_mapped_memory(acceptor_mmap);

        let errmodel_range = slice_by_name(&mut archive, &metadata.errmodel.id)?;
        let errmodel_mmap = unsafe {
            MmapOptions::new()
                .offset(errmodel_range.0)
                .len(errmodel_range.1)
                .map(&file)
        }.map_err(|err| SpellerArchiveError::Io(err))?; 
        let errmodel = Transducer::from_mapped_memory(errmodel_mmap);

        let speller = Speller::new(errmodel, acceptor);

        Ok(SpellerArchive {
            handle: mmap,
            metadata: metadata,
            speller: speller,
        })
    }

    pub fn speller(&self) -> Arc<Speller> {
        self.speller.clone()
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
