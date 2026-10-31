//! The fixtures of the `@divvun/fst` JavaScript suite, and what native
//! divvun-fst gives for them. `js/test/fixtures.mjs` runs it at test-build
//! time:
//!
//! ```text
//! js_fixtures derive <sources.json> <out>
//! js_fixtures expect <out> <request.json> <expected.json>
//! ```
//!
//! `derive` copies the real spellers and transducers `sources.json` names
//! (fixture name to path) into `out`, and writes beside them the fixtures made
//! from them and from `tests/fixtures`: a box version 1 copy of a version 0
//! BHFST, a stored copy of a deflated ZHFST, its members, BHFSTs with and
//! without metadata and with DHFST members, DHFST files, and THFST
//! directories in box archives.
//!
//! `expect` loads every fixture the request names from its file, through the
//! memory-mapped readers a native divvun-fst user has, never from bytes, and
//! writes each result beside the call that gave it, in the shapes the
//! package returns. Configs come in divvunspell's own kebab-case JSON, read
//! by divvunspell's own deserialiser, so the package's camelCase mirror is
//! checked against the struct rather than against itself.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter};
use divvun_fst::archive::boxf::ThfstBoxSpellerArchive;
use divvun_fst::archive::{self, SpellerArchive, meta::SpellerMetadata, meta::SpellerTitle};
use divvun_fst::speller::{SpellerConfig, suggestion::Suggestion};
use divvun_fst::transducer::convert::ConvertFile as _;
use divvun_fst::transducer::dhfst::acceptor_writer::{AcceptorOptions, write_acceptor};
use divvun_fst::transducer::dhfst::writer::{SourceModel, WriteOptions, write};
use divvun_fst::transducer::dhfst::{DhfstTransducer, DhfstType, acceptor::DhfstAcceptor};
use divvun_fst::transducer::hfst::HfstTransducer;
use divvun_fst::transducer::thfst::MmapThfstTransducer;
use divvun_fst::transducer::{
    LookupResult, Transducer, TransducerFormat, TransducerLoader, lookup,
};
use divvun_fst::vfs::{self, Filesystem as _};
use serde::Deserialize;
use serde_json::{Map, Value, json};

type Failure = Box<dyn std::error::Error>;

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["derive", sources, out] => derive(Path::new(sources), Path::new(out)),
        ["expect", out, request, expected] => {
            expect(Path::new(out), Path::new(request), Path::new(expected))
        }
        _ => Err("usage: js_fixtures derive <sources.json> <out> | expect <out> <request.json> <expected.json>".into()),
    }
}

/// The repository's own THFST fixtures.
fn repository_fixture(name: &str) -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures")).join(name)
}

fn stored() -> CompressionConfig {
    CompressionConfig::new(Compression::Stored)
}

fn box_path(path: impl AsRef<Path>) -> Result<BoxPath<'static>, Failure> {
    BoxPath::new(path.as_ref())
        .map_err(|error| format!("{}: {error}", path.as_ref().display()).into())
}

/// A box version 1 archive at `path`, with `members` (box path, bytes) in it
/// stored, their directories made first.
fn write_box(path: &Path, members: &[(String, Vec<u8>)]) -> Result<(), Failure> {
    let mut writer = BoxWriter::create_with_alignment(path, 8)?;
    for (name, bytes) in members {
        if let Some(parent) = Path::new(name).parent()
            && parent != Path::new("")
        {
            writer.mkdir_all(box_path(parent)?, BoxHashMap::new())?;
        }
        writer.insert(
            &stored(),
            box_path(name)?,
            std::io::Cursor::new(bytes.clone()),
            BoxHashMap::new(),
        )?;
    }
    writer.finish()?;
    Ok(())
}

/// The three files of the THFST directory `dir`, under `name` in a box.
fn thfst_members(dir: &Path, name: &str) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    ["alphabet", "index", "transition"]
        .iter()
        .map(|component| {
            let bytes = std::fs::read(dir.join(component))?;
            let path = match name {
                "" => (*component).to_owned(),
                name => format!("{name}/{component}"),
            };
            Ok((path, bytes))
        })
        .collect()
}

fn symbol_names<T: Transducer>(transducer: &T) -> Vec<String> {
    let mut names: Vec<String> = transducer
        .alphabet()
        .key_table()
        .iter()
        .map(|key| key.to_string())
        .collect();
    if let Some(first) = names.first_mut() {
        "@_EPSILON_SYMBOL_@".clone_into(first);
    }
    names
}

