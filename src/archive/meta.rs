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
    info: SpellerMetadataInfo,
    /// acceptor metadata
    acceptor: SpellerMetadataAcceptor,
    /// error model metadata
    errmodel: SpellerMetadataErrmodel,
}

impl SpellerMetadata {
    /// Get the speller information
    pub fn info(&self) -> &SpellerMetadataInfo {
        &self.info
    }

    /// Get the acceptor metadata
    pub fn acceptor(&self) -> &SpellerMetadataAcceptor {
        &self.acceptor
    }

    /// Get the error model metadata
    pub fn errmodel(&self) -> &SpellerMetadataErrmodel {
        &self.errmodel
    }

    /// Get mutable reference to acceptor metadata
    ///
    /// # Warning
    /// This method is only for internal tooling use and should not be used in normal applications.
    /// It may be removed in a future version.
    #[doc(hidden)]
    pub fn acceptor_mut(&mut self) -> &mut SpellerMetadataAcceptor {
        &mut self.acceptor
    }

    /// Get mutable reference to error model metadata
    ///
    /// # Warning
    /// This method is only for internal tooling use and should not be used in normal applications.
    /// It may be removed in a future version.
    #[doc(hidden)]
    pub fn errmodel_mut(&mut self) -> &mut SpellerMetadataErrmodel {
        &mut self.errmodel
    }
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
    locale: String,
    /// localised, human readable titles of speller
    title: Vec<SpellerTitle>,
    /// human readable description of speller
    description: String,
    /// creator and copyright owner of the speller
    producer: String,
}

impl SpellerMetadataInfo {
    /// Get the ISO-639 locale code
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Get the localized titles
    pub fn title(&self) -> &[SpellerTitle] {
        &self.title
    }

    /// Get the description
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Get the producer/creator
    pub fn producer(&self) -> &str {
        &self.producer
    }
}

/// Acceptor metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataAcceptor {
    /// acceptor type:
    /// - `blah` if normal dictionary automaton
    /// - `foo` if analyzer
    #[serde(rename = "type", default)]
    type_: String,
    /// locally unique id for this acceptor
    id: String,
    /// localised human readable titles of speller
    title: Vec<SpellerTitle>,
    /// human readable description of the acceptor
    description: String,
    /// marker for incomplete strings
    continuation: Option<String>,
}

impl SpellerMetadataAcceptor {
    /// Get the acceptor type
    pub fn type_(&self) -> &str {
        &self.type_
    }

    /// Get the acceptor ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the localized titles
    pub fn title(&self) -> &[SpellerTitle] {
        &self.title
    }

    /// Get the description
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Get the continuation marker for incomplete strings
    pub fn continuation(&self) -> Option<&str> {
        self.continuation.as_deref()
    }

    /// Set the acceptor ID
    ///
    /// # Warning
    /// This method is only for internal tooling use and should not be used in normal applications.
    /// It may be removed in a future version.
    #[doc(hidden)]
    pub fn set_id(&mut self, id: String) {
        self.id = id;
    }
}

/// Error model metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpellerMetadataErrmodel {
    /// locally unique id for the error model
    id: String,
    /// localised human readable titles for the error model
    title: Vec<SpellerTitle>,
    /// human readable description of the error model
    description: String,
}

impl SpellerMetadataErrmodel {
    /// Get the error model ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the localized titles
    pub fn title(&self) -> &[SpellerTitle] {
        &self.title
    }

    /// Get the description
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Set the error model ID
    ///
    /// # Warning
    /// This method is only for internal tooling use and should not be used in normal applications.
    /// It may be removed in a future version.
    #[doc(hidden)]
    pub fn set_id(&mut self, id: String) {
        self.id = id;
    }
}

impl std::str::FromStr for SpellerMetadata {
    type Err = Error;

    fn from_str(string: &str) -> Result<SpellerMetadata, Error> {
        SpellerMetadata::from_bytes(string.as_bytes())
    }
}

impl SpellerMetadata {
    /// Parse speller metadata from XML bytes.
    ///
    /// The XML format follows the HFST speller specification with `<info>`,
    /// `<acceptor>`, and `<errmodel>` elements containing metadata about
    /// the spell-checker and its component transducers.
    ///
    /// # Errors
    ///
    /// Returns an error if the XML is malformed or doesn't match the expected schema.
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
