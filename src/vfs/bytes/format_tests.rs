//! Every format loads from bytes. These tests hand the library bytes and
//! never a path, so they also run without the `fs` feature, which is the
//! configuration a `wasm32-unknown-unknown` build has.

use std::path::Path;
use std::sync::Arc;

use super::{Bytes, Filesystem, v0_archive};
use crate::archive::SpellerArchive;
use crate::archive::boxf::{
    DHFST_ACCEPTOR_MEMBER, DHFST_ERRMODEL_MEMBER, THFST_ACCEPTOR_MEMBER, THFST_ERRMODEL_MEMBER,
    ThfstBytesBoxSpellerArchive,
};
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::dhfst::DhfstTransducer;
use crate::transducer::dhfst::acceptor::DhfstAcceptor;
use crate::transducer::dhfst::acceptor_writer::{AcceptorOptions, write_acceptor};
use crate::transducer::dhfst::writer::{SourceModel, WriteOptions, write};
use crate::transducer::hfst::{HfstTransducer, test_support};
use crate::transducer::thfst::BytesThfstTransducer;
use crate::transducer::{ErrorModel, Transducer, TransducerError, TransducerLoader};

const WORDS: [&str; 5] = ["kat", "cet", "car", "cäät", "katt"];

/// A BHFST's `meta.json`, bundling a config of `n-best` 3.
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
    },
    "spellerConfig": { "n-best": 3 }
}"#;

fn fixture(name: &str, component: &str) -> Vec<u8> {
    let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
        .join(name)
        .join(component);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The THFST fixtures `lexicon.thfst` and `mutator.thfst` as files in
/// memory, under the member names a BHFST archive gives them.
fn thfst_files() -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for (name, member) in [
        ("lexicon.thfst", THFST_ACCEPTOR_MEMBER),
        ("mutator.thfst", THFST_ERRMODEL_MEMBER),
    ] {
        for component in ["alphabet", "index", "transition"] {
            files.push((format!("{member}/{component}"), fixture(name, component)));
        }
    }
    files
}

fn thfst_fs() -> Filesystem {
    let mut fs = Filesystem::new();
    for (name, bytes) in thfst_files() {
        fs.insert(name, bytes);
    }
    fs
}

fn thfst(fs: &Filesystem, member: &str) -> BytesThfstTransducer {
    BytesThfstTransducer::from_path(fs, member).expect("the fixture loads")
}

fn suggestions(speller: Arc<dyn Speller + Send + Sync>) -> Vec<Vec<(String, u32)>> {
    let mut config = SpellerConfig::default();
    config.n_best = None;
    WORDS
        .iter()
        .map(|word| {
            speller
                .clone()
                .suggest_with_config(word, &config)
                .into_iter()
                .map(|s| (s.value.to_string(), s.weight.0.to_bits()))
                .collect()
        })
        .collect()
}

fn thfst_suggestions() -> Vec<Vec<(String, u32)>> {
    let fs = thfst_fs();
    let speller = HfstSpeller::new(
        thfst(&fs, THFST_ERRMODEL_MEMBER),
        thfst(&fs, THFST_ACCEPTOR_MEMBER),
    );
    let found = suggestions(speller);
    assert!(found[1].iter().any(|(word, _)| word == "cat"), "{found:?}");
    found
}

fn names<T: Transducer>(t: &T) -> Vec<String> {
    let mut names: Vec<String> = t
        .alphabet()
        .key_table()
        .iter()
        .map(|k| k.to_string())
        .collect();
    names[0] = "@_EPSILON_SYMBOL_@".to_string();
    names
}

fn dhfst_errmodel() -> Vec<u8> {
    let mutator = thfst(&thfst_fs(), THFST_ERRMODEL_MEMBER);
    let model = SourceModel::from_transducer(&mutator, names(&mutator)).expect("it reads");
    let options = WriteOptions {
        threads: 1,
        ..WriteOptions::default()
    };
    write(&model, &options).expect("it writes").bytes
}

fn dhfst_acceptor() -> Vec<u8> {
    let lexicon = thfst(&thfst_fs(), THFST_ACCEPTOR_MEMBER);
    let options = AcceptorOptions {
        threads: 1,
        ..AcceptorOptions::default()
    };
    write_acceptor(&lexicon, &names(&lexicon), &options)
        .expect("it writes")
        .bytes
}

#[test]
fn thfst_loads_from_files_in_memory() {
    thfst_suggestions();
}

