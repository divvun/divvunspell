use memmap::Mmap;
use std::sync::Arc;

mod boxf;
pub mod error;
pub mod meta;
mod zip;

pub use self::boxf::BoxSpellerArchive;
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
