//! Handling of archives of spell-checking models.
use memmap2::Mmap;
use std::{ffi::OsString, path::Path, sync::Arc};

pub mod boxf;
pub mod error;
pub mod meta;
pub mod zip;

use self::{boxf::ThfstChunkedBoxSpellerArchive, meta::SpellerMetadata};
use crate::{
    archive::{error::SpellerArchiveError, zip::ZipSpellerArchive},
    speller::Speller,
};

pub(crate) struct TempMmap {
    mmap: Arc<Mmap>,

    // Not really dead, needed to drop when TempMmap drops
    _tempdir: tempfile::TempDir,
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

/// Speller archive is a file read into spell-checker with metadata.
pub trait SpellerArchive {
    /// Read and parse a speller archive.
    fn open(path: &Path) -> Result<Self, SpellerArchiveError>
    where
        Self: Sized;

    /// Retrieve spell-checker.
    ///
    /// The returned speller can perform both spell checking and morphological analysis
    /// depending on the `OutputMode` passed to `suggest()`.
    fn speller(&self) -> Arc<dyn Speller + Send + Sync>;

    /// Retrieve metadata.
    fn metadata(&self) -> Option<&SpellerMetadata>;
}

/// Reads a speller archive.
pub fn open<P>(path: P) -> Result<Arc<dyn SpellerArchive + Send + Sync>, SpellerArchiveError>
where
    P: AsRef<Path>,
{
    match path.as_ref().extension() {
        Some(x) if x == "bhfst" => {
            ThfstChunkedBoxSpellerArchive::open(path.as_ref()).map(|x| Arc::new(x) as _)
        }
        Some(x) if x == "zhfst" => ZipSpellerArchive::open(path.as_ref()).map(|x| Arc::new(x) as _),
        unknown => Err(SpellerArchiveError::UnsupportedExt(
            unknown
                .map(|x| x.to_owned())
                .unwrap_or_else(|| OsString::new()),
        )),
    }
}
