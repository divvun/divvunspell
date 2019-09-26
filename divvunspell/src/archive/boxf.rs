use std::sync::Arc;

use box_format::BoxFileReader;

use super::error::SpellerArchiveError;
use super::meta::SpellerMetadata;
use crate::speller::Speller;
use crate::transducer::{thfst::MemmapThfstTransducer, Transducer};
use crate::vfs::boxf::Filesystem as BoxFilesystem;
use crate::vfs::Filesystem;

pub type ThfstBoxSpellerArchive = BoxSpellerArchive<
    MemmapThfstTransducer<crate::vfs::boxf::File>,
    MemmapThfstTransducer<crate::vfs::boxf::File>,
>;

pub struct BoxSpellerArchive<T, U>
where
    T: Transducer<crate::vfs::boxf::File>,
    U: Transducer<crate::vfs::boxf::File>,
{
    metadata: Option<SpellerMetadata>,
    speller: Arc<Speller<crate::vfs::boxf::File, T, U>>,
}

impl<T, U> BoxSpellerArchive<T, U>
where
    T: Transducer<crate::vfs::boxf::File>,
    U: Transducer<crate::vfs::boxf::File>,
{
    pub fn open<P: AsRef<std::path::Path>>(
        file_path: P,
    ) -> Result<BoxSpellerArchive<T, U>, SpellerArchiveError> {
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

        let speller = Speller::new(errmodel, acceptor);
        Ok(BoxSpellerArchive { speller, metadata })
    }

    pub fn speller(&self) -> Arc<Speller<crate::vfs::boxf::File, T, U>> {
        self.speller.clone()
    }

    pub fn metadata(&self) -> Option<&SpellerMetadata> {
        self.metadata.as_ref()
    }
}
