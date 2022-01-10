use rust_bert::RustBertError;
use rust_bert::pipelines::common::ModelType;
use rust_bert::pipelines::text_generation::TextGenerationConfig;
use rust_bert::pipelines::text_generation::TextGenerationModel;
use rust_bert::resources::LocalResource;
use rust_bert::resources::Resource;
use ::zip::{CompressionMethod, ZipArchive};
use memmap2::MmapOptions;
use std::fs::File;
use std::io::prelude::*;
use std::io::Seek;
use std::path::PathBuf;
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
    ai_model: Result<TextGenerationModel, RustBertError>,
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
            .map(&*zipfile)
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
        let config_resource = Resource::Local(LocalResource {
            local_path: PathBuf::from(format!("{:?}/config.json", file_path)),
        });
        let vocab_resource = Resource::Local(LocalResource {
            local_path: PathBuf::from(format!("{:?}/vocab.json", file_path)),
        });
        let merges_resource = Resource::Local(LocalResource {
            local_path: PathBuf::from(format!("{:?}/merges.txt", file_path)),
        });
        let weights_resource = Resource::Local(LocalResource {
            local_path: PathBuf::from(format!("{:?}/rust_model.ot", file_path)),
        });

        let generate_config = TextGenerationConfig {
            model_resource: weights_resource,
            vocab_resource: vocab_resource,
            merges_resource: merges_resource,
            config_resource: config_resource,
            model_type: ModelType::GPT2,
            max_length: 24,
            do_sample: true,
            num_beams: 5,
            temperature: 1.1,
            num_return_sequences: 1,
            ..Default::default()
        };
        let ai_model = TextGenerationModel::new(generate_config);
        // ai_model = Ok(ai_model);
        // ai_model = ai_model.as_ref();
        // Ok(BoxSpellerArchive { speller, metadata, ai_model })
        Ok(ZipSpellerArchive { metadata, speller, ai_model })
    }
    fn ai_model(&self) -> Result<&TextGenerationModel, &RustBertError> { 
        todo!()

    }
    fn speller(&self) -> Arc<dyn Speller + Send> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        Some(&self.metadata)
    }
   
}
