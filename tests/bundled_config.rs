//! Speller configuration bundled inside an archive.
//!
//! A speller's runtime parameters are measured against the language it ships
//! for, so an archive may carry its own — a `speller-config.json` member in a
//! ZHFST, a `spellerConfig` key in a BHFST's `meta.json`. These tests run the
//! BHFST side end to end, because a box archive can be built from the small
//! THFST fixtures; the ZHFST member reading is covered by unit tests in
//! `src/archive/zip.rs`, which need no transducers.

use std::path::{Path, PathBuf};

use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter};
use divvun_fst::archive::{SpellerArchive, boxf::ThfstBoxSpellerArchive};
use divvun_fst::speller::SpellerConfig;

const ALIGNMENT: u32 = 8;

fn fixtures_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

/// `meta.json` for the fixture archive, with `extra` spliced in as further
/// keys — that being where a BHFST's bundled config lives.
fn meta_json(extra: &str) -> String {
    format!(
        r#"{{
            "info": {{
                "locale": "se",
                "title": [{{ "lang": null, "$value": "Fixture speller" }}],
                "description": "Fixture speller",
                "producer": "divvunspell tests"
            }},
            "acceptor": {{
                "type": "general",
                "id": "acceptor.default.thfst",
                "title": [{{ "lang": null, "$value": "Fixture lexicon" }}],
                "description": "Fixture lexicon"
            }},
            "errmodel": {{
                "id": "errmodel.default.thfst",
                "title": [{{ "lang": null, "$value": "Fixture error model" }}],
                "description": "Fixture error model"
            }}{}
        }}"#,
        extra
    )
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

/// A BHFST holding the standard test fixtures and the given `meta.json`.
fn build_bhfst(dir: &Path, meta_json: &str) -> PathBuf {
    let path = dir.join("fixture.bhfst");
    let mut boxfile = BoxWriter::create_with_alignment(&path, ALIGNMENT).expect("create bhfst");

    insert_thfst(
        &mut boxfile,
        &fixtures_dir().join("lexicon.thfst"),
        "acceptor.default.thfst",
    );
    insert_thfst(
        &mut boxfile,
        &fixtures_dir().join("mutator.thfst"),
        "errmodel.default.thfst",
    );

    boxfile
        .insert(
            &CompressionConfig::new(Compression::Stored),
            BoxPath::new("meta.json").expect("box path"),
            std::io::Cursor::new(meta_json.as_bytes().to_vec()),
            BoxHashMap::new(),
        )
        .expect("insert meta.json");

    boxfile.finish().expect("finish bhfst");
    path
}

/// The fixture archive, loaded the way the CLI loads a BHFST.
fn open(path: &Path) -> ThfstBoxSpellerArchive {
    ThfstBoxSpellerArchive::open(path).expect("open archive")
}

const WORD: &str = "kat";

fn suggestions(archive: &dyn SpellerArchive) -> Vec<String> {
    archive
        .speller()
        .suggest(WORD)
        .iter()
        .map(|s| s.value().to_string())
        .collect()
}

fn suggestions_with(archive: &dyn SpellerArchive, config: &SpellerConfig) -> Vec<String> {
    archive
        .speller()
        .suggest_with_config(WORD, config)
        .iter()
        .map(|s| s.value().to_string())
        .collect()
}

/// The fixture speller has to offer more than one correction for the n-best
/// cut below to prove anything.
#[test]
fn the_fixture_offers_several_corrections_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_bhfst(dir.path(), &meta_json(""));
    let archive = open(&path);

    assert!(suggestions(&archive).len() > 1);
}

#[test]
fn an_archive_without_a_bundled_config_is_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_bhfst(dir.path(), &meta_json(""));
    let archive = open(&path);

    assert!(archive.bundled_config().is_none());
    assert_eq!(
        suggestions(&archive),
        suggestions_with(&archive, &SpellerConfig::default())
    );
}

#[test]
fn a_bundled_config_replaces_the_built_in_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_bhfst(
        dir.path(),
        &meta_json(r#", "spellerConfig": { "n-best": 1 }"#),
    );
    let archive = open(&path);

    let bundled = archive.bundled_config().expect("bundled config");
    assert_eq!(bundled.n_best, Some(1));
    // Whole-struct, not a field merge: everything the config did not name is
    // the built-in default, not a zero.
    assert_eq!(bundled.max_weight, SpellerConfig::default().max_weight);

    // And it is what a call naming no config of its own actually runs with.
    assert_eq!(suggestions(&archive).len(), 1);
}

#[test]
fn a_caller_supplied_config_beats_the_bundled_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_bhfst(
        dir.path(),
        &meta_json(r#", "spellerConfig": { "n-best": 1 }"#),
    );
    let archive = open(&path);

    let caller = SpellerConfig {
        n_best: Some(10),
        ..SpellerConfig::default()
    };
    assert!(suggestions_with(&archive, &caller).len() > 1);
}

#[test]
fn a_malformed_bundled_config_falls_back_to_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Well-formed JSON, nonsense config: the kind of thing a typo produces.
    let path = build_bhfst(
        dir.path(),
        &meta_json(r#", "spellerConfig": { "n-best": "one" }"#),
    );
    let archive = open(&path);

    assert!(archive.bundled_config().is_none());
    assert_eq!(
        suggestions(&archive),
        suggestions_with(&archive, &SpellerConfig::default())
    );
}
