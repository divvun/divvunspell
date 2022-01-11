#[cfg(feature = "gpt2")]
pub mod gpt2;

use std::sync::Arc;

pub trait Predictor {
    fn predict(self: Arc<Self>, raw_input: &str) -> Vec<String>;
}
