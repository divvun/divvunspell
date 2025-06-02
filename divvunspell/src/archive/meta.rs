//! Data structures of speller metadata.
//!
//! These are usually read from the speller archives, in xml or json files or
//! such. XML format is described here and json format there.
use serde::{Deserialize, Serialize};
use serde_xml_rs::{from_reader, Error, ParserConfig};

/// Speller metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadata {
    /// speller info
    pub info: SpellerMetadataInfo,
    /// acceptor metadata
    pub acceptor: SpellerMetadataAcceptor,
    /// error model metadata
    pub errmodel: SpellerMetadataErrmodel,
}

/// Predictor metadata
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct PredictorMetadata {
    /// whether speller is
    #[serde(default)]
    pub speller: bool,
}

/// localised speller title
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerTitle {
    /// ISO 639 code of the title's content language
    pub lang: Option<String>,
    /// translated title
    #[serde(rename = "$value")]
    pub value: String,
}

/// Speller metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataInfo {
    /// ISO-639 code of speller language
    pub locale: String,
    /// localised, human readable titles of speller
    pub title: Vec<SpellerTitle>,
    /// human readable description of speller
    pub description: String,
    /// creator and copyright owner of the speller
    pub producer: String,
}

/// Acceptor metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataAcceptor {
    /// acceptor type:
    /// - `blah` if normal dictionary automaton
    /// - `foo` if analyser
    #[serde(rename = "type", default)]
    pub type_: String,
    /// locally unique id for this acceptor
    pub id: String,
    /// localised human readable titles of speller
    pub title: Vec<SpellerTitle>,
    /// human readable description of the acceptor
    pub description: String,
    /// marker for incomplete strings
    pub continuation: Option<String>,
}

/// Error model metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataErrmodel {
    /// locally unique id for the error model
    pub id: String,
    /// localised human readable titles for the error model
    pub title: Vec<SpellerTitle>,
    /// human readable description of the error model
    pub description: String,
}

impl std::str::FromStr for SpellerMetadata {
    type Err = Error;

    fn from_str(string: &str) -> Result<SpellerMetadata, Error> {
        SpellerMetadata::from_bytes(string.as_bytes())
    }
}

impl SpellerMetadata {
    pub fn from_bytes(bytes: &[u8]) -> Result<SpellerMetadata, Error> {
        let mut reader = ParserConfig::new()
            .trim_whitespace(true)
            .ignore_comments(true)
            .coalesce_characters(true)
            .create_reader(bytes)
            .into_inner();

        from_reader(&mut reader)
    }
}

impl PredictorMetadata {
    pub fn from_bytes(bytes: &[u8]) -> Result<PredictorMetadata, Error> {
        let mut reader = ParserConfig::new()
            .trim_whitespace(true)
            .ignore_comments(true)
            .coalesce_characters(true)
            .create_reader(bytes)
            .into_inner();

        from_reader(&mut reader)
    }
}

#[test]
fn test_xml_parse() {
    use std::str::FromStr;

    let xml_data = r##"
        <?xml version="1.0" encoding="UTF-8"?>
        <hfstspeller dtdversion="1.0" hfstversion="3">
        <info>
            <locale>se</locale>
            <title>Giellatekno/Divvun/UiT fst-based speller for Northern Sami</title>
            <description>This is an fst-based speller for Northern Sami. It is based
            on the normative subset of the morphological analyzer for Northern Sami.
            The source code can be found at:
            https://victorio.uit.no/langtech/trunk/langs/sme/
            License: GPL3+.</description>
            <version vcsrev="GT_REVISION">GT_VERSION</version>
            <date>DATE</date>
            <producer>Giellatekno/Divvun/UiT contributors</producer>
            <contact email="feedback@divvun.no" website="http://divvun.no"/>
        </info>
        <acceptor type="general" id="acceptor.default.hfst">
            <title>Giellatekno/Divvun/UiT dictionary Northern Sami</title>
            <description>Giellatekno/Divvun/UiT dictionary for
            Northern Sami compiled for HFST.</description>
        </acceptor>
        <errmodel id="errmodel.default.hfst">
            <title>Levenshtein edit distance transducer</title>
            <description>Correction model for keyboard misstrokes, at most 2 per
            word.</description>
            <type type="default"/>
            <model>errormodel.default.hfst</model>
        </errmodel>
        </hfstspeller>
    "##;

    let s = SpellerMetadata::from_str(&xml_data).unwrap();
    println!("{:#?}", s);
}
