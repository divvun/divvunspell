pub mod meta;

use memmap::{Mmap, MmapOptions};
use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::sync::Arc;
use ::zip::ZipArchive;

use self::meta::SpellerMetadata;
use crate::speller::Speller;
use crate::transducer::hfst::HfstTransducer;

mod zip;

pub use self::zip::ZipSpellerArchive;

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
