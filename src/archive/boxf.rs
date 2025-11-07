//! Box-based archive stuff.
use std::sync::Arc;

use box_format::BoxFileReader;

use super::error::SpellerArchiveError;
use super::{SpellerArchive, meta::SpellerMetadata};
use crate::speller::{HfstSpeller, Speller};
use crate::transducer::{
    Transducer,
    thfst::{MmapThfstTransducer, chunked::MmapThfstChunkedTransducer},
};
use crate::vfs::Filesystem;
use crate::vfs::boxf::Filesystem as BoxFilesystem;

/// An archive with mmaped language and error model THFST automata archive.
pub type ThfstBoxSpellerArchive = BoxSpellerArchive<
    MmapThfstTransducer<crate::vfs::boxf::File>,
    MmapThfstTransducer<crate::vfs::boxf::File>,
>;

/// An archive with mmaped chunked language and error model THFST automata
/// file.
pub type ThfstChunkedBoxSpeller = HfstSpeller<
    crate::vfs::boxf::File,
    MmapThfstChunkedTransducer<crate::vfs::boxf::File>,
    MmapThfstChunkedTransducer<crate::vfs::boxf::File>,
>;

/// An archive with mmaped language and error model THFST automata file.
pub type ThfstBoxSpeller = HfstSpeller<
    crate::vfs::boxf::File,
    MmapThfstTransducer<crate::vfs::boxf::File>,
    MmapThfstTransducer<crate::vfs::boxf::File>,
>;

/// An archive with mmaped chunked language and error model THFST automata
/// archive.
pub type ThfstChunkedBoxSpellerArchive = BoxSpellerArchive<
    MmapThfstChunkedTransducer<crate::vfs::boxf::File>,
    MmapThfstChunkedTransducer<crate::vfs::boxf::File>,
>;

/// Speller in box archive.
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
    /// get the spell-checking component
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
