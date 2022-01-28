use std::sync::Arc;

use box_format::BoxFileReader;

#[cfg(feature = "gpt2")]
use tempfile::TempDir;

#[cfg(feature = "gpt2")]
use super::{error::PredictorArchiveError, meta::PredictorMetadata, PredictorArchive};

use super::error::SpellerArchiveError;
use super::{meta::SpellerMetadata, SpellerArchive};
use crate::speller::{HfstSpeller, Speller};
use crate::transducer::{
    thfst::{MemmapThfstChunkedTransducer, MemmapThfstTransducer},
    Transducer,
};
use crate::vfs::boxf::Filesystem as BoxFilesystem;
use crate::vfs::Filesystem;

pub type ThfstBoxSpellerArchive = BoxSpellerArchive<
    MemmapThfstTransducer<crate::vfs::boxf::File>,
    MemmapThfstTransducer<crate::vfs::boxf::File>,
>;

pub type ThfstChunkedBoxSpeller = HfstSpeller<
    crate::vfs::boxf::File,
    MemmapThfstChunkedTransducer<crate::vfs::boxf::File>,
    MemmapThfstChunkedTransducer<crate::vfs::boxf::File>,
>;

pub type ThfstBoxSpeller = HfstSpeller<
    crate::vfs::boxf::File,
    MemmapThfstTransducer<crate::vfs::boxf::File>,
    MemmapThfstTransducer<crate::vfs::boxf::File>,
>;

pub type ThfstChunkedBoxSpellerArchive = BoxSpellerArchive<
    MemmapThfstChunkedTransducer<crate::vfs::boxf::File>,
    MemmapThfstChunkedTransducer<crate::vfs::boxf::File>,
>;

pub struct BoxSpellerArchive<T, U>
where
    T: Transducer<crate::vfs::boxf::File>,
    U: Transducer<crate::vfs::boxf::File>,
{
    metadata: Option<SpellerMetadata>,
    speller: Arc<HfstSpeller<crate::vfs::boxf::File, T, U>>,
}

impl<T, U> BoxSpellerArchive<T, U>
where
    T: Transducer<crate::vfs::boxf::File> + Send + Sync + 'static,
    U: Transducer<crate::vfs::boxf::File> + Send + Sync + 'static,
{
    pub fn hfst_speller(&self) -> Arc<HfstSpeller<crate::vfs::boxf::File, T, U>> {
        self.speller.clone()
    }
}

impl<T, U> SpellerArchive for BoxSpellerArchive<T, U>
where
    T: Transducer<crate::vfs::boxf::File> + Send + Sync + 'static,
    U: Transducer<crate::vfs::boxf::File> + Send + Sync + 'static,
{
    fn open(file_path: &std::path::Path) -> Result<BoxSpellerArchive<T, U>, SpellerArchiveError> {
        let archive = BoxFileReader::open(file_path).map_err(|e| {
            SpellerArchiveError::File(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;

        let fs = BoxFilesystem::new(&archive);

        let metadata = fs
            .open_file("meta.json")
            .ok()
            .and_then(|x| serde_json::from_reader(x).ok());
        let errmodel =
            T::from_path(&fs, "errmodel.default.thfst").map_err(SpellerArchiveError::Transducer)?;
        let acceptor =
            U::from_path(&fs, "acceptor.default.thfst").map_err(SpellerArchiveError::Transducer)?;

        let speller = HfstSpeller::new(errmodel, acceptor);
        Ok(BoxSpellerArchive { speller, metadata })
    }

    fn speller(&self) -> Arc<dyn Speller + Send + Sync> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        self.metadata.as_ref()
    }
}

#[cfg(feature = "gpt2")]
pub struct BoxGpt2PredictorArchive {
    #[allow(unused)]
    model_path: std::path::PathBuf,
    model: Arc<crate::predictor::gpt2::Gpt2Predictor>,
    _temp_dir: TempDir, // necessary to keep the temp dir alive until dropped
    metadata: Option<PredictorMetadata>,
}

#[cfg(feature = "gpt2")]
impl PredictorArchive for BoxGpt2PredictorArchive {
    fn open(path: &std::path::Path, predictor_name: Option<&str>) -> Result<Self, PredictorArchiveError>
    where
        Self: Sized,
    {
        let archive = BoxFileReader::open(path).map_err(|e| {
            PredictorArchiveError::File(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;
        let fs = BoxFilesystem::new(&archive);

        let predictor_name = predictor_name.unwrap_or("gpt2_predictor");
        let predictor_path = std::path::Path::new(predictor_name);

        // TODO: make this name customizable via metadata?
        let file = fs
            .open_file(predictor_path.join("meta.json"))
            .map_err(|e| PredictorArchiveError::Io("Could not load meta.json".into(), e))?;

        let metadata = serde_json::from_reader(file)?;

        let temp_dir = fs.copy_to_temp_dir(&predictor_path).map_err(|e| {
            PredictorArchiveError::Io(format!("Could not copy '{}' to temp directory", predictor_name), e)
        })?;
        let model_path = temp_dir.path().join(&predictor_path);

        let model = Arc::new(crate::predictor::gpt2::Gpt2Predictor::new(&model_path)?);

        Ok(BoxGpt2PredictorArchive {
            model_path,
            model,
            _temp_dir: temp_dir,
            metadata,
        })
    }

    fn predictor(&self) -> Arc<dyn crate::predictor::Predictor + Send + Sync> {
        self.model.clone()
    }

    fn metadata(&self) -> Option<&PredictorMetadata> {
        self.metadata.as_ref()
    }
}
