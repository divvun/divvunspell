use std::sync::Arc;

use box_format::BoxFileReader;

use super::error::SpellerArchiveError;
use super::meta::SpellerMetadata;
use crate::speller::Speller;
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

pub type ThfstChunkedBoxSpeller = Speller<
    crate::vfs::boxf::File,
    MemmapThfstChunkedTransducer<crate::vfs::boxf::File>,
    MemmapThfstChunkedTransducer<crate::vfs::boxf::File>,
>;

pub type ThfstBoxSpeller = Speller<
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

#[cfg(feature = "ffi")]
pub(crate) mod ffi {
    use super::*;
    use cursed::{FromForeign, InputType, ReturnType, ToForeign};
    use std::error::Error;
    use std::ffi::c_void;

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler::<ThfstBoxSpellerArchive>")]
    pub extern "C" fn divvun_thfst_box_speller_archive_open(
        #[marshal(cursed::PathMarshaler)] path: std::path::PathBuf,
    ) -> Result<Arc<ThfstBoxSpellerArchive>, Box<dyn Error>> {
        ThfstBoxSpellerArchive::open(&path)
            .map(|x| Arc::new(x))
            .map_err(|e| Box::new(e) as _)
    }

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler::<ThfstBoxSpeller>")]
    pub extern "C" fn divvun_thfst_box_speller_archive_speller(
        #[marshal(cursed::ArcRefMarshaler::<ThfstBoxSpellerArchive>)] handle: Arc<
            ThfstBoxSpellerArchive,
        >,
    ) -> Arc<ThfstBoxSpeller> {
        handle.speller()
    }

    #[cthulhu::invoke(return_marshaler = "cursed::StringMarshaler")]
    pub extern "C" fn divvun_thfst_box_speller_archive_locale(
        #[marshal(cursed::ArcRefMarshaler::<ThfstBoxSpellerArchive>)] handle: Arc<
            ThfstBoxSpellerArchive,
        >,
    ) -> Result<String, Box<dyn Error>> {
        match handle.metadata() {
            Some(v) => Ok(v.info.locale.to_string()),
            None => Err(Box::new(SpellerArchiveError::NoMetadata) as _),
        }
    }

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler::<ThfstChunkedBoxSpellerArchive>")]
    pub extern "C" fn divvun_thfst_chunked_box_speller_archive_open(
        #[marshal(cursed::PathMarshaler)] path: std::path::PathBuf,
    ) -> Result<Arc<ThfstChunkedBoxSpellerArchive>, Box<dyn Error>> {
        ThfstChunkedBoxSpellerArchive::open(&path)
            .map(|x| Arc::new(x))
            .map_err(|e| Box::new(e) as _)
    }

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler::<ThfstChunkedBoxSpeller>")]
    pub extern "C" fn divvun_thfst_chunked_box_speller_archive_speller(
        #[marshal(cursed::ArcRefMarshaler::<ThfstChunkedBoxSpellerArchive>)] handle: Arc<
            ThfstChunkedBoxSpellerArchive,
        >,
    ) -> Arc<ThfstChunkedBoxSpeller> {
        handle.speller()
    }

    #[cthulhu::invoke(return_marshaler = "cursed::StringMarshaler")]
    pub extern "C" fn divvun_thfst_chunked_box_speller_archive_locale(
        #[marshal(cursed::ArcRefMarshaler::<ThfstChunkedBoxSpellerArchive>)] handle: Arc<
            ThfstChunkedBoxSpellerArchive,
        >,
    ) -> Result<String, Box<dyn Error>> {
        match handle.metadata() {
            Some(v) => Ok(v.info.locale.to_string()),
            None => Err(Box::new(SpellerArchiveError::NoMetadata) as _),
        }
    }
}
