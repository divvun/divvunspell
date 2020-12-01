use memmap::Mmap;
use std::{ffi::OsString, path::Path, sync::Arc};

pub mod boxf;
pub mod error;
pub mod meta;
pub mod zip;

pub use self::boxf::BoxSpellerArchive;
use self::{error::SpellerArchiveError, boxf::ThfstBoxSpellerArchive};
use self::meta::SpellerMetadata;
pub use self::zip::ZipSpellerArchive;
use crate::speller::Speller;

pub(crate) struct TempMmap {
    mmap: Arc<Mmap>,

    // Not really dead, needed to drop when TempMmap drops
    _tempdir: tempdir::TempDir,
}

pub(crate) enum MmapRef {
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

pub trait SpellerArchive {
    fn open(path: &Path) -> Result<Self, SpellerArchiveError>
    where
        Self: Sized;

    fn speller(&self) -> Arc<dyn Speller + Send + Sync>;
    fn metadata(&self) -> Option<&SpellerMetadata>;
}

pub fn open<P>(path: P) -> Result<Arc<dyn SpellerArchive + Send + Sync>, SpellerArchiveError>
where
    P: AsRef<Path>,
{
    match path.as_ref().extension() {
        Some(x) if x == "bhfst" => {
            ThfstBoxSpellerArchive::open(path.as_ref())
                .map(|x| Arc::new(x) as _)
        }
        Some(x) if x == "zhfst" => {
            ZipSpellerArchive::open(path.as_ref())
                .map(|x| Arc::new(x) as _)
        }
        unknown => Err(SpellerArchiveError::UnsupportedExt(
            unknown
                .map(|x| x.to_owned())
                .unwrap_or_else(|| OsString::new()),
        )),
    }
}