#[test]
fn optimized_lookup_loads_from_bytes_and_from_files_in_memory() {
    let from_bytes = HfstSpeller::new(
        HfstTransducer::from_bytes(&test_support::errmodel(), "errmodel.hfst").expect("loads"),
        HfstTransducer::from_bytes(&test_support::lexicon(), "lexicon.hfst").expect("loads"),
    );
    let mut fs = Filesystem::new();
    fs.insert("errmodel.hfst", test_support::errmodel());
    fs.insert("lexicon.hfst", test_support::lexicon());
    let from_fs = HfstSpeller::new(
        <HfstTransducer as TransducerLoader<_>>::from_path(&fs, "errmodel.hfst").expect("loads"),
        <HfstTransducer as TransducerLoader<_>>::from_path(&fs, "lexicon.hfst").expect("loads"),
    );
    let want = suggestions(from_bytes.clone());
    assert!(want[1].iter().any(|(word, _)| word == "cat"), "{want:?}");
    assert_eq!(suggestions(from_fs), want);
    assert!(from_bytes.is_correct("cart"));
    assert!(matches!(
        HfstTransducer::from_bytes(&[], "empty.hfst"),
        Err(TransducerError::CorruptHeader { offset: 0, .. })
    ));
}

#[test]
fn an_error_model_loads_from_bytes_in_either_format() {
    let lexicon = || thfst(&thfst_fs(), THFST_ACCEPTOR_MEMBER);
    let errmodel = ErrorModel::from_bytes(&dhfst_errmodel(), "errmodel.dhfst").expect("loads");
    let ErrorModel::Dhfst(errmodel) = errmodel else {
        panic!("a DHFST file loads as DHFST");
    };
    assert_eq!(
        suggestions(HfstSpeller::new(errmodel, lexicon())),
        thfst_suggestions()
    );
    let ol = ErrorModel::from_bytes(&test_support::errmodel(), "errmodel.hfst").expect("loads");
    assert!(matches!(ol, ErrorModel::Hfst(_)));
}

#[test]
fn dhfst_files_load_from_bytes() {
    let fs = thfst_fs();
    let errmodel = DhfstTransducer::from_bytes(&dhfst_errmodel(), "errmodel.dhfst").expect("loads");
    let acceptor = DhfstAcceptor::from_bytes(&dhfst_acceptor(), "acceptor.dhfst").expect("loads");
    assert_eq!(
        suggestions(HfstSpeller::new(errmodel, acceptor)),
        thfst_suggestions()
    );

    let mut dhfst_fs = Filesystem::new();
    dhfst_fs.insert("a.dhfst", dhfst_acceptor());
    let acceptor =
        <DhfstAcceptor as TransducerLoader<_>>::from_path(&dhfst_fs, "a.dhfst").expect("loads");
    assert_eq!(
        suggestions(HfstSpeller::new(
            thfst(&fs, THFST_ERRMODEL_MEMBER),
            acceptor
        )),
        thfst_suggestions()
    );
}

#[test]
fn a_bhfst_loads_from_bytes_whatever_its_members() {
    let want = thfst_suggestions();
    let thfst = thfst_files();
    let errmodel = dhfst_errmodel();
    let acceptor = dhfst_acceptor();
    let acceptor_files = |dhfst: bool| -> Vec<(&str, &[u8], u8)> {
        if dhfst {
            vec![(DHFST_ACCEPTOR_MEMBER, acceptor.as_slice(), 0)]
        } else {
            thfst
                .iter()
                .filter(|(name, _)| name.starts_with(THFST_ACCEPTOR_MEMBER))
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice(), 0))
                .collect()
        }
    };
    let errmodel_files = |dhfst: bool| -> Vec<(&str, &[u8], u8)> {
        if dhfst {
            vec![(DHFST_ERRMODEL_MEMBER, errmodel.as_slice(), 0)]
        } else {
            thfst
                .iter()
                .filter(|(name, _)| name.starts_with(THFST_ERRMODEL_MEMBER))
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice(), 0))
                .collect()
        }
    };
    for dhfst_acceptor in [false, true] {
        for dhfst_errmodel in [false, true] {
            let mut files = acceptor_files(dhfst_acceptor);
            files.extend(errmodel_files(dhfst_errmodel));
            files.push(("meta.json", META_JSON.as_bytes(), 0));
            let archive = ThfstBytesBoxSpellerArchive::from_bytes(v0_archive(&files), "t.bhfst")
                .expect("reads");
            assert_eq!(
                archive.hfst_speller().is_some(),
                !dhfst_acceptor && !dhfst_errmodel
            );
            assert_eq!(archive.bundled_config().and_then(|c| c.n_best), Some(3));
            assert_eq!(
                suggestions(archive.speller()),
                want,
                "DHFST acceptor {dhfst_acceptor}, DHFST error model {dhfst_errmodel}"
            );
        }
    }
}

#[test]
fn bytes_are_shared_not_copied_by_the_in_memory_files() {
    let archive = Bytes::from(v0_archive(&[("a/b", b"payload", 0)]));
    let fs = Filesystem::from_box(archive.clone()).expect("reads");
    let file = crate::vfs::Filesystem::open_file(&fs, "a/b").expect("opens");
    let contents = crate::vfs::File::contents(&file).expect("contents");
    assert_eq!(&contents[..], b"payload");
    let offset = (contents.as_ptr() as usize).checked_sub(archive.as_ptr() as usize);
    assert!(
        offset.is_some_and(|offset| offset < archive.len()),
        "the file is a window onto the archive"
    );
}
