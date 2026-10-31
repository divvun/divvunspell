//! Spellers loaded from the bytes of a speller archive.

use std::sync::Arc;

use divvun_fst::archive::{self, SpellerArchive, meta::SpellerMetadata};
use divvun_fst::speller::{SpellerConfig, suggestion::Suggestion};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::config;
use crate::error::GlueError;

/// The name the archive goes by in error messages: the bytes have none.
const ARCHIVE_NAME: &str = "speller";

/// A speller the package read, with the archive it came from. The wrapper's
/// `Speller` class in `js/fst/speller.ts` owns one.
#[wasm_bindgen]
pub struct Speller {
    archive: Arc<dyn SpellerArchive + Send + Sync>,
    speller: Arc<dyn divvun_fst::speller::Speller + Send + Sync>,
}

/// The speller in a BHFST (box version 0 or 1) or ZHFST archive, told apart
/// by its first bytes. The archive's members are read in place from `bytes`,
/// which the speller keeps, except where a ZHFST member is deflated.
#[wasm_bindgen(js_name = readSpeller)]
pub fn read_speller(bytes: Vec<u8>) -> Result<Speller, GlueError> {
    if bytes.is_empty() {
        return Err(GlueError::new(
            "invalid-format",
            "a speller archive cannot be empty",
        ));
    }
    let archive = archive::from_bytes(bytes, ARCHIVE_NAME)?;
    let speller = archive.speller();
    Ok(Speller { archive, speller })
}

/// The config a call runs with: the one it passed, as JSON text, or, when it
/// passed none, `None` for the speller's own default.
fn config_of(json: Option<String>) -> Result<Option<SpellerConfig>, GlueError> {
    json.as_deref().map(config::parse).transpose()
}

#[wasm_bindgen]
impl Speller {
    /// The archive's metadata as the JSON text of a plain object, or `None`
    /// when the archive carries none, as a BHFST without `meta.json` does.
    pub fn metadata(&self) -> Option<String> {
        self.archive.metadata().map(metadata_json)
    }

    /// The config the archive bundles, as JSON text, or `None` when it
    /// bundles none.
    #[wasm_bindgen(js_name = bundledConfig)]
    pub fn bundled_config(&self) -> Option<String> {
        self.archive.bundled_config().map(config::to_json)
    }

    #[wasm_bindgen(js_name = isCorrect)]
    pub fn is_correct(&self, word: &str, config: Option<String>) -> Result<bool, GlueError> {
        let speller = Arc::clone(&self.speller);
        Ok(match config_of(config)? {
            Some(config) => speller.is_correct_with_config(word, &config),
            None => speller.is_correct(word),
        })
    }

    /// The suggestions for `word`, as the JSON text of an array.
    pub fn suggest(&self, word: &str, config: Option<String>) -> Result<String, GlueError> {
        let speller = Arc::clone(&self.speller);
        let found = match config_of(config)? {
            Some(config) => speller.suggest_with_config(word, &config),
            None => speller.suggest(word),
        };
        Ok(suggestions_json(&found))
    }

    /// The lexicon's analyses of `word` as typed, as the JSON text of an
    /// array.
    #[wasm_bindgen(js_name = analyzeInput)]
    pub fn analyze_input(&self, word: &str, config: Option<String>) -> Result<String, GlueError> {
        let speller = Arc::clone(&self.speller);
        let found = match config_of(config)? {
            Some(config) => speller.analyze_input_with_config(word, &config),
            None => speller.analyze_input(word),
        };
        Ok(suggestions_json(&found))
    }

    /// The suggestions for `word` with their analyses, as the JSON text of an
    /// array.
    #[wasm_bindgen(js_name = analyzeOutput)]
    pub fn analyze_output(&self, word: &str, config: Option<String>) -> Result<String, GlueError> {
        let speller = Arc::clone(&self.speller);
        let found = match config_of(config)? {
            Some(config) => speller.analyze_output_with_config(word, &config),
            None => speller.analyze_output(word),
        };
        Ok(suggestions_json(&found))
    }
}

/// A suggestion as the wrapper returns it. Each weight is the f32 divvun-fst
/// computed, carried as the double that holds it exactly, so a result
/// compares equal to the native one without a tolerance.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuggestionOutput<'a> {
    value: &'a str,
    weight: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight_details: Option<WeightDetailsOutput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WeightDetailsOutput {
    lexicon_weight: f64,
    mutator_weight: f64,
    reweight_start: f64,
    reweight_mid: f64,
    reweight_end: f64,
}

impl<'a> From<&'a Suggestion> for SuggestionOutput<'a> {
    fn from(suggestion: &'a Suggestion) -> Self {
        SuggestionOutput {
            value: suggestion.value(),
            weight: f64::from(suggestion.weight().0),
            completed: suggestion.completed(),
            weight_details: suggestion
                .weight_details()
                .map(|details| WeightDetailsOutput {
                    lexicon_weight: f64::from(details.lexicon_weight.0),
                    mutator_weight: f64::from(details.mutator_weight.0),
                    reweight_start: f64::from(details.reweight_start),
                    reweight_mid: f64::from(details.reweight_mid),
                    reweight_end: f64::from(details.reweight_end),
                }),
        }
    }
}

