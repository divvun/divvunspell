pub mod meta;

use memmap::{Mmap, MmapOptions};
use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::sync::Arc;
use zip::ZipArchive;

use self::meta::SpellerMetadata;
use crate::speller::Speller;
use crate::transducer::HfstTransducer;

pub struct SpellerArchive {
    metadata: SpellerMetadata,
    speller: Arc<Speller<HfstTransducer>>,
}

pub struct TempMmap {
    mmap: Arc<Mmap>,

    // Not really dead, needed to drop when TempMmap drops
    #[allow(dead_code)]
    tempdir: tempdir::TempDir,
}

pub enum MmapRef {
    Direct(Arc<Mmap>),
    Temp(TempMmap),
}

impl MmapRef {
    pub fn map(&self) -> Arc<Mmap> {
        match self {
            MmapRef::Direct(mmap) => Arc::clone(mmap),
            MmapRef::Temp(tmmap) => Arc::clone(&tmmap.mmap),
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
            Ok(v) => Ok(MmapRef::Temp(TempMmap {
                mmap: Arc::new(v),
                tempdir,
            })),
            Err(err) => panic!(err),
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
        Err(err) => panic!(err),
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
    Unknown(u8),
}

impl std::error::Error for SpellerArchiveError {}

impl std::fmt::Display for SpellerArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{:?}", self)
    }
}

impl SpellerArchive {
    pub fn new(file_path: &str) -> Result<SpellerArchive, SpellerArchiveError> {
        let file = File::open(file_path).map_err(SpellerArchiveError::OpenFileFailed)?;
        let reader = std::io::BufReader::new(&file);
        let mut archive = ZipArchive::new(reader).expect("zip");

        // Open file a second time to get around borrow checker
        let mut file = File::open(file_path).map_err(SpellerArchiveError::OpenFileFailed)?;

        let metadata_mmap = mmap_by_name(&mut file, &mut archive, "index.xml")
            .map_err(SpellerArchiveError::MetadataMmapFailed)?;
        let metadata = SpellerMetadata::from_bytes(&*metadata_mmap.map()).expect("meta");

        let acceptor_mmap = mmap_by_name(&mut file, &mut archive, &metadata.acceptor.id)
            .map_err(SpellerArchiveError::AcceptorMmapFailed)?;
        let errmodel_mmap = mmap_by_name(&mut file, &mut archive, &metadata.errmodel.id)
            .map_err(SpellerArchiveError::ErrmodelMmapFailed)?;
        drop(archive);

        let acceptor = HfstTransducer::from_mapped_memory(acceptor_mmap.map());
        let errmodel = HfstTransducer::from_mapped_memory(errmodel_mmap.map());

        let speller = Speller::new(errmodel, acceptor);

        Ok(SpellerArchive { metadata, speller })
    }

    pub fn speller(&self) -> Arc<Speller<HfstTransducer>> {
        self.speller.clone()
    }

    pub fn metadata(&self) -> &SpellerMetadata {
        &self.metadata
    }
}
