use ::zip::{CompressionMethod, ZipArchive};
use memmap::MmapOptions;
use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::sync::Arc;

use super::error::SpellerArchiveError;
use super::meta::SpellerMetadata;
use super::{MmapRef, SpellerArchive, TempMmap};
use crate::speller::{HfstSpeller, Speller};
use crate::transducer::hfst::HfstTransducer;

pub type HfstZipSpeller =
    HfstSpeller<std::fs::File, HfstTransducer<std::fs::File>, HfstTransducer<std::fs::File>>;

pub struct ZipSpellerArchive {
    metadata: SpellerMetadata,
    speller: Arc<HfstZipSpeller>,
}

fn mmap_by_name<R: Read + Seek>(
    zipfile: &mut File,
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<MmapRef, std::io::Error> {
    let mut index = archive.by_name(name)?;

    if index.compression() != CompressionMethod::Stored {
        let tempdir = tempdir::TempDir::new("divvunspell")?;
        let outpath = tempdir.path().join(index.mangled_name());

        let mut outfile = File::create(&outpath)?;
        std::io::copy(&mut index, &mut outfile)?;

        let outfile = File::open(&outpath)?;

        let mmap = unsafe { MmapOptions::new().map(&outfile) };

        return match mmap {
            Ok(v) => Ok(MmapRef::Temp(TempMmap {
                mmap: Arc::new(v),
                _tempdir: tempdir,
            })),
            Err(err) => return Err(err),
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
        Err(err) => Err(err),
    }
}

impl ZipSpellerArchive {
    pub fn hfst_speller(
        &self,
    ) -> Arc<HfstSpeller<std::fs::File, HfstTransducer<std::fs::File>, HfstTransducer<std::fs::File>>>
    {
        self.speller.clone()
    }
}

impl SpellerArchive for ZipSpellerArchive {
    fn open(file_path: &std::path::Path) -> Result<ZipSpellerArchive, SpellerArchiveError> {
        let file = File::open(&file_path).map_err(SpellerArchiveError::File)?;
        let reader = std::io::BufReader::new(&file);
        let mut archive = ZipArchive::new(reader).expect("zip");

        // // Open file a second time to get around borrow checker
        let mut file = File::open(file_path).map_err(SpellerArchiveError::File)?;

        let metadata_mmap = mmap_by_name(&mut file, &mut archive, "index.xml")
            .map_err(|e| SpellerArchiveError::Io("index.xml".into(), e))?;
        let metadata = SpellerMetadata::from_bytes(&*metadata_mmap.map()).expect("meta");

        let acceptor_id = &metadata.acceptor.id;
        let errmodel_id = &metadata.errmodel.id;

        let acceptor_mmap = mmap_by_name(&mut file, &mut archive, &acceptor_id)
            .map_err(|e| SpellerArchiveError::Io(acceptor_id.into(), e))?;
        let errmodel_mmap = mmap_by_name(&mut file, &mut archive, &errmodel_id)
            .map_err(|e| SpellerArchiveError::Io(errmodel_id.into(), e))?;
        drop(archive);

        let acceptor = HfstTransducer::from_mapped_memory(acceptor_mmap.map());
        let errmodel = HfstTransducer::from_mapped_memory(errmodel_mmap.map());

        let speller = HfstSpeller::new(errmodel, acceptor);

        Ok(ZipSpellerArchive { metadata, speller })
    }

    fn speller(&self) -> Arc<dyn Speller + Send + Sync> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        Some(&self.metadata)
    }
}
