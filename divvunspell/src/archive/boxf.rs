// use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
// use rust_bert::Config;
// use rust_bert::gpt2::Gpt2Config;
// use tempfile::tempfile;
// use tempfile::NamedTempFile;
// use std::io::{self, Write, Read};
// std::env::temp_dir;
use box_format::BoxFileReader;
use rust_bert::RustBertError;
use rust_bert::pipelines::common::ModelType;
use rust_bert::pipelines::text_generation::{TextGenerationModel, TextGenerationConfig};

use super::error::SpellerArchiveError;
use super::{meta::SpellerMetadata, SpellerArchive};
use crate::speller::{HfstSpeller, Speller};
use crate::transducer::{
    thfst::{MemmapThfstChunkedTransducer, MemmapThfstTransducer},
    Transducer,
};
use rust_bert::resources::{LocalResource, Resource};

use crate::vfs::boxf::{Filesystem as BoxFilesystem, File};
use crate::vfs::{Filesystem, self};

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
    ai_model: Result<TextGenerationModel, RustBertError>,
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
    T: Transducer<crate::vfs::boxf::File> + Send + 'static,
    U: Transducer<crate::vfs::boxf::File> + Send + 'static,
{
    fn open(file_path: &std::path::Path) -> Result<BoxSpellerArchive<T, U>, SpellerArchiveError> {
        let archive = BoxFileReader::open(file_path).map_err(SpellerArchiveError::File)?;

        let fs = BoxFilesystem::new(&archive);

        let metadata = fs
            .open("meta.json")
            .ok()
            .and_then(|x| serde_json::from_reader(x).ok());
        // println!("{:?}", metadata);
        let errmodel =
            T::from_path(&fs, "errmodel.default.thfst").map_err(SpellerArchiveError::Transducer)?;
        let acceptor =
            U::from_path(&fs, "acceptor.default.thfst").map_err(SpellerArchiveError::Transducer)?;

        let speller = HfstSpeller::new(errmodel, acceptor);
        
        let config_file = fs
            .open("config.json")
            .ok();
        //     .and_then(|x|serde_json::from_reader(x).ok());
        // let file: Result<vfs::boxf::File, dyn serde::ser::StdError> = serde_json::from_reader(config_file.unwrap());
        // println!("{:?}", file);
        

        // let tmp_dir = std::env::temp_dir().join("config.json");
        
        // tmp_dir.write_all(config_file.unwrap());
        // let cfg_path = tmp_dir.unwrap().path().join("config.json");
        // let mut cfg_file = File::create(cfg_path);
        // println!("{:?}", cfg_file);

        // let config = config_file
        //     .ok()
        //     .and_then(|x| Gpt2Config::from_file(x.path()).ok());
        // println!("{:?}", config);
            
        let config_resource = Resource::Local(LocalResource {
            local_path: file_path.join("config.json")                                       
        });
        let vocab_resource = Resource::Local(LocalResource {
            local_path: file_path.join("vocab.json"),
        });
        let merges_resource = Resource::Local(LocalResource {
            local_path: file_path.join("merges.txt"),   
        });
        let weights_resource = Resource::Local(LocalResource {
            local_path: file_path.join("rust_model.ot"),
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
        Ok(BoxSpellerArchive { speller, metadata, ai_model })
    }

    fn speller(&self) -> Arc<dyn Speller + Send> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        self.metadata.as_ref()
    }

    fn ai_model(&self) -> Result<&TextGenerationModel, &RustBertError> {
        self.ai_model.as_ref()
    }
}

// fn write_temp_dir() -> Result<File, Erorr> {
    // 
// }
