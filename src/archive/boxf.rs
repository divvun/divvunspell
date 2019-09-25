use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::sync::Arc;

use memmap::{Mmap, MmapOptions};
use box_format::{BoxFileReader, Compression};

use self::meta::SpellerMetadata;
use crate::speller::Speller;
use crate::transducer::Transducer;

use super::*;

pub struct BoxSpellerArchive<T: Transducer> {
    metadata: SpellerMetadata,
    speller: Arc<Speller<T>>,
}

fn mmap_by_name<'a, R: Read + Seek>(
    boxfile: &mut BoxFileReader,
    name: &str,
) -> Result<MmapRef, std::io::Error> {
    let record = boxfile.metadata()
        .records()
        .iter()
        .find_map(|r| r.as_file().filter(|f| f.path() == name))
        .unwrap();

    if record.compression != Compression::Stored {
        let tempdir = tempdir::TempDir::new("divvunspell")?;
        let outpath = tempdir.path().join(name);

        let mut outfile = File::create(&outpath)?;
        {
            let data = unsafe { boxfile.data(&record) }?;
            std::io::copy(&mut std::io::Cursor::new(data), &mut outfile)?;
        }

        let outfile = File::open(&outpath)?;

        let mmap = unsafe { MmapOptions::new().map(&outfile) };

        return match mmap {
            Ok(v) => Ok(MmapRef::Temp(TempMmap {
                mmap: Arc::new(v),
                tempdir,
            })),
            Err(err) => panic!(err),
        };
    }

    let mmap = unsafe { boxfile.data(&record) };

    match mmap {
        Ok(v) => Ok(MmapRef::Direct(Arc::new(v))),
        Err(err) => panic!(err),
    }
}

impl<T: Transducer> BoxSpellerArchive<T> {
    pub fn new(file_path: &str) -> Result<BoxSpellerArchive<T>, SpellerArchiveError> {
        let archive = BoxFileReader::open(file_path)
            .map_err(SpellerArchiveError::OpenFileFailed)?;

        // let metadata = {
        //     let metadata_mmap = mmap_by_name(&mut archive, "index.xml")
        //         .map_err(SpellerArchiveError::MetadataMmapFailed)?;
        //     SpellerMetadata::from_bytes(&*metadata_mmap.map()).expect("meta")
        // };

        let acceptor_mmap = mmap_by_name(&mut archive, &format!("acceptor.{}", T::FILE_EXT))
            .map_err(SpellerArchiveError::AcceptorMmapFailed)?;
        let errmodel_mmap = mmap_by_name(&mut archive, &format!("errmodel.{}", T::FILE_EXT))
            .map_err(SpellerArchiveError::ErrmodelMmapFailed)?;
            
        let acceptor = T::from_mapped_memory(acceptor_mmap.map());
        let errmodel = T::from_mapped_memory(errmodel_mmap.map());
        
        // let file = File::open(file_path).map_err(SpellerArchiveError::OpenFileFailed)?;
        // let reader = std::io::BufReader::new(&file);
        // let mut archive = ZipArchive::new(reader).expect("zip");

        // // Open file a second time to get around borrow checker
        // let mut file = File::open(file_path).map_err(SpellerArchiveError::OpenFileFailed)?;


        // let acceptor_mmap = mmap_by_name(&mut file, &mut archive, &metadata.acceptor.id)
        //     .map_err(SpellerArchiveError::AcceptorMmapFailed)?;
        // let errmodel_mmap = mmap_by_name(&mut file, &mut archive, &metadata.errmodel.id)
        //     .map_err(SpellerArchiveError::ErrmodelMmapFailed)?;
        // drop(archive);


        // let speller = Speller::new(errmodel, acceptor);

        // Ok(ZipSpellerArchive { metadata, speller })
        // 
        unimplemented!();
    }

    pub fn speller(&self) -> Arc<Speller<T>> {
        self.speller.clone()
    }

    pub fn metadata(&self) -> &SpellerMetadata {
        &self.metadata
    }
}
