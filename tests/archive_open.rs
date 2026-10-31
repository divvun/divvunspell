//! BHFST archives opened through [`divvun_fst::archive::open`].
//!
//! `archive::open` reads a box archive's THFST members with the chunked
//! memory-mapped reader, which the other loaders do not use. Each fixture pair
//! is boxed into a BHFST and opened three ways: through `archive::open`, as a
//! [`ThfstBoxSpellerArchive`] with each table in one map, and from the
//! archive's bytes. All three must answer every word identically.

#![cfg(feature = "fs")]

use std::path::{Path, PathBuf};

use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter};
use divvun_fst::archive::{self, SpellerArchive, boxf::ThfstBoxSpellerArchive};
use divvun_fst::speller::SpellerConfig;

const ALIGNMENT: u32 = 8;

const META_JSON: &str = r#"{
    "info": {
        "locale": "se",
        "title": [{ "lang": null, "$value": "Fixture speller" }],
        "description": "Fixture speller",
        "producer": "divvunspell tests"
    },
    "acceptor": {
        "type": "general",
        "id": "acceptor.default.thfst",
        "title": [{ "lang": null, "$value": "Fixture lexicon" }],
        "description": "Fixture lexicon"
    },
    "errmodel": {
        "id": "errmodel.default.thfst",
        "title": [{ "lang": null, "$value": "Fixture error model" }],
        "description": "Fixture error model"
    }
}"#;

/// Inputs drawn from every fixture's alphabet, misspelled and not, so each
/// pair walks its tables to the end on some of them.
const WORDS: &[&str] = &[
    "", "c", "cat", "car", "cart", "care", "kat", "cet", "cad", "ked", "kare", "card", "Cat",
    "CAT", "Kat", "KAT", "xyz", "abc", "abxc", "xbc", "ab", "bc", "cät", "cär", "cäd", "kät", "cä",
    "ät", "cöt", "cöä", "ct", "cäät", "ä", "cbt", "cxt", "ctt", "ca", "at", "tac",
];

fn fixtures_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn insert_thfst(boxfile: &mut BoxWriter, source: &Path, name: &str) {
    boxfile
        .mkdir(BoxPath::new(name).expect("box path"), BoxHashMap::new())
        .expect("mkdir");

    for component in ["alphabet", "index", "transition"] {
        let file = std::fs::File::open(source.join(component)).expect("open fixture component");
        boxfile
            .insert(
                &CompressionConfig::new(Compression::Stored),
                BoxPath::new(Path::new(name).join(component)).expect("box path"),
                std::io::BufReader::new(file),
                BoxHashMap::new(),
            )
            .expect("insert component");
    }
}

fn build_bhfst(dir: &Path, lexicon: &str, mutator: &str) -> PathBuf {
    let path = dir.join("fixture.bhfst");
    let mut boxfile = BoxWriter::create_with_alignment(&path, ALIGNMENT).expect("create bhfst");
    insert_thfst(
        &mut boxfile,
        &fixtures_dir().join(lexicon),
        "acceptor.default.thfst",
    );
    insert_thfst(
        &mut boxfile,
        &fixtures_dir().join(mutator),
        "errmodel.default.thfst",
    );
    boxfile
        .insert(
            &CompressionConfig::new(Compression::Stored),
            BoxPath::new("meta.json").expect("box path"),
            std::io::Cursor::new(META_JSON.as_bytes().to_vec()),
            BoxHashMap::new(),
        )
        .expect("insert meta.json");
    boxfile.finish().expect("finish bhfst");
    path
}

/// Everything a speller says about `word`: whether it is correct, and each
/// suggestion with its weight's bits.
fn answers(archive: &dyn SpellerArchive, word: &str) -> (bool, Vec<(String, u32)>) {
    let config = SpellerConfig {
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let speller = archive.speller();
    let correct = speller.clone().is_correct(word);
    let suggestions = speller
        .suggest_with_config(word, &config)
        .iter()
        .map(|s| (s.value().to_string(), s.weight().0.to_bits()))
        .collect();
    (correct, suggestions)
}

fn assert_readers_agree(lexicon: &str, mutator: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_bhfst(dir.path(), lexicon, mutator);

    let chunked = archive::open(&path).expect("open through archive::open");
    let whole = ThfstBoxSpellerArchive::open(&path).expect("open whole-table archive");
    let bytes = archive::from_bytes(std::fs::read(&path).expect("read bhfst"), &path)
        .expect("open archive from bytes");

    let mut suggested = 0;
    for word in WORDS {
        let expected = answers(&whole, word);
        suggested += expected.1.len();
        assert_eq!(
            answers(chunked.as_ref(), word),
            expected,
            "{lexicon} + {mutator}: archive::open disagrees on {word:?}"
        );
        assert_eq!(
            answers(bytes.as_ref(), word),
            expected,
            "{lexicon} + {mutator}: archive::from_bytes disagrees on {word:?}"
        );
    }
    assert!(
        suggested > 0,
        "{lexicon} + {mutator}: no word suggested anything, so nothing was compared"
    );
}

#[test]
fn lexicon_and_mutator() {
    assert_readers_agree("lexicon.thfst", "mutator.thfst");
}

#[test]
fn flag_lexicon_and_mutator() {
    assert_readers_agree("flag-lexicon.thfst", "flag-mutator.thfst");
}

#[test]
fn identity_lexicon_and_mutator() {
    assert_readers_agree("identity-lexicon.thfst", "identity-mutator.thfst");
}

#[test]
fn eps_lexicon_and_mutator() {
    assert_readers_agree("eps-lexicon.thfst", "eps-mutator.thfst");
}

#[test]
fn reorder_lexicon_and_mutator() {
    assert_readers_agree("reorder-lexicon.thfst", "reorder-mutator.thfst");
}

#[test]
fn lexicon_and_wildcard_compact_mutator() {
    assert_readers_agree("lexicon.thfst", "wildcard-compact-mutator.thfst");
}

#[test]
fn lexicon_and_wildcard_expanded_mutator() {
    assert_readers_agree("lexicon.thfst", "wildcard-expanded-mutator.thfst");
}

#[test]
fn unknown_out_lexicon_and_compact_mutator() {
    assert_readers_agree(
        "unknown-out-lexicon.thfst",
        "unknown-out-compact-mutator.thfst",
    );
}

#[test]
fn unknown_out_lexicon_and_expanded_mutator() {
    assert_readers_agree(
        "unknown-out-lexicon.thfst",
        "unknown-out-expanded-mutator.thfst",
    );
}
