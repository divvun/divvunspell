pub mod meta;

use memmap::{Mmap, MmapOptions};
use std::fs::File;
use zip::ZipArchive;
use std::io::prelude::*;
use std::io::Seek;
use std::sync::Arc;

use self::meta::SpellerMetadata;
use crate::transducer::HfstTransducer;
use crate::speller::Speller;

pub struct SpellerArchive {
    metadata: SpellerMetadata,
    speller: Arc<Speller<HfstTransducer>>,
}

pub struct TempMmap {
    mmap: Arc<Mmap>,

    // Not really dead, needed to drop when TempMmap drops
    #[allow(dead_code)] 
    tempdir: tempdir::TempDir
}

pub enum MmapRef {
    Direct(Arc<Mmap>),
    Temp(TempMmap)
}

impl MmapRef {
    pub fn map(&self) -> Arc<Mmap> {
        match self {
            MmapRef::Direct(mmap) => Arc::clone(mmap),
            MmapRef::Temp(tmmap) => Arc::clone(&tmmap.mmap)
        }
    }
}

fn mmap_by_name<'a, R: Read + Seek>(
    zipfile: &mut File,
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<MmapRef, std::io::Error> {
    let mut index = archive.by_name(name).unwrap();

    if index.compression() != zip::CompressionMethod::Stored {
        let tempdir = tempdir::TempDir::new("divvunspell")?;
        let outpath = tempdir.path().join(index.sanitized_name());

        let mut outfile = File::create(&outpath)?;
        std::io::copy(&mut index, &mut outfile)?;

        let outfile = File::open(&outpath)?;

        let mmap = unsafe { MmapOptions::new().map(&outfile) };

        return match mmap {
            Ok(v) => Ok(MmapRef::Temp(TempMmap { mmap: Arc::new(v), tempdir })),
            Err(err) => panic!(err)
        };
    }

    let mmap = unsafe {
        MmapOptions::new()
            .offset(index.data_start())
            .len(index.size() as usize)
            .map(&zipfile)
    };

    match mmap {
        Ok(v) => Ok(MmapRef::Direct(Arc::new(v))),
        Err(err) => panic!(err)
    }    
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
        let mut archive = ZipArchive::new(reader).expect("zip");

        // Open file a second time to get around borrow checker
        let mut file = File::open(file_path)
            .map_err(|e| SpellerArchiveError::OpenFileFailed(e))?;

        let metadata_mmap = mmap_by_name(&mut file, &mut archive, "index.xml")
            .map_err(|e| SpellerArchiveError::MetadataMmapFailed(e))?;
        let metadata = SpellerMetadata::from_bytes(&*metadata_mmap.map()).expect("meta");

        let acceptor_mmap = mmap_by_name(&mut file, &mut archive, &metadata.acceptor.id)
            .map_err(|e| SpellerArchiveError::AcceptorMmapFailed(e))?;
        let errmodel_mmap = mmap_by_name(&mut file, &mut archive, &metadata.errmodel.id)
            .map_err(|e| SpellerArchiveError::ErrmodelMmapFailed(e))?;
        drop(archive);

        let acceptor = HfstTransducer::from_mapped_memory(acceptor_mmap.map());
        let errmodel = HfstTransducer::from_mapped_memory(errmodel_mmap.map());

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
