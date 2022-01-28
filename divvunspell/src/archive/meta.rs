use serde::{Deserialize, Serialize};
use serde_xml_rs::{from_reader, Error, ParserConfig};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadata {
    pub info: SpellerMetadataInfo,
    pub acceptor: SpellerMetadataAcceptor,
    pub errmodel: SpellerMetadataErrmodel,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct PredictorMetadata {
    #[serde(default)]
    pub speller: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerTitle {
    pub lang: Option<String>,
    #[serde(rename = "$value")]
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataInfo {
    pub locale: String,
    pub title: Vec<SpellerTitle>,
    pub description: String,
    pub producer: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataAcceptor {
    #[serde(rename = "type", default)]
    pub type_: String,
    pub id: String,
    pub title: Vec<SpellerTitle>,
    pub description: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataErrmodel {
    pub id: String,
    pub title: Vec<SpellerTitle>,
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
            on the normative subset of the morphological analyser for Northern Sami.
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
