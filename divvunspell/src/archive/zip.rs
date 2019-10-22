use ::zip::{CompressionMethod, ZipArchive};
use memmap::MmapOptions;
use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::sync::Arc;

use super::error::SpellerArchiveError;
use super::meta::SpellerMetadata;
use super::{MmapRef, TempMmap};
use crate::speller::Speller;
use crate::transducer::hfst::HfstTransducer;

pub struct ZipSpellerArchive {
    metadata: SpellerMetadata,
    speller:
        Arc<Speller<std::fs::File, HfstTransducer<std::fs::File>, HfstTransducer<std::fs::File>>>,
}

fn mmap_by_name<R: Read + Seek>(
    zipfile: &mut File,
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<MmapRef, std::io::Error> {
    let mut index = archive.by_name(name).unwrap();

    if index.compression() != CompressionMethod::Stored {
        let tempdir = tempdir::TempDir::new("divvunspell")?;
        let outpath = tempdir.path().join(index.sanitized_name());

        let mut outfile = File::create(&outpath)?;
        std::io::copy(&mut index, &mut outfile)?;

        let outfile = File::open(&outpath)?;

        let mmap = unsafe { MmapOptions::new().map(&outfile) };

        return match mmap {
            Ok(v) => Ok(MmapRef::Temp(TempMmap {
                mmap: Arc::new(v),
                _tempdir: tempdir,
            })),
            Err(err) => panic!(err),
        };
    }

    let mmap = unsafe {
        MmapOptions::new()
            .offset(index.data_start())
            .len(index.size() as usize)
            .map(&zipfile)
    };

    match mmap {
        Ok(v) => Ok(MmapRef::Direct(Arc::new(v))),
        Err(err) => panic!(err),
    }
}

impl ZipSpellerArchive {
    pub fn open<P: AsRef<std::path::Path>>(
        file_path: P,
    ) -> Result<ZipSpellerArchive, SpellerArchiveError> {
        let file = File::open(&file_path).map_err(SpellerArchiveError::File)?;
        let reader = std::io::BufReader::new(&file);
        let mut archive = ZipArchive::new(reader).expect("zip");

        // // Open file a second time to get around borrow checker
        let mut file = File::open(file_path).map_err(SpellerArchiveError::File)?;

        let metadata_mmap =
            mmap_by_name(&mut file, &mut archive, "index.xml").map_err(SpellerArchiveError::Io)?;
        let metadata = SpellerMetadata::from_bytes(&*metadata_mmap.map()).expect("meta");

        let acceptor_mmap = mmap_by_name(&mut file, &mut archive, &metadata.acceptor.id)
            .map_err(SpellerArchiveError::Io)?;
        let errmodel_mmap = mmap_by_name(&mut file, &mut archive, &metadata.errmodel.id)
            .map_err(SpellerArchiveError::Io)?;
        drop(archive);

        let acceptor = HfstTransducer::from_mapped_memory(acceptor_mmap.map());
        let errmodel = HfstTransducer::from_mapped_memory(errmodel_mmap.map());

        let speller = Speller::new(errmodel, acceptor);

        Ok(ZipSpellerArchive { metadata, speller })
    }

    pub fn speller(
        &self,
    ) -> Arc<Speller<std::fs::File, HfstTransducer<std::fs::File>, HfstTransducer<std::fs::File>>>
    {
        self.speller.clone()
    }

    pub fn metadata(&self) -> Option<&SpellerMetadata> {
        Some(&self.metadata)
    }
}

#[cfg(feature = "ffi")]
pub(crate) mod ffi {
    use super::*;
    use cursed::{FromForeign, InputType, ReturnType, ToForeign};
    use std::error::Error;
    use std::ffi::c_void;

    pub type HfstZipSpeller = Speller<std::fs::File, HfstTransducer<std::fs::File>, HfstTransducer<std::fs::File>>;

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler::<ZipSpellerArchive>")]
    pub extern "C" fn divvun_hfst_zip_speller_archive_open(
        #[marshal(cursed::PathMarshaler)] path: &std::path::Path,
    ) -> Result<Arc<ZipSpellerArchive>, Box<dyn Error>> {
        ZipSpellerArchive::open(path)
            .map(|x| Arc::new(x))
            .map_err(|e| Box::new(e) as _)
    }

    #[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler::<HfstZipSpeller>")]
    pub extern "C" fn divvun_hfst_zip_speller_archive_speller(
        #[marshal(cursed::ArcRefMarshaler::<ZipSpellerArchive>)] handle: &Arc<ZipSpellerArchive>,
    ) -> Arc<HfstZipSpeller> {
        handle.speller()
    }

    #[cthulhu::invoke(return_marshaler = "cursed::StringMarshaler")]
    pub extern "C" fn divvun_hfst_zip_speller_archive_locale(
        #[marshal(cursed::ArcRefMarshaler::<ZipSpellerArchive>)] handle: &ZipSpellerArchive,
    ) -> Result<String, Box<dyn Error>> {
        match handle.metadata() {
            Some(v) => Ok(v.info.locale.to_string()),
            None => Err(Box::new(SpellerArchiveError::NoMetadata) as _),
        }
    }
}
