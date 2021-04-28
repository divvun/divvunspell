use std::sync::Arc;

use box_format::BoxFileReader;

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
        let archive = BoxFileReader::open(file_path).map_err(SpellerArchiveError::File)?;

        let fs = BoxFilesystem::new(&archive);

        let metadata = fs
            .open("meta.json")
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