/// The repository's fixture error model as a DHFST error model, and its
/// lexicon as a DHFST acceptor.
fn small_dhfst() -> Result<(Vec<u8>, Vec<u8>), Failure> {
    let mutator = MmapThfstTransducer::from_path(&vfs::Fs, repository_fixture("mutator.thfst"))?;
    let model = SourceModel::from_transducer(&mutator, symbol_names(&mutator))?;
    let errmodel = write(
        &model,
        &WriteOptions {
            threads: 1,
            ..WriteOptions::default()
        },
    )?
    .bytes;
    let lexicon = MmapThfstTransducer::from_path(&vfs::Fs, repository_fixture("lexicon.thfst"))?;
    let acceptor = write_acceptor(
        &lexicon,
        &symbol_names(&lexicon),
        &AcceptorOptions {
            threads: 1,
            ..AcceptorOptions::default()
        },
    )?
    .bytes;
    Ok((errmodel, acceptor))
}

/// The `meta.json` of the small spellers, with `config` bundled when given.
fn small_meta(config: Option<Value>) -> Vec<u8> {
    let mut meta = json!({
        "info": {
            "locale": "zxx",
            "title": [{"lang": null, "$value": "Test speller"}, {"lang": "se", "$value": "Geahččalanmálle"}],
            "description": "The repository's test lexicon and error model.",
            "producer": "divvunspell"
        },
        "acceptor": {
            "type": "general",
            "id": "acceptor.default.thfst",
            "title": [{"lang": null, "$value": "cat, car, cart, care"}],
            "description": "Four words.",
            "continuation": null
        },
        "errmodel": {
            "id": "errmodel.default.thfst",
            "title": [{"lang": null, "$value": "k:c e:a d:t"}],
            "description": "Three substitutions."
        }
    });
    if let Some(config) = config {
        meta["spellerConfig"] = config;
    }
    serde_json::to_vec_pretty(&meta).expect("JSON serialises")
}

/// Every file of the zip archive `path`, by name, decompressed.
fn zip_members(path: &Path) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let mut members = Vec::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index)?;
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes)?;
        members.push((member.name().to_owned(), bytes));
    }
    Ok(members)
}

/// Every file of the box archive `path`, of either version, by path.
fn box_members(path: &Path, scratch: &Path) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let reader = box_format::sync::BoxReader::open(path)
        .map_err(|error| format!("{}: {error:?}", path.display()))?;
    let dir = scratch.join("box");
    reader
        .extract_all(&dir)
        .map_err(|error| format!("{}: {error:?}", path.display()))?;
    let mut members = Vec::new();
    let mut pending = vec![dir.clone()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                let relative = entry
                    .path()
                    .strip_prefix(&dir)?
                    .to_string_lossy()
                    .replace('\\', "/");
                members.push((relative, std::fs::read(entry.path())?));
            }
        }
    }
    members.sort();
    Ok(members)
}

/// The directory fixtures are written to, and what has been written there.
struct Written<'a> {
    dir: &'a Path,
    names: Vec<String>,
}

impl Written<'_> {
    fn file(&mut self, name: &str, bytes: &[u8]) -> Result<(), Failure> {
        std::fs::write(self.dir.join(name), bytes)?;
        self.names.push(name.to_owned());
        Ok(())
    }

    fn boxed(&mut self, name: &str, members: &[(String, Vec<u8>)]) -> Result<(), Failure> {
        write_box(&self.dir.join(name), members)?;
        self.names.push(name.to_owned());
        Ok(())
    }
}

