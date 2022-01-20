use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rust_bert::pipelines::common::ModelType;
use rust_bert::pipelines::text_generation::{TextGenerationConfig, TextGenerationModel};
use rust_bert::resources::{LocalResource, Resource};
use rust_bert::RustBertError;

use super::Predictor;

pub struct Gpt2Predictor {
    model: Mutex<TextGenerationModel>,
}

impl Gpt2Predictor {
    pub fn new(model_path: &Path) -> Result<Self, RustBertError> {
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
            num_beams: 1,
            temperature: 1.1,
            num_return_sequences: 1,
            ..Default::default()
        };
        let model = Mutex::new(TextGenerationModel::new(generate_config)?);
        Ok(Self { model })
    }

    fn generate(&self, raw_input: &str) -> Vec<String> {
        let guard = self.model.lock();
        guard.generate(&[raw_input], None)
    }
}

impl Predictor for Gpt2Predictor {
    fn predict(self: Arc<Self>, raw_input: &str) -> Vec<String> {
        self.generate(raw_input)
    }
}
