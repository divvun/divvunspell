use serde_xml_rs::{ParserConfig, deserialize, Error};

#[derive(Deserialize, Debug)]
struct SpellerMetadata {
    info: SpellerMetadataInfo,
    acceptor: SpellerMetadataAcceptor,
    errmodel: SpellerMetadataErrmodel
}

#[derive(Deserialize, Debug)]
struct SpellerMetadataInfo {
    locale: String,
    title: String,
    description: String,
    producer: String
}

#[derive(Deserialize, Debug)]
struct SpellerMetadataAcceptor {
    #[serde(rename = "type", default)]
    type_: String,
    id: String,
    title: String,
    description: String
}

#[derive(Deserialize, Debug)]
struct SpellerMetadataErrmodel {
    //#[serde(rename = "type", default)]
    //type_: String,
    id: String,
    title: String,
    description: String
}

impl SpellerMetadata {
    fn from_str(string: &str) -> Result<SpellerMetadata, Error> {
        let mut reader = ParserConfig::new()
            .trim_whitespace(true)
            .ignore_comments(true)
            .coalesce_characters(true)
            .create_reader(string.as_bytes())
            .into_inner();

        deserialize(&mut reader)
    }
}

#[test]
fn test_xml_parse() {
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
    
    let metadata = SpellerMetadata::from_str(&xml_data).unwrap();
    println!("{:#?}", metadata);
}