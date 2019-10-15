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

#[cfg(feature = "ffi")]
pub(crate) mod ffi {
    use super::*;
    use crate::transducer::thfst::MemmapThfstChunkedTransducer;
    use cursed::{FromForeign, InputType, ReturnType, ToForeign};
    use std::error::Error;
    use std::ffi::c_void;

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

    pub struct ThfstBoxSpellerArchiveMarshaler;

    impl InputType for ThfstBoxSpellerArchiveMarshaler {
        type Foreign = *const c_void;
    }

    impl ReturnType for ThfstBoxSpellerArchiveMarshaler {
        type Foreign = *const c_void;

        fn foreign_default() -> Self::Foreign {
            std::ptr::null()
        }
    }

    impl ToForeign<Result<ThfstBoxSpellerArchive, SpellerArchiveError>, *const c_void>
        for ThfstBoxSpellerArchiveMarshaler
    {
        type Error = SpellerArchiveError;

        fn to_foreign(
            result: Result<ThfstBoxSpellerArchive, SpellerArchiveError>,
        ) -> Result<*const c_void, Self::Error> {
            result.map(|x| Box::into_raw(Box::new(x)) as *const _)
        }
    }

    impl<'a> FromForeign<*const c_void, &'a ThfstBoxSpellerArchive>
        for ThfstBoxSpellerArchiveMarshaler
    {
        type Error = Box<dyn Error>;

        fn from_foreign(ptr: *const c_void) -> Result<&'a ThfstBoxSpellerArchive, Self::Error> {
            if ptr.is_null() {
                panic!();
            }

            Ok(unsafe { &*ptr.cast() })
        }
    }

    pub struct ThfstChunkedBoxSpellerArchiveMarshaler;

    impl InputType for ThfstChunkedBoxSpellerArchiveMarshaler {
        type Foreign = *const c_void;
    }

    impl ReturnType for ThfstChunkedBoxSpellerArchiveMarshaler {
        type Foreign = *const c_void;

        fn foreign_default() -> Self::Foreign {
            std::ptr::null()
        }
    }

    impl ToForeign<Result<ThfstChunkedBoxSpellerArchive, SpellerArchiveError>, *const c_void>
        for ThfstChunkedBoxSpellerArchiveMarshaler
    {
        type Error = SpellerArchiveError;

        fn to_foreign(
            result: Result<ThfstChunkedBoxSpellerArchive, SpellerArchiveError>,
        ) -> Result<*const c_void, Self::Error> {
            result.map(|x| Box::into_raw(Box::new(x)) as *const _)
        }
    }

    impl<'a> FromForeign<*const c_void, &'a ThfstChunkedBoxSpellerArchive>
        for ThfstChunkedBoxSpellerArchiveMarshaler
    {
        type Error = Box<dyn Error>;

        fn from_foreign(
            ptr: *const c_void,
        ) -> Result<&'a ThfstChunkedBoxSpellerArchive, Self::Error> {
            if ptr.is_null() {
                panic!();
            }

            Ok(unsafe { &*ptr.cast() })
        }
    }

    #[cthulhu::invoke(return_marshaler = "ThfstBoxSpellerArchiveMarshaler")]
    pub extern "C" fn divvun_thfst_box_speller_archive_open(
        #[marshal(cursed::PathMarshaler)] path: &std::path::Path,
    ) -> Result<ThfstBoxSpellerArchive, SpellerArchiveError> {
        ThfstBoxSpellerArchive::open(path)
    }

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler")]
    pub extern "C" fn divvun_thfst_box_speller_archive_speller(
        #[marshal(ThfstBoxSpellerArchiveMarshaler)] handle: &ThfstBoxSpellerArchive,
    ) -> Arc<ThfstBoxSpeller> {
        handle.speller()
    }

    #[cthulhu::invoke(return_marshaler = "ThfstChunkedBoxSpellerArchiveMarshaler")]
    pub extern "C" fn divvun_thfst_chunked_box_speller_archive_open(
        #[marshal(cursed::PathMarshaler)] path: &std::path::Path,
    ) -> Result<ThfstChunkedBoxSpellerArchive, SpellerArchiveError> {
        ThfstChunkedBoxSpellerArchive::open(path)
    }

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler")]
    pub extern "C" fn divvun_thfst_chunked_box_speller_archive_speller(
        #[marshal(ThfstChunkedBoxSpellerArchiveMarshaler)] handle: &ThfstChunkedBoxSpellerArchive,
    ) -> Arc<ThfstChunkedBoxSpeller> {
        handle.speller()
    }
}
