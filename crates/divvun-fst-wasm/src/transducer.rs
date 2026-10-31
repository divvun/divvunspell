//! Transducers loaded from bytes, and lookup on them.

use std::io::Read as _;
use std::path::Path;

use divvun_fst::transducer::dhfst::{DhfstTransducer, DhfstType, acceptor::DhfstAcceptor};
use divvun_fst::transducer::hfst::HfstTransducer;
use divvun_fst::transducer::thfst::BytesThfstTransducer;
use divvun_fst::transducer::{Lookup, LookupResult, TransducerFormat, TransducerLoader as _};
use divvun_fst::vfs::{Filesystem as _, bytes::Filesystem};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::GlueError;

/// The name a transducer that is the whole of the bytes goes by, in error
/// messages and in the file system it is read from.
const TRANSDUCER_NAME: &str = "transducer";

/// The first bytes of a box archive.
const BOX_MAGIC: &[u8] = b"\xffBOX";

/// The member name the root of a box goes by, for a THFST directory whose
/// files are at the root.
const ROOT: &str = ".";

enum Machine {
    Hfst(HfstTransducer),
    Thfst(BytesThfstTransducer),
    DhfstAcceptor(DhfstAcceptor),
    DhfstErrorModel(DhfstTransducer),
}

/// A transducer the package read. The wrapper's `Transducer` class in
/// `js/fst/transducer.ts` owns one. It keeps its lookup tables from one word
/// to the next.
#[wasm_bindgen]
pub struct Transducer {
    machine: Machine,
    lookup: Lookup,
}

/// The transducer in `bytes`: HFST optimized lookup, a DHFST acceptor or
/// error model, or a box archive holding one, a THFST directory or a single
/// file, as a BHFST holds its members. `member` names the one to read from a
/// box that holds several; the root of a box is `.`.
#[wasm_bindgen(js_name = readTransducer)]
pub fn read_transducer(bytes: Vec<u8>, member: Option<String>) -> Result<Transducer, GlueError> {
    if bytes.is_empty() {
        return Err(GlueError::new(
            "invalid-format",
            "a transducer cannot be empty",
        ));
    }
    let machine = if bytes.starts_with(BOX_MAGIC) {
        let fs = Filesystem::from_box(bytes)
            .map_err(|error| GlueError::new("invalid-format", format!("a box archive: {error}")))?;
        let member = match member {
            Some(member) => member,
            None => only_member(&fs)?,
        };
        read_member(&fs, &member)?
    } else {
        if let Some(member) = member {
            return Err(GlueError::new(
                "invalid-argument",
                format!("only a box archive has members, so there is no member {member:?} to read"),
            ));
        }
        let mut fs = Filesystem::new();
        fs.insert(TRANSDUCER_NAME, bytes);
        read_file(&fs, TRANSDUCER_NAME)?
    };
    Ok(Transducer {
        machine,
        lookup: Lookup::new(),
    })
}

/// The first eight bytes of the file at `path`, enough for any header.
fn head(fs: &Filesystem, path: &str) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(8);
    fs.open_file(path)?.take(8).read_to_end(&mut head)?;
    Ok(head)
}

/// The format a file's first bytes declare, or `None` when they declare none
/// this library reads.
fn file_format(fs: &Filesystem, path: &str) -> Option<TransducerFormat> {
    TransducerFormat::detect(&head(fs, path).ok()?, Path::new(path)).ok()
}

/// The transducer in the file at `path`, in the format its first bytes
/// declare: a member's name never decides how it is read.
fn read_file(fs: &Filesystem, path: &str) -> Result<Machine, GlueError> {
    let head = head(fs, path)
        .map_err(|error| GlueError::new("invalid-format", format!("{path}: {error}")))?;
    Ok(match TransducerFormat::detect(&head, Path::new(path))? {
        TransducerFormat::Hfst => Machine::Hfst(HfstTransducer::from_path(fs, path)?),
        TransducerFormat::Dhfst {
            kind: DhfstType::Acceptor,
            ..
        } => Machine::DhfstAcceptor(DhfstAcceptor::from_path(fs, path)?),
        TransducerFormat::Dhfst {
            kind: DhfstType::ErrorModel,
            ..
        } => Machine::DhfstErrorModel(DhfstTransducer::from_path(fs, path)?),
    })
}

