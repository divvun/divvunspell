use std::path::Path;

use rust_bert::pipelines::common::ModelType;
use rust_bert::pipelines::text_generation::{TextGenerationConfig, TextGenerationModel};
use rust_bert::resources::{LocalResource, Resource};
use smol_str::SmolStr;

use crate::speller::suggestion::Suggestion;

pub fn load_mlmodel(model_path: &Path) -> Result<TextGenerationModel, Box<dyn std::error::Error>> {
    let config_resource = Resource::Local(LocalResource {
        local_path: model_path.join("config.json"),
    });
    let vocab_resource = Resource::Local(LocalResource {
        local_path: model_path.join("vocab.json"),
    });
    let merges_resource = Resource::Local(LocalResource {
        local_path: model_path.join("merges.txt"),
    });
    let weights_resource = Resource::Local(LocalResource {
        local_path: model_path.join("rust_model.ot"),
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
    let model = TextGenerationModel::new(generate_config)?;
    Ok(model)
}

pub fn generate_suggestions(model: &TextGenerationModel, input: &str) -> Vec<Suggestion> {
    let output = model.generate(&[input.as_ref()], None);

    let mut res: Vec<Suggestion> = vec![];
    for o in output {
        res.push(Suggestion::new(SmolStr::new(o), 0.0));
    }
    res
}

#[cfg(feature = "internal_ffi")]
mod ffi {
    use super::*;
    use crate::speller::ffi::SuggestionVecMarshaler;
    use cffi::{FromForeign, ToForeign};
    use std::sync::Arc;

    #[cffi::marshal(return_marshaler = "cffi::ArcMarshaler::<TextGenerationModel>")]
    pub extern "C" fn divvun_ml_load_model(
        #[marshal(cffi::PathBufMarshaler)] model_path: std::path::PathBuf,
    ) -> Arc<TextGenerationModel> {
        let model = load_mlmodel(&model_path).unwrap();
        Arc::new(model)
    }

    #[cffi::marshal(return_marshaler = "SuggestionVecMarshaler")]
    pub extern "C" fn divvun_ml_suggest(
        #[marshal(cffi::ArcMarshaler::<TextGenerationModel>)] model: Arc<TextGenerationModel>,
        #[marshal(cffi::StrMarshaler)] word: &str,
    ) -> Vec<Suggestion> {
        generate_suggestions(&model, word)
    }
}