fn derive(sources: &Path, out: &Path) -> Result<(), Failure> {
    let sources: BTreeMap<String, PathBuf> = serde_json::from_slice(&std::fs::read(sources)?)?;
    std::fs::create_dir_all(out)?;
    let scratch = tempfile::tempdir()?;
    let mut written = Written {
        dir: out,
        names: Vec::new(),
    };

    for (name, path) in &sources {
        written.file(name, &std::fs::read(path)?)?;
    }

    let lexicon = thfst_members(
        &repository_fixture("lexicon.thfst"),
        "acceptor.default.thfst",
    )?;
    let mutator = thfst_members(
        &repository_fixture("mutator.thfst"),
        "errmodel.default.thfst",
    )?;
    let (dhfst_errmodel, dhfst_acceptor) = small_dhfst()?;
    let config = json!({
        "n-best": 3,
        "max-weight": 30,
        "reweight": {"start-penalty": 1, "end-penalty": 2, "mid-penalty": 0.5},
        "recase": true
    });
    written.boxed(
        "small.bhfst",
        &[
            lexicon.clone(),
            mutator.clone(),
            vec![("meta.json".into(), small_meta(Some(config)))],
        ]
        .concat(),
    )?;
    written.boxed("small-bare.bhfst", &[lexicon.clone(), mutator].concat())?;
    written.boxed(
        "small-dhfst.bhfst",
        &[
            ("acceptor.default.dhfst".into(), dhfst_acceptor.clone()),
            ("errmodel.default.dhfst".into(), dhfst_errmodel.clone()),
            ("meta.json".into(), small_meta(None)),
        ],
    )?;
    written.boxed(
        "lexicon.box",
        &thfst_members(&repository_fixture("lexicon.thfst"), "")?,
    )?;
    written.boxed(
        "two-lexicons.box",
        &[
            lexicon,
            thfst_members(&repository_fixture("lexicon.thfst"), "second.thfst")?,
        ]
        .concat(),
    )?;
    written.file("small-acceptor.dhfst", &dhfst_acceptor)?;
    written.file("small-errmodel.dhfst", &dhfst_errmodel)?;

    if let Some(sma) = sources.get("sma.bhfst") {
        written.boxed("sma.v1.bhfst", &box_members(sma, scratch.path())?)?;
    }

    if let Some(ga) = sources.get("ga.zhfst") {
        let members = zip_members(ga)?;
        let mut zip = zip::ZipWriter::new(std::fs::File::create(out.join("ga.stored.zhfst"))?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in &members {
            zip.start_file(name.as_str(), options)?;
            std::io::Write::write_all(&mut zip, bytes)?;
        }
        zip.finish()?;
        written.names.push("ga.stored.zhfst".into());
        for (name, bytes) in &members {
            if name.ends_with(".hfst") {
                written.file(&format!("ga.{name}"), bytes)?;
            }
        }
    }

    if let Some(analyser) = sources.get("sma-analyser.hfstol") {
        let transducer = HfstTransducer::from_path(&vfs::Fs, analyser)?;
        let source = scratch.path().join("analyser.hfstol");
        std::fs::copy(analyser, &source)?;
        MmapThfstTransducer::convert_file(&transducer, &source)?;
        let mut members = thfst_members(&scratch.path().join("analyser.thfst"), "analyser.thfst")?;
        members.push(("README".into(), b"An analyser, as THFST.\n".to_vec()));
        written.boxed("sma-analyser.box", &members)?;
    }

    for name in written.names {
        println!("{name}");
    }
    Ok(())
}

/// What `expect` is asked.
#[derive(Deserialize)]
struct Request {
    /// Configs by name, in divvunspell's kebab-case JSON, `verbose` beside
    /// the fields its deserialiser reads.
    configs: BTreeMap<String, Value>,
    spellers: Vec<SpellerRequest>,
    lookups: Vec<LookupRequest>,
}

/// The config name that stands for passing none.
const NO_CONFIG: &str = "none";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpellerRequest {
    file: String,
    words: Vec<String>,
    /// The configs every word is checked and corrected under.
    configs: Vec<String>,
    /// The configs every word is analysed under.
    analyse: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LookupRequest {
    file: String,
    /// The member the package is asked for, or none to let it find the one.
    member: Option<String>,
    /// The member read natively; `""` is a box's root.
    native_member: Option<String>,
    #[serde(default)]
    words: Vec<String>,
    /// Words taken from another transducer's outputs for these.
    words_from: Option<Box<LookupRequest>>,
    limits: Vec<Option<usize>>,
}

/// `config` as divvunspell reads a config file, and `verbose`, which no file
/// sets, from beside it.
fn native_config(config: &Value) -> Result<SpellerConfig, Failure> {
    let mut fields = config.as_object().cloned().ok_or("a config is an object")?;
    let verbose = fields.remove("verbose").and_then(|v| v.as_bool());
    let mut config: SpellerConfig = serde_json::from_value(Value::Object(fields))?;
    if let Some(verbose) = verbose {
        config.verbose = verbose;
    }
    Ok(config)
}

/// `config` as the kebab-case JSON divvunspell writes, with `verbose`.
fn config_json(config: &SpellerConfig) -> Result<Value, Failure> {
    let mut value = serde_json::to_value(config)?;
    value["verbose"] = Value::Bool(config.verbose);
    Ok(value)
}

fn suggestion_json(suggestion: &Suggestion) -> Value {
    let mut out = Map::new();
    out.insert("value".into(), json!(suggestion.value()));
    out.insert("weight".into(), json!(f64::from(suggestion.weight().0)));
    if let Some(completed) = suggestion.completed() {
        out.insert("completed".into(), json!(completed));
    }
    if let Some(details) = suggestion.weight_details() {
        out.insert(
            "weightDetails".into(),
            json!({
                "lexiconWeight": f64::from(details.lexicon_weight.0),
                "mutatorWeight": f64::from(details.mutator_weight.0),
                "reweightStart": f64::from(details.reweight_start),
                "reweightMid": f64::from(details.reweight_mid),
                "reweightEnd": f64::from(details.reweight_end),
            }),
        );
    }
    Value::Object(out)
}

fn suggestions_json(suggestions: &[Suggestion]) -> Value {
    Value::Array(suggestions.iter().map(suggestion_json).collect())
}

fn titles_json(titles: &[SpellerTitle]) -> Value {
    titles
        .iter()
        .map(|title| json!({"lang": title.lang, "value": title.value}))
        .collect()
}

fn metadata_json(metadata: &SpellerMetadata) -> Value {
    let (info, acceptor, errmodel) = (metadata.info(), metadata.acceptor(), metadata.errmodel());
    json!({
        "info": {
            "locale": info.locale(),
            "title": titles_json(info.title()),
            "description": info.description(),
            "producer": info.producer(),
        },
        "acceptor": {
            "type": acceptor.type_(),
            "id": acceptor.id(),
            "title": titles_json(acceptor.title()),
            "description": acceptor.description(),
            "continuation": acceptor.continuation(),
        },
        "errorModel": {
            "id": errmodel.id(),
            "title": titles_json(errmodel.title()),
            "description": errmodel.description(),
        },
    })
}

fn speller_json(
    out: &Path,
    request: &SpellerRequest,
    configs: &BTreeMap<String, Option<SpellerConfig>>,
) -> Result<Value, Failure> {
    let path = out.join(&request.file);
    // BHFSTs are read by the memory-mapped THFST reader rather than the
    // chunked one `archive::open` picks, which indexes past its last chunk
    // on tables as small as the repository's fixtures.
    let archive: Arc<dyn SpellerArchive + Send + Sync> =
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("bhfst") => Arc::new(ThfstBoxSpellerArchive::open(&path)?),
            _ => archive::open(&path)?,
        };
    let speller = archive.speller();
    let config = |name: &str| -> Result<Option<&SpellerConfig>, Failure> {
        configs
            .get(name)
            .map(Option::as_ref)
            .ok_or_else(|| format!("no config called {name}").into())
    };
    let mut results = Vec::new();
    for name in &request.configs {
        let config = config(name)?;
        for word in &request.words {
            let speller = || Arc::clone(&speller);
            let (correct, suggest, analyse) = match config {
                Some(config) => (
                    speller().is_correct_with_config(word, config),
                    speller().suggest_with_config(word, config),
                    speller().analyze_output_with_config(word, config),
                ),
                None => (
                    speller().is_correct(word),
                    speller().suggest(word),
                    speller().analyze_output(word),
                ),
            };
            results.push(json!({
                "word": word,
                "config": name,
                "isCorrect": correct,
                "suggest": suggestions_json(&suggest),
                "analyzeOutput": suggestions_json(&analyse),
            }));
        }
    }
    let mut analyses = Vec::new();
    for name in &request.analyse {
        let config = config(name)?;
        for word in &request.words {
            let found = match config {
                Some(config) => Arc::clone(&speller).analyze_input_with_config(word, config),
                None => Arc::clone(&speller).analyze_input(word),
            };
            analyses.push(json!({
                "word": word,
                "config": name,
                "analyzeInput": suggestions_json(&found),
            }));
        }
    }
    Ok(json!({
        "file": request.file,
        "metadata": archive.metadata().map(metadata_json),
        "bundledConfig": archive.bundled_config().map(config_json).transpose()?,
        "results": results,
        "analyses": analyses,
    }))
}

/// A transducer loaded natively, by the format the package reports for it.
enum Native {
    Hfst(HfstTransducer),
    Thfst(MmapThfstTransducer),
    Acceptor(DhfstAcceptor),
    ErrorModel(DhfstTransducer),
}

impl Native {
    fn format(&self) -> &'static str {
        match self {
            Native::Hfst(_) => "optimized-lookup",
            Native::Thfst(_) => "thfst",
            Native::Acceptor(_) => "dhfst-acceptor",
            Native::ErrorModel(_) => "dhfst-error-model",
        }
    }

    fn lookup(&self, word: &str) -> Vec<LookupResult> {
        match self {
            Native::Hfst(t) => lookup(t, word),
            Native::Thfst(t) => lookup(t, word),
            Native::Acceptor(t) => lookup(t, word),
            Native::ErrorModel(t) => lookup(t, word),
        }
    }

    /// The single file at `path` that `fs` serves, read as its header says.
    fn file<FS, F>(fs: &FS, path: &Path) -> Result<Native, Failure>
    where
        FS: vfs::Filesystem<File = F>,
        F: vfs::File,
        HfstTransducer: TransducerLoader<F>,
        DhfstAcceptor: TransducerLoader<F>,
        DhfstTransducer: TransducerLoader<F>,
    {
        let mut head = Vec::new();
        fs.open_file(path)?.take(8).read_to_end(&mut head)?;
        Ok(match TransducerFormat::detect(&head, path)? {
            TransducerFormat::Hfst => Native::Hfst(HfstTransducer::from_path(fs, path)?),
            TransducerFormat::Dhfst {
                kind: DhfstType::Acceptor,
                ..
            } => Native::Acceptor(DhfstAcceptor::from_path(fs, path)?),
            TransducerFormat::Dhfst {
                kind: DhfstType::ErrorModel,
                ..
            } => Native::ErrorModel(DhfstTransducer::from_path(fs, path)?),
        })
    }

    fn load(out: &Path, request: &LookupRequest) -> Result<Native, Failure> {
        let path = out.join(&request.file);
        let Some(member) = &request.native_member else {
            return Native::file(&vfs::Fs, &path);
        };
        let reader = box_format::sync::BoxReader::open(&path)
            .map_err(|error| format!("{}: {error:?}", path.display()))?;
        let fs = vfs::boxf::Filesystem::new(&reader);
        let member = Path::new(member);
        if fs.open_file(member.join("alphabet")).is_ok() {
            return Ok(Native::Thfst(MmapThfstTransducer::from_path(&fs, member)?));
        }
        Native::file(&fs, member)
    }
}