/// The THFST directories and the transducer files a box holds, by member
/// name, sorted. A directory is a THFST one when it holds an `alphabet`; the
/// files inside it are its tables, not transducers of their own.
fn members(fs: &Filesystem) -> Vec<String> {
    let mut directories: Vec<String> = fs
        .paths()
        .filter_map(|path| match path.rsplit_once('/') {
            Some((directory, "alphabet")) => Some(directory.to_owned()),
            None if path == "alphabet" => Some(ROOT.to_owned()),
            _ => None,
        })
        .collect();
    let inside = |path: &str| {
        directories.iter().any(|directory| {
            directory == ROOT && !path.contains('/')
                || path
                    .strip_prefix(directory.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let files: Vec<String> = fs
        .paths()
        .filter(|path| !inside(path) && file_format(fs, path).is_some())
        .map(str::to_owned)
        .collect();
    directories.extend(files);
    directories.sort();
    directories
}

/// The name of the one transducer a box holds.
fn only_member(fs: &Filesystem) -> Result<String, GlueError> {
    let mut found = members(fs);
    match found.len() {
        0 => Err(GlueError::new(
            "unsupported-format",
            "the box archive holds no transducer: no THFST directory and no HFST or DHFST file",
        )),
        1 => Ok(found.remove(0)),
        count => Err(GlueError::new(
            "invalid-argument",
            format!(
                "the box archive holds {count} transducers ({}); name the one to read with the member option",
                found.join(", ")
            ),
        )),
    }
}

/// The transducer `member` names in a box: a THFST directory, the root
/// included, or a single file.
fn read_member(fs: &Filesystem, member: &str) -> Result<Machine, GlueError> {
    let directory = Path::new(member);
    if fs.open_file(directory.join("alphabet")).is_ok() {
        return Ok(Machine::Thfst(BytesThfstTransducer::from_path(
            fs, directory,
        )?));
    }
    if fs.open_file(member).is_ok() {
        return read_file(fs, member);
    }
    let found = members(fs);
    let holds = match found.is_empty() {
        true => "no transducer".to_owned(),
        false => found.join(", "),
    };
    Err(GlueError::new(
        "invalid-argument",
        format!("the box archive has no member {member:?}; it holds {holds}"),
    ))
}

/// A result as the wrapper returns it, the weight carried exactly.
#[derive(Debug, Serialize)]
struct LookupResultOutput<'a> {
    output: &'a str,
    symbols: Vec<&'a str>,
    weight: f64,
}

fn results_json(results: &[LookupResult]) -> String {
    let out: Vec<LookupResultOutput<'_>> = results
        .iter()
        .map(|result| LookupResultOutput {
            output: &result.output,
            symbols: result
                .symbols
                .iter()
                .map(|symbol| symbol.as_str())
                .collect(),
            weight: f64::from(result.weight.0),
        })
        .collect();
    serde_json::to_string(&out).expect("results of strings and finite numbers serialise")
}

#[wasm_bindgen]
impl Transducer {
    /// The format it was read in, one of the names `js/fst/transducer.ts`
    /// lists.
    pub fn format(&self) -> String {
        match self.machine {
            Machine::Hfst(_) => "optimized-lookup",
            Machine::Thfst(_) => "thfst",
            Machine::DhfstAcceptor(_) => "dhfst-acceptor",
            Machine::DhfstErrorModel(_) => "dhfst-error-model",
        }
        .to_owned()
    }

    /// The results of looking `word` up, cheapest first, as JSON text: the
    /// first `limit` of them, or all of them without one.
    pub fn lookup(&mut self, word: &str, limit: Option<u32>) -> String {
        let mut results = match &self.machine {
            Machine::Hfst(t) => self.lookup.lookup(t, word),
            Machine::Thfst(t) => self.lookup.lookup(t, word),
            Machine::DhfstAcceptor(t) => self.lookup.lookup(t, word),
            Machine::DhfstErrorModel(t) => self.lookup.lookup(t, word),
        };
        if let Some(limit) = limit {
            results.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        results_json(&results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap};
    use divvun_fst::transducer;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures")).join(name)
    }

    /// A box archive holding the THFST fixture `source` under each of
    /// `names`, `.` for its root, and `extra` files beside them.
    fn boxed(source: &str, names: &[&str], extra: &[(&str, &[u8])]) -> Vec<u8> {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("t.box");
        let mut writer =
            box_format::sync::BoxWriter::create_with_alignment(&path, 8).expect("a box");
        let stored = CompressionConfig::new(Compression::Stored);
        for name in names {
            if *name != ROOT {
                writer
                    .mkdir(BoxPath::new(name).expect("a box path"), BoxHashMap::new())
                    .expect("mkdir");
            }
            for component in ["alphabet", "index", "transition"] {
                let bytes = std::fs::read(fixture(source).join(component)).expect("a fixture");
                writer
                    .insert(
                        &stored,
                        BoxPath::new(Path::new(name).join(component)).expect("a box path"),
                        std::io::Cursor::new(bytes),
                        BoxHashMap::new(),
                    )
                    .expect("insert");
            }
        }
        for (name, bytes) in extra {
            writer
                .insert(
                    &stored,
                    BoxPath::new(name).expect("a box path"),
                    std::io::Cursor::new(bytes.to_vec()),
                    BoxHashMap::new(),
                )
                .expect("insert");
        }
        writer.finish().expect("finish");
        std::fs::read(&path).expect("the box")
    }

    fn native(source: &str, word: &str) -> String {
        let thfst = divvun_fst::transducer::thfst::MmapThfstTransducer::from_path(
            &divvun_fst::vfs::Fs,
            fixture(source),
        )
        .expect("the fixture loads");
        results_json(&transducer::lookup(&thfst, word))
    }

    #[test]
    fn a_thfst_directory_in_a_box_looks_up_as_natively() {
        for names in [&[ROOT][..], &["lexicon.thfst"][..]] {
            let mut transducer =
                read_transducer(boxed("lexicon.thfst", names, &[]), None).expect("reads");
            assert_eq!(transducer.format(), "thfst");
            for word in ["cat", "cart", "kat", ""] {
                assert_eq!(
                    transducer.lookup(word, None),
                    native("lexicon.thfst", word),
                    "{word}"
                );
            }
        }
    }

    #[test]
    fn a_box_of_several_transducers_needs_a_member() {
        let bytes = boxed(
            "lexicon.thfst",
            &["acceptor.default.thfst", "errmodel.default.thfst"],
            &[("meta.json", b"{}")],
        );
        let error = read_transducer(bytes.clone(), None)
            .err()
            .expect("ambiguous");
        assert_eq!(error.code(), "invalid-argument");
        assert!(
            error
                .message()
                .contains("2 transducers (acceptor.default.thfst, errmodel.default.thfst)"),
            "{}",
            error.message()
        );
        let mut chosen = read_transducer(bytes.clone(), Some("errmodel.default.thfst".into()))
            .expect("the named member");
        assert_eq!(chosen.lookup("cat", None), native("lexicon.thfst", "cat"));
        let missing = read_transducer(bytes, Some("lexicon".into()))
            .err()
            .expect("no such member");
        assert_eq!(missing.code(), "invalid-argument");
        assert!(missing.message().contains("no member \"lexicon\""));
    }

    #[test]
    fn a_box_with_no_transducer_and_bytes_with_no_format_are_unsupported() {
        let empty_box = boxed("lexicon.thfst", &[], &[("meta.json", b"{}")]);
        let error = read_transducer(empty_box, None)
            .err()
            .expect("nothing to read");
        assert_eq!(error.code(), "unsupported-format");
        let error = read_transducer(b"PK\x03\x04 a zip".to_vec(), None)
            .err()
            .expect("not a transducer");
        assert_eq!(error.code(), "unsupported-format");
        let error = read_transducer(b"HFST\0".to_vec(), Some("x".into()))
            .err()
            .expect("no members");
        assert_eq!(error.code(), "invalid-argument");
    }

    #[test]
    fn a_limit_keeps_the_cheapest_results() {
        let mut transducer =
            read_transducer(boxed("lexicon.thfst", &[ROOT], &[]), None).expect("reads");
        let all: Vec<serde_json::Value> =
            serde_json::from_str(&transducer.lookup("cat", None)).expect("JSON");
        let one: Vec<serde_json::Value> =
            serde_json::from_str(&transducer.lookup("cat", Some(1))).expect("JSON");
        assert_eq!(one.len(), all.len().min(1));
        assert_eq!(one.first(), all.first());
    }
}
