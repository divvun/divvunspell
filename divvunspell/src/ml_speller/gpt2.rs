use std::path::PathBuf;

// use tch::{nn, Device};
use rust_bert::pipelines::common::ModelType;
use rust_bert::pipelines::text_generation::{TextGenerationConfig, TextGenerationModel};
// use rust_bert::gpt2::{GPT2LMHeadModel, Gpt2Config};
use rust_bert::resources::{LocalResource, Resource};
use smol_str::SmolStr;

use crate::speller::suggestion::AISuggestion;
// use rust_bert::Config;
// use rust_tokenizers::tokenizer::Gpt2Tokenizer;

pub fn load_mlmodel() -> Result<TextGenerationModel, Box<dyn std::error::Error>> {
    let config_resource = Resource::Local(LocalResource {
        local_path: PathBuf::from("model_big_1024/config.json"),
    });
    let vocab_resource = Resource::Local(LocalResource {
        local_path: PathBuf::from("model_big_1024/vocab.json"),
    });
    let merges_resource = Resource::Local(LocalResource {
        local_path: PathBuf::from("model_big_1024/merges.txt"),
    });
    let weights_resource = Resource::Local(LocalResource {
        local_path: PathBuf::from("model_big_1024/rust_model.ot"),
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

pub fn generate_suggestions(model: &TextGenerationModel, input: &String) -> Vec<AISuggestion> {
    // let input_context = "Gaskab";
    let output = model.generate(&[input.as_ref()], None);

    // for sentence in output {
    //     println!("{:?}", sentence);
    // }
    let mut res: Vec<AISuggestion> = vec![];
    for o in output {
        res.push(AISuggestion::new(SmolStr::new(o)));
    }
    res
}