/// Every output a transducer gives the request's words, in order, each once.
fn outputs(out: &Path, request: &LookupRequest) -> Result<Vec<String>, Failure> {
    let native = Native::load(out, request)?;
    let mut found: Vec<String> = Vec::new();
    for word in &request.words {
        for result in native.lookup(word) {
            if !found.iter().any(|known| *known == result.output) {
                found.push(result.output.to_string());
            }
        }
    }
    Ok(found)
}

fn lookup_json(out: &Path, request: &LookupRequest) -> Result<Vec<Value>, Failure> {
    let native = Native::load(out, request)?;
    let words = match &request.words_from {
        Some(from) => outputs(out, from)?,
        None => request.words.clone(),
    };
    let mut entries = Vec::new();
    for word in &words {
        let all = native.lookup(word);
        for limit in &request.limits {
            let kept = all.iter().take(limit.unwrap_or(usize::MAX));
            let results: Vec<Value> = kept
                .map(|result| {
                    json!({
                        "output": result.output.as_str(),
                        "symbols": result.symbols.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                        "weight": f64::from(result.weight.0),
                    })
                })
                .collect();
            entries.push(json!({
                "file": request.file,
                "member": request.member,
                "format": native.format(),
                "word": word,
                "limit": limit,
                "results": results,
            }));
        }
    }
    Ok(entries)
}

fn expect(out: &Path, request: &Path, expected: &Path) -> Result<(), Failure> {
    let request: Request = serde_json::from_slice(&std::fs::read(request)?)?;
    let mut configs: BTreeMap<String, Option<SpellerConfig>> = BTreeMap::new();
    configs.insert(NO_CONFIG.into(), None);
    for (name, config) in &request.configs {
        configs.insert(name.clone(), Some(native_config(config)?));
    }
    let mut spellers = Vec::new();
    for speller in &request.spellers {
        eprintln!("native: {}", speller.file);
        spellers.push(speller_json(out, speller, &configs)?);
    }
    let mut lookups = Vec::new();
    for entry in &request.lookups {
        eprintln!("native: {} {:?}", entry.file, entry.member);
        lookups.extend(lookup_json(out, entry)?);
    }
    let document = json!({
        "defaultConfig": config_json(&SpellerConfig::default())?,
        "spellers": spellers,
        "lookups": lookups,
    });
    std::fs::write(expected, serde_json::to_vec(&document)?)?;
    Ok(())
}
