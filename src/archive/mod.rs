pub mod meta;

use memmap::{Mmap, MmapOptions};
use std::fs::File;
use zip::ZipArchive;
use std::io::prelude::*;
use std::io::{Cursor, Seek};
use std::sync::Arc;

use self::meta::SpellerMetadata;
use crate::transducer::{Transducer, HfstTransducer};
use crate::speller::Speller;

pub struct SpellerArchive {
    metadata: SpellerMetadata,
    speller: Arc<Speller<HfstTransducer>>,
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
    OpenFileFailed(std::io::Error),
    MmapFailed(std::io::Error),
    MetadataMmapFailed(std::io::Error),
    AcceptorMmapFailed(std::io::Error),
    ErrmodelMmapFailed(std::io::Error),
    UnsupportedCompressed,
    Unknown(u8)
}

impl SpellerArchiveError {
    // pub fn from(code: u8) -> SpellerArchiveError {
    //     match code {
    //         1 => SpellerArchiveError::OpenFileFailed,
    //         2 => SpellerArchiveError::MmapFailed,
    //         3 => SpellerArchiveError::MetadataMmapFailed,
    //         4 => SpellerArchiveError::AcceptorMmapFailed,
    //         5 => SpellerArchiveError::ErrmodelMmapFailed,
    //         6 => SpellerArchiveError::UnsupportedCompressed,
    //         _ => SpellerArchiveError::Unknown(code)
    //     }
    // }

    // pub fn to_u8(&self) -> u8 {
    //     match self {
    //         SpellerArchiveError::OpenFileFailed => 1,
    //         SpellerArchiveError::MmapFailed => 2,
    //         SpellerArchiveError::MetadataMmapFailed => 3,
    //         SpellerArchiveError::AcceptorMmapFailed => 4,
    //         SpellerArchiveError::ErrmodelMmapFailed => 5,
    //         SpellerArchiveError::UnsupportedCompressed => 6,
    //         _ => std::u8::MAX
    //     }
    // }

    // pub fn to_string(&self) -> String {
    //     match self {
    //         SpellerArchiveError::OpenFileFailed => "Open file failed.".into(),
    //         SpellerArchiveError::MmapFailed => "Mmap failed.".into(),
    //         SpellerArchiveError::MetadataMmapFailed => "Metadata mmap failed.".into(),
    //         SpellerArchiveError::AcceptorMmapFailed => "Acceptor mmap failed.".into(),
    //         SpellerArchiveError::ErrmodelMmapFailed => "Errmodel mmap failed.".into(),
    //         SpellerArchiveError::UnsupportedCompressed => "The provided file is compressed and cannot be memory mapped. Rezip with no compression.".into(),
    //         _ => format!("Unknown error code {}.", self.to_u8())
    //     }
    // }
}

impl SpellerArchive {
    pub fn new(file_path: &str) -> Result<SpellerArchive, SpellerArchiveError> {
        let file = File::open(file_path)
            .map_err(|e| SpellerArchiveError::OpenFileFailed(e))?;

        let reader = std::io::BufReader::new(&file);
        let mut archive = ZipArchive::new(reader).unwrap();

        let data = slice_by_name(&mut archive, "index.xml")?;
        let metadata_mmap = unsafe {
            MmapOptions::new()
                .offset(data.0)
                .len(data.1)
                .map(&file)
        }.map_err(|e| SpellerArchiveError::MetadataMmapFailed(e))?; 

        let metadata = SpellerMetadata::from_bytes(&metadata_mmap).unwrap();
        let acceptor_range = slice_by_name(&mut archive, &metadata.acceptor.id)?;
        let errmodel_range = slice_by_name(&mut archive, &metadata.errmodel.id)?;
        drop(archive);

        // eprintln!("Acceptor range: {:?}", acceptor_range);

        // Load transducers
        let acceptor_mmap = unsafe {
            MmapOptions::new()
                .offset(acceptor_range.0)
                .len(acceptor_range.1)
                .map(&file)
        }.map_err(|e| SpellerArchiveError::AcceptorMmapFailed(e))?; 
        let acceptor = HfstTransducer::from_mapped_memory(acceptor_mmap);

        let errmodel_mmap = unsafe {
            MmapOptions::new()
                .offset(errmodel_range.0)
                .len(errmodel_range.1)
                .map(&file)
        }.map_err(|e| SpellerArchiveError::ErrmodelMmapFailed(e))?; 
        let errmodel = HfstTransducer::from_mapped_memory(errmodel_mmap);

        let speller = Speller::new(errmodel, acceptor);

        Ok(SpellerArchive {
            metadata: metadata,
            speller: speller,
        })
    }

    pub fn speller(&self) -> Arc<Speller<HfstTransducer>> {
        self.speller.clone()
    }

    pub fn metadata(&self) -> &SpellerMetadata {
        &self.metadata
    }
}

#[test]
fn test_load_zhfst() {
    // let zhfst = SpellerArchive::new("./se-store.zhfst").unwrap();
    // let two = zhfst.speller();
    // let res = two.suggest("nuvviDspeller");
    // println!("{:?}", res);
}