fn suggestions_json(suggestions: &[Suggestion]) -> String {
    let out: Vec<SuggestionOutput<'_>> = suggestions.iter().map(SuggestionOutput::from).collect();
    serde_json::to_string(&out).expect("suggestions of strings and finite numbers serialise")
}

#[derive(Debug, Serialize)]
struct TitleOutput<'a> {
    lang: Option<&'a str>,
    value: &'a str,
}

#[derive(Debug, Serialize)]
struct InfoOutput<'a> {
    locale: &'a str,
    title: Vec<TitleOutput<'a>>,
    description: &'a str,
    producer: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptorOutput<'a> {
    #[serde(rename = "type")]
    type_: &'a str,
    id: &'a str,
    title: Vec<TitleOutput<'a>>,
    description: &'a str,
    continuation: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ErrorModelOutput<'a> {
    id: &'a str,
    title: Vec<TitleOutput<'a>>,
    description: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetadataOutput<'a> {
    info: InfoOutput<'a>,
    acceptor: AcceptorOutput<'a>,
    error_model: ErrorModelOutput<'a>,
}

fn titles(titles: &[archive::meta::SpellerTitle]) -> Vec<TitleOutput<'_>> {
    titles
        .iter()
        .map(|title| TitleOutput {
            lang: title.lang.as_deref(),
            value: &title.value,
        })
        .collect()
}

fn metadata_json(metadata: &SpellerMetadata) -> String {
    let info = metadata.info();
    let acceptor = metadata.acceptor();
    let errmodel = metadata.errmodel();
    let out = MetadataOutput {
        info: InfoOutput {
            locale: info.locale(),
            title: titles(info.title()),
            description: info.description(),
            producer: info.producer(),
        },
        acceptor: AcceptorOutput {
            type_: acceptor.type_(),
            id: acceptor.id(),
            title: titles(acceptor.title()),
            description: acceptor.description(),
            continuation: acceptor.continuation(),
        },
        error_model: ErrorModelOutput {
            id: errmodel.id(),
            title: titles(errmodel.title()),
            description: errmodel.description(),
        },
    };
    serde_json::to_string(&out).expect("metadata of strings serialises")
}

#[cfg(test)]
mod tests {
    use super::*;
    use divvun_fst::speller::suggestion::WeightDetails;
    use divvun_fst::types::Weight;

    #[test]
    fn suggestions_carry_their_f32_weights_exactly() {
        let suggestions = [
            Suggestion::new("cat".into(), Weight(0.1), None),
            Suggestion::new("cart".into(), Weight(5.0), Some(true)),
        ];
        assert_eq!(
            suggestions_json(&suggestions),
            r#"[{"value":"cat","weight":0.10000000149011612},{"value":"cart","weight":5.0,"completed":true}]"#
        );
    }

    #[test]
    fn verbose_suggestions_carry_their_weight_details() {
        let details = WeightDetails {
            lexicon_weight: Weight(1.0),
            mutator_weight: Weight(5.0),
            reweight_start: 10.0,
            reweight_mid: -1.0,
            reweight_end: 0.0,
        };
        let suggestion = Suggestion::new_with_details("cat".into(), Weight(16.0), None, details);
        assert_eq!(
            suggestions_json(&[suggestion]),
            r#"[{"value":"cat","weight":16.0,"weightDetails":{"lexiconWeight":1.0,"mutatorWeight":5.0,"reweightStart":10.0,"reweightMid":-1.0,"reweightEnd":0.0}}]"#
        );
    }

    #[test]
    fn metadata_names_the_parts_of_the_archive() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <hfstspeller dtdversion="1.0" hfstversion="3">
            <info>
                <locale>se</locale>
                <title>Northern Sami</title>
                <title lang="se">Davvisámegiella</title>
                <description>A speller.</description>
                <producer>Divvun</producer>
            </info>
            <acceptor type="general" id="acceptor.default.hfst">
                <title>Dictionary</title>
                <description>The lexicon.</description>
            </acceptor>
            <errmodel id="errmodel.default.hfst">
                <title>Edit distance</title>
                <description>Misstrokes.</description>
            </errmodel>
            </hfstspeller>"#;
        let metadata = SpellerMetadata::from_bytes(xml.as_bytes()).expect("metadata parses");
        assert_eq!(
            metadata_json(&metadata),
            concat!(
                r#"{"info":{"locale":"se","title":[{"lang":null,"value":"Northern Sami"},{"lang":"se","value":"Davvisámegiella"}],"description":"A speller.","producer":"Divvun"},"#,
                r#""acceptor":{"type":"general","id":"acceptor.default.hfst","title":[{"lang":null,"value":"Dictionary"}],"description":"The lexicon.","continuation":null},"#,
                r#""errorModel":{"id":"errmodel.default.hfst","title":[{"lang":null,"value":"Edit distance"}],"description":"Misstrokes."}}"#
            )
        );
    }

    #[test]
    fn bytes_that_are_no_archive_are_refused_by_their_start() {
        let error = read_speller(vec![0, 1, 2, 3, 4])
            .err()
            .expect("not an archive");
        assert_eq!(error.code(), "unsupported-format");
        assert!(
            error.message().contains("00 01 02 03"),
            "{}",
            error.message()
        );
        let empty = read_speller(Vec::new()).err().expect("empty");
        assert_eq!(empty.code(), "invalid-format");
    }
}
