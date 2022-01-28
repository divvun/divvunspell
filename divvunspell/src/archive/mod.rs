use memmap2::Mmap;
use std::{ffi::OsString, path::Path, sync::Arc};

pub mod boxf;
pub mod error;
pub mod meta;
pub mod zip;

use error::PredictorArchiveError;

pub use self::{boxf::BoxSpellerArchive, zip::ZipSpellerArchive};

use self::{
    boxf::ThfstChunkedBoxSpellerArchive,
    error::SpellerArchiveError,
    meta::{PredictorMetadata, SpellerMetadata},
};
use crate::{predictor::Predictor, speller::Speller};

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

pub trait SpellerArchive {
    fn open(path: &Path) -> Result<Self, SpellerArchiveError>
    where
        Self: Sized;

    fn speller(&self) -> Arc<dyn Speller + Send + Sync>;
    fn metadata(&self) -> Option<&SpellerMetadata>;
}

pub trait PredictorArchive {
    fn open(path: &Path, predictor_name: Option<&str>) -> Result<Self, PredictorArchiveError>
    where
        Self: Sized;

    fn predictor(&self) -> Arc<dyn Predictor + Send + Sync>;
    fn metadata(&self) -> Option<&PredictorMetadata>;
}

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

#[cfg(feature = "internal_ffi")]
pub(crate) mod ffi {
    use super::*;
    use cffi::{FromForeign, ToForeign};
    use std::error::Error;

    #[cffi::marshal(return_marshaler = "cffi::ArcMarshaler::<dyn SpellerArchive + Send + Sync>")]
    pub extern "C" fn divvun_speller_archive_open(
        #[marshal(cffi::PathBufMarshaler)] path: std::path::PathBuf,
    ) -> Result<Arc<dyn SpellerArchive + Send + Sync>, Box<dyn Error>> {
        open(&path).map_err(|e| Box::new(e) as _)
    }

    #[cffi::marshal(return_marshaler = "cffi::ArcMarshaler::<dyn Speller + Send + Sync>")]
    pub extern "C" fn divvun_speller_archive_speller(
        #[marshal(cffi::ArcRefMarshaler::<dyn SpellerArchive + Send + Sync>)] handle: Arc<
            dyn SpellerArchive + Send + Sync,
        >,
    ) -> Arc<dyn Speller + Send + Sync> {
        handle.speller()
    }

    #[cffi::marshal(return_marshaler = "cffi::StringMarshaler")]
    pub extern "C" fn divvun_speller_archive_locale(
        #[marshal(cffi::ArcRefMarshaler::<dyn SpellerArchive + Send + Sync>)] handle: Arc<
            dyn SpellerArchive + Send + Sync,
        >,
    ) -> Result<String, Box<dyn Error>> {
        match handle.metadata() {
            Some(v) => Ok(v.info.locale.to_string()),
            None => Err(Box::new(SpellerArchiveError::NoMetadata) as _),
        }
    }
}
