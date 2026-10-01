//! Write, check and package error models in the compact DHFST format.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context as _, bail};
use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter};
use clap::Parser;
use divvun_fst::archive::meta::SpellerMetadata;
use divvun_fst::archive::{DHFST_FORMAT, boxf::DHFST_ERRMODEL_MEMBER};
use divvun_fst::transducer::Transducer;
use divvun_fst::transducer::dhfst::{
    self, DefaultKind, DhfstTransducer,
    writer::{
        ContextSpec, EditStageSpec, SourceArc, SourceModel, SourceState, StageSpec, StagesSpec,
        TableSpec, WriteOptions, verify_reader, write,
    },
};
use divvun_fst::transducer::hfst::HfstTransducer;
use divvun_fst::transducer::{TransducerFormat, TransducerLoader, convert::ConvertFile, thfst};
use divvun_fst::types::{SymbolNumber, TransitionTableIndex};
use divvun_fst::vfs::Fs;
use zip::ZipArchive;

#[derive(Debug, Parser)]
#[command(
    name = "dhfst-tools",
    about = "Write, check and package error models in the compact DHFST format."
)]
enum Opts {
    /// Write an error model as DHFST, checking every (state, pair) of the
    /// encoding and every (state, input) of the written file against it
    Write {
        /// the error model: HFST optimized lookup, or DHFST to re-encode
        input: PathBuf,
        /// the DHFST file to write
        output: PathBuf,
        /// longest fallback chain to allow
        #[arg(long, default_value_t = 4, conflicts_with = "unbounded")]
        max_depth: u32,
        /// allow fallback chains of any length
        #[arg(long)]
        unbounded: bool,
        /// worker threads (0: all cores)
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },

    /// Check that a DHFST file answers every (state, input) exactly as the
    /// HFST optimized-lookup model it was written from, as the suggestion
    /// search reads that model
    Check {
        /// the HFST optimized-lookup error model
        source: PathBuf,
        /// the DHFST file
        dhfst: PathBuf,
        /// worker threads (0: all cores)
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },

    /// Describe a DHFST file
    Info {
        /// the DHFST file
        path: PathBuf,
    },

    /// Write a staged DHFST file (version 2): a stored model whose call arcs
    /// call an edit-table stage
    ///
    /// The stored model is the error model rebuilt with the table-shaped
    /// component replaced by one arc "<DHFST_CALL_IN>":"<DHFST_CALL_OUT>".
    /// Other arcs on those symbols (identities hfst's harmonisation adds to
    /// `?` loops) are dropped. The file keeps the reference model's symbol
    /// numbering, with the call symbols after it.
    Stage {
        /// the stored model, HFST optimized lookup
        stored: PathBuf,
        /// the edit table, as JSON from derive_table.py
        table: PathBuf,
        /// the original error model, HFST optimized lookup
        reference: PathBuf,
        /// the DHFST file to write
        output: PathBuf,
        /// longest fallback chain to allow
        #[arg(long, default_value_t = 4)]
        max_depth: u32,
        /// worker threads (0: all cores)
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },

    /// Write every reachable state and arc of an error model, as the search
    /// reads it (stages included), as AT&T text
    Dump {
        /// HFST optimized lookup or DHFST
        input: PathBuf,
        /// the AT&T text file to write
        output: PathBuf,
    },

    /// Build a BHFST archive from a ZHFST archive's acceptor and a DHFST
    /// error model
    Bhfst {
        /// the ZHFST archive whose acceptor, metadata and config to use
        archive: PathBuf,
        /// the DHFST error model
        dhfst: PathBuf,
        /// the BHFST archive to write
        output: PathBuf,
    },
}

/// Symbol names as an HFST optimized-lookup file stores them.
fn hfst_symbol_names(transducer: &HfstTransducer) -> anyhow::Result<Vec<String>> {
    let buf = transducer.buffer();
    let mut at = transducer.header().len();
    let count = transducer.header().symbol_count().0 as usize;
    let mut names = Vec::with_capacity(count);
    for s in 0..count {
        let rest = buf
            .get(at..)
            .with_context(|| format!("alphabet ends before symbol {s}"))?;
        let end = rest
            .iter()
            .position(|b| *b == 0)
            .with_context(|| format!("symbol {s} is not terminated"))?;
        names.push(
            String::from_utf8(rest[..end].to_vec())
                .with_context(|| format!("symbol {s} is not UTF-8"))?,
        );
        at += end + 1;
    }
    Ok(names)
}

/// Read an error model, in either format, as the search sees it.
fn read_source(path: &Path) -> anyhow::Result<SourceModel> {
    let mut header = [0u8; 8];
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open '{}'", path.display()))?;
    let filled = file.read(&mut header)?;
    match TransducerFormat::detect(&header[..filled], path)? {
        TransducerFormat::Hfst => {
            let transducer = HfstTransducer::from_path(&Fs, path)
                .with_context(|| format!("failed to load '{}'", path.display()))?;
            let names = hfst_symbol_names(&transducer)?;
            Ok(SourceModel::from_transducer(&transducer, names)?)
        }
        TransducerFormat::Dhfst { .. } => {
            let transducer = DhfstTransducer::from_path(&Fs, path)
                .with_context(|| format!("failed to load '{}'", path.display()))?;
            let names = transducer.symbol_names().to_vec();
            Ok(SourceModel::from_transducer(&transducer, names)?)
        }
    }
}

fn human(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.2} GiB", n as f64 / (1u64 << 30) as f64)
    } else if n >= 1 << 20 {
        format!("{:.2} MiB", n as f64 / (1u64 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn cmd_write(
    input: &Path,
    output: &Path,
    max_depth: Option<u32>,
    threads: usize,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let source = read_source(input)?;
    let read_secs = started.elapsed().as_secs_f64();
    let input_bytes = std::fs::metadata(input)?.len();
    println!(
        "read {}: {} states, {} arcs, {} symbols, {} duplicate arcs dropped ({:.1} s)",
        input.display(),
        source.states().len(),
        source.arc_count(),
        source.symbols().len(),
        source.duplicate_arcs(),
        read_secs
    );

    let encode_started = Instant::now();
    let written = write(
        &source,
        &WriteOptions {
            max_fallback_depth: max_depth,
            threads,
            source_name: input
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            stages: None,
        },
    )?;
    let encode_secs = encode_started.elapsed().as_secs_f64();
    std::fs::write(output, &written.bytes)
        .with_context(|| format!("failed to write '{}'", output.display()))?;

    let r = &written.report;
    let arcs = r.source_arcs.max(1) as f64;
    let pc = |x: u64| 100.0 * x as f64 / arcs;
    let entries = r.explicit_arcs + r.blockers + r.defaults.iter().sum::<u64>();
    println!(
        "encoding: {} entries ({:.2}% of arcs) = explicit {} + blockers {} + defaults {} (identity {}, substitution {}, deletion {}, insertion {}); classes {}, class pairs {}",
        entries,
        pc(entries),
        r.explicit_arcs,
        r.blockers,
        r.defaults.iter().sum::<u64>(),
        r.defaults[0],
        r.defaults[1],
        r.defaults[2],
        r.defaults[3],
        r.classes,
        r.class_pairs
    );
    println!(
        "fallbacks: {} states; depth bound {}, longest chain {}; states per chain length {:?}",
        r.states_with_fallback,
        max_depth.map_or("none".to_string(), |d| d.to_string()),
        r.max_depth,
        r.depth_histogram
    );
    let b = &r.answered_by;
    println!(
        "source arcs answered by: own explicit {:.2}%, own default identity {:.2}% substitution {:.2}% deletion {:.2}% insertion {:.2}%, inherited explicit {:.2}%, inherited default {:.2}%",
        pc(b[0]),
        pc(b[1]),
        pc(b[2]),
        pc(b[3]),
        pc(b[4]),
        pc(b[5]),
        pc(b[6])
    );
    println!(
        "checked: {} (state, pair) resolutions of the encoding; {} (state, input) queries, {} arcs, read back from the written bytes: all equal to the source",
        r.pairs_checked, r.queries_checked, r.arcs_checked
    );
    println!(
        "wrote {}: {} bytes ({}), {:.2}% of the input's {} ({:.1} s)",
        output.display(),
        written.bytes.len(),
        human(written.bytes.len() as u64),
        100.0 * written.bytes.len() as f64 / input_bytes.max(1) as f64,
        human(input_bytes),
        encode_secs
    );
    Ok(())
}

fn cmd_check(source: &Path, dhfst_path: &Path, threads: usize) -> anyhow::Result<()> {
    let started = Instant::now();
    let transducer = HfstTransducer::from_path(&Fs, source)
        .with_context(|| format!("failed to load '{}'", source.display()))?;
    let names = hfst_symbol_names(&transducer)?;
    let model = SourceModel::from_transducer(&transducer, names)?;
    let reader = DhfstTransducer::from_path(&Fs, dhfst_path)
        .with_context(|| format!("failed to load '{}'", dhfst_path.display()))?;
    let threads = match threads {
        0 => std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
        n => n,
    };
    let (queries, arcs) = verify_reader(&model, &reader, threads)?;
    println!(
        "{} answers all {} (state, input) queries of {} exactly as the search reads it: {} arcs, {} states, {} exact duplicate arcs in the source ({:.1} s)",
        dhfst_path.display(),
        queries,
        source.display(),
        arcs,
        model.states().len(),
        model.duplicate_arcs(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cmd_info(path: &Path) -> anyhow::Result<()> {
    let reader = DhfstTransducer::from_path(&Fs, path)
        .with_context(|| format!("failed to load '{}'", path.display()))?;
    let b = reader.buffer();
    println!(
        "{}: {} bytes ({})",
        path.display(),
        b.len(),
        human(b.len() as u64)
    );
    println!(
        "header: DHFST version {}, flags {:#x}, max fallback depth {}",
        dhfst::VERSION,
        reader.flags(),
        reader.max_fallback_depth()
    );
    let n_sections = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as usize;
    for s in 0..n_sections {
        let at = dhfst::HEADER_LEN + dhfst::SECTION_ENTRY_LEN * s;
        let tag = String::from_utf8_lossy(&b[at..at + 4]).into_owned();
        let mut off = [0u8; 8];
        off.copy_from_slice(&b[at + 8..at + 16]);
        let mut len = [0u8; 8];
        len.copy_from_slice(&b[at + 16..at + 24]);
        println!(
            "  section {tag}: offset {}, {} bytes",
            u64::from_le_bytes(off),
            u64::from_le_bytes(len)
        );
    }
    let mut explicit = 0u64;
    let mut blockers = 0u64;
    let mut defaults = [0u64; 4];
    let mut with_fallback = 0u64;
    let mut finals = 0u64;
    for q in 0..reader.state_count() {
        let Some(state) = reader.stored_state(q) else {
            continue;
        };
        with_fallback += state.fallback.is_some() as u64;
        finals += state.final_weight.is_some() as u64;
        for e in state.first..state.first + state.len {
            let entry = reader.entry(e);
            match DefaultKind::from_record(entry.input) {
                Some(kind) => defaults[kind as usize] += 1,
                None if entry.target == dhfst::NONE => blockers += 1,
                None => explicit += 1,
            }
        }
    }
    println!(
        "states {} (final {}, with fallback {}), symbols {}, classes {}, class pairs {}",
        reader.state_count(),
        finals,
        with_fallback,
        reader.symbol_names().len(),
        reader.class_count(),
        reader.class_pair_count()
    );
    println!(
        "entries {}: explicit arcs {}, blockers {}, defaults identity {} substitution {} deletion {} insertion {}",
        reader.entry_count(),
        explicit,
        blockers,
        defaults[0],
        defaults[1],
        defaults[2],
        defaults[3]
    );
    if let Some(meta) = reader.meta() {
        println!("meta: {meta}");
    }
    Ok(())
}

fn cmd_bhfst(archive_path: &Path, dhfst_path: &Path, output: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(dhfst_path)
        .with_context(|| format!("failed to read '{}'", dhfst_path.display()))?;
    let version = DhfstTransducer::from_bytes(&bytes, dhfst_path)
        .with_context(|| format!("'{}' is not a DHFST error model", dhfst_path.display()))?
        .version();

    let mut archive = ZipArchive::new(std::fs::File::open(archive_path)?)
        .with_context(|| format!("failed to read '{}'", archive_path.display()))?;
    let mut xml = Vec::new();
    archive
        .by_name("index.xml")
        .context("the archive has no index.xml")?
        .read_to_end(&mut xml)?;
    let mut metadata = SpellerMetadata::from_bytes(&xml)
        .map_err(|e| anyhow::anyhow!("index.xml does not parse: {e}"))?;
    let acceptor_id = metadata.acceptor().id().to_string();

    let dir = tempfile::tempdir()?;
    let acceptor_path = dir.path().join("acceptor.default.hfst");
    {
        let mut out = std::fs::File::create(&acceptor_path)?;
        std::io::copy(
            &mut archive
                .by_name(&acceptor_id)
                .with_context(|| format!("the archive has no {acceptor_id}"))?,
            &mut out,
        )?;
    }
    let config = match archive.by_name(divvun_fst::archive::BUNDLED_CONFIG_MEMBER) {
        Ok(mut f) => {
            let mut s = Vec::new();
            f.read_to_end(&mut s)?;
            Some(serde_json::from_slice::<serde_json::Value>(&s)?)
        }
        Err(_) => None,
    };
    let acceptor = HfstTransducer::from_path(&Fs, &acceptor_path)?;
    thfst::MmapThfstTransducer::convert_file(&acceptor, &acceptor_path)?;
    let thfst_dir = acceptor_path.with_extension("thfst");

    metadata
        .acceptor_mut()
        .set_id("acceptor.default.thfst".into());
    metadata.errmodel_mut().set_id(DHFST_ERRMODEL_MEMBER.into());
    metadata
        .errmodel_mut()
        .set_format(Some(DHFST_FORMAT.into()), Some(version.to_string()));
    let mut meta = serde_json::to_value(&metadata)?;
    if let (Some(config), Some(object)) = (config, meta.as_object_mut()) {
        object.insert(divvun_fst::archive::BUNDLED_CONFIG_KEY.into(), config);
    }

    let mut boxfile = BoxWriter::create_with_alignment(output, 8)
        .with_context(|| format!("failed to create '{}'", output.display()))?;
    let dir_path = BoxPath::new("acceptor.default.thfst")?;
    boxfile.mkdir(dir_path, BoxHashMap::new())?;
    for name in ["alphabet", "index", "transition"] {
        let file = std::fs::File::open(thfst_dir.join(name))?;
        boxfile.insert(
            &CompressionConfig::new(Compression::Stored),
            BoxPath::new(Path::new("acceptor.default.thfst").join(name))?,
            std::io::BufReader::new(file),
            BoxHashMap::new(),
        )?;
    }
    boxfile.insert(
        &CompressionConfig::new(Compression::Stored),
        BoxPath::new(DHFST_ERRMODEL_MEMBER)?,
        std::io::Cursor::new(bytes.clone()),
        BoxHashMap::new(),
    )?;
    boxfile.insert(
        &CompressionConfig::new(Compression::Stored),
        BoxPath::new("meta.json")?,
        std::io::Cursor::new(serde_json::to_string_pretty(&meta)?.into_bytes()),
        BoxHashMap::new(),
    )?;
    boxfile.finish()?;
    println!(
        "wrote {}: {} bytes; acceptor as THFST, error model {} ({} bytes), meta.json errmodel format \"{}\"",
        output.display(),
        std::fs::metadata(output)?.len(),
        DHFST_ERRMODEL_MEMBER,
        bytes.len(),
        DHFST_FORMAT
    );
    Ok(())
}

/// Name of the placeholder arc's input symbol in the stored model.
const CALL_IN: &str = "<DHFST_CALL_IN>";
/// Name of the placeholder arc's output symbol in the stored model.
const CALL_OUT: &str = "<DHFST_CALL_OUT>";

#[derive(serde::Deserialize)]
struct TableJson {
    symbols: Vec<String>,
    start: u32,
    contexts: Vec<ContextJson>,
    tables: Vec<EditTableJson>,
}

#[derive(serde::Deserialize)]
struct ContextJson {
    #[serde(rename = "final")]
    final_weight: Option<f32>,
    ident: Vec<(Vec<String>, u32)>,
    table: Option<u32>,
}

#[derive(serde::Deserialize)]
struct EditTableJson {
    target: u32,
    sub: Vec<(String, String, f32)>,
    del: Vec<(String, f32)>,
    ins: Vec<(String, f32)>,
    swap: Vec<(String, String, f32)>,
}

fn cmd_stage(
    stored: &Path,
    table: &Path,
    reference: &Path,
    output: &Path,
    max_depth: u32,
    threads: usize,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let stored_t = HfstTransducer::from_path(&Fs, stored)
        .with_context(|| format!("failed to load '{}'", stored.display()))?;
    let stored_names = hfst_symbol_names(&stored_t)?;
    let source = SourceModel::from_transducer(&stored_t, stored_names.clone())?;
    let reference_t = HfstTransducer::from_path(&Fs, reference)
        .with_context(|| format!("failed to load '{}'", reference.display()))?;
    let mut symbols = hfst_symbol_names(&reference_t)?;
    let n_alphabet = symbols.len() as u16;
    symbols.push("@DHFST_CALL_1_IN@".into());
    symbols.push("@DHFST_CALL_1_OUT@".into());
    let index_of = |name: &str| {
        symbols[..n_alphabet as usize]
            .iter()
            .position(|s| s == name)
    };

    let call_in = stored_names.iter().position(|s| s == CALL_IN);
    let call_out = stored_names.iter().position(|s| s == CALL_OUT);
    let mut map: Vec<Option<u16>> = Vec::with_capacity(stored_names.len());
    let mut missing: Vec<&str> = Vec::new();
    for (i, name) in stored_names.iter().enumerate() {
        if Some(i) == call_in {
            map.push(Some(n_alphabet));
        } else if Some(i) == call_out {
            map.push(Some(n_alphabet + 1));
        } else {
            match index_of(name) {
                Some(at) => map.push(Some(at as u16)),
                None => {
                    missing.push(name);
                    map.push(None);
                }
            }
        }
    }
    if !missing.is_empty() {
        bail!("symbols of the stored model missing from the reference: {missing:?}");
    }
    let unused = symbols[..n_alphabet as usize]
        .iter()
        .filter(|s| !stored_names.contains(s))
        .count();

    let (mut calls, mut dropped) = (0u64, 0u64);
    let mut states: Vec<SourceState> = Vec::new();
    for state in source.states() {
        let mut arcs = Vec::with_capacity(state.arcs.len());
        for arc in &state.arcs {
            let (Some(i), Some(o)) = (map[arc.input as usize], map[arc.output as usize]) else {
                bail!("an arc names a symbol with no place in the reference");
            };
            let is_call = |s: u16| s >= n_alphabet;
            if is_call(i) || is_call(o) {
                if (i, o) == (n_alphabet, n_alphabet + 1) {
                    calls += 1;
                } else {
                    dropped += 1;
                    continue;
                }
            }
            arcs.push(SourceArc {
                input: i,
                output: o,
                target: arc.target,
                weight: arc.weight,
            });
        }
        states.push(SourceState {
            final_weight: state.final_weight,
            arcs,
        });
    }
    let model = SourceModel::new(symbols.clone(), states)?;

    let spec: TableJson = serde_json::from_reader(std::fs::File::open(table)?)
        .with_context(|| format!("failed to read '{}'", table.display()))?;
    let sym = |name: &str| -> anyhow::Result<u16> {
        index_of(name)
            .map(|i| i as u16)
            .with_context(|| format!("table symbol {name:?} is not in the reference alphabet"))
    };
    let mut contexts = Vec::new();
    for c in &spec.contexts {
        let mut ident = Vec::new();
        for (names, target) in &c.ident {
            ident.push((
                names
                    .iter()
                    .map(|n| sym(n))
                    .collect::<anyhow::Result<Vec<u16>>>()?,
                *target,
            ));
        }
        contexts.push(ContextSpec {
            final_weight: c.final_weight,
            ident,
            table: c.table,
        });
    }
    let mut tables = Vec::new();
    for t in &spec.tables {
        tables.push(TableSpec {
            target: t.target,
            sub: t
                .sub
                .iter()
                .map(|(x, y, w)| Ok((sym(x)?, sym(y)?, *w)))
                .collect::<anyhow::Result<_>>()?,
            del: t
                .del
                .iter()
                .map(|(x, w)| Ok((sym(x)?, *w)))
                .collect::<anyhow::Result<_>>()?,
            ins: t
                .ins
                .iter()
                .map(|(x, w)| Ok((sym(x)?, *w)))
                .collect::<anyhow::Result<_>>()?,
            swap: t
                .swap
                .iter()
                .map(|(x, y, w)| Ok((sym(x)?, sym(y)?, *w)))
                .collect::<anyhow::Result<_>>()?,
        });
    }
    let _ = &spec.symbols;
    let stages = StagesSpec {
        n_alphabet: n_alphabet as u32,
        stages: vec![StageSpec {
            call_input: n_alphabet,
            call_output: n_alphabet + 1,
            table: EditStageSpec {
                contexts,
                start: spec.start,
                tables,
            },
        }],
    };
    let written = write(
        &model,
        &WriteOptions {
            max_fallback_depth: Some(max_depth),
            threads,
            source_name: stored
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            stages: Some(stages),
        },
    )?;
    std::fs::write(output, &written.bytes)?;
    let r = &written.report;
    println!(
        "stored model: {} states, {} arcs; {} call arcs; {} harmonisation arcs on call symbols dropped; {} reference symbols unused by it",
        model.states().len(),
        model.arc_count(),
        calls,
        dropped,
        unused
    );
    println!(
        "checked: {} (state, pair) resolutions, {} stored (state, input) queries, {} stage (state, input) queries against the table",
        r.pairs_checked, r.queries_checked, r.stage_queries_checked
    );
    println!(
        "wrote {}: {} bytes ({}), of which STAG {} bytes ({:.1} s)",
        output.display(),
        written.bytes.len(),
        human(written.bytes.len() as u64),
        r.stage_bytes,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Symbol name as AT&T text needs it.
fn att_name(name: &str) -> String {
    match name {
        "" | "@_EPSILON_SYMBOL_@" => "@0@".into(),
        " " => "@_SPACE_@".into(),
        "\t" => "@_TAB_@".into(),
        "\u{a0}" => "<U+00A0>".into(),
        "\u{202f}" => "<U+202F>".into(),
        other => other.into(),
    }
}

fn dump<T: Transducer>(
    t: &T,
    names: &[String],
    n_inputs: usize,
    output: &Path,
) -> anyhow::Result<(usize, u64)> {
    use std::collections::HashMap;
    use std::io::Write as _;
    let mut ids: HashMap<u32, u32> = HashMap::new();
    let mut order: Vec<u32> = vec![0];
    ids.insert(0, 0);
    let mut out = std::io::BufWriter::new(std::fs::File::create(output)?);
    let mut finals: Vec<(u32, f32)> = Vec::new();
    let mut arcs = 0u64;
    let mut cursor = 0;
    while cursor < order.len() {
        let state = order[cursor];
        let id = cursor as u32;
        cursor += 1;
        let mut lines: Vec<(u32, u16, u16, u32)> = Vec::new();
        for input in 0..n_inputs as u16 {
            t.for_each_arc(
                TransitionTableIndex(state),
                SymbolNumber(input),
                |o, target, w| {
                    let next = order.len() as u32;
                    let tid = *ids.entry(target.0).or_insert_with(|| {
                        order.push(target.0);
                        next
                    });
                    lines.push((tid, input, o.0, w.0.to_bits()));
                },
            );
        }
        for (tid, i, o, w) in lines {
            writeln!(
                out,
                "{id}\t{tid}\t{}\t{}\t{}",
                att_name(&names[i as usize]),
                att_name(&names[o as usize]),
                f32::from_bits(w)
            )?;
            arcs += 1;
        }
        let at = TransitionTableIndex(state);
        if t.is_final(at)
            && let Some(w) = t.final_weight(at)
        {
            finals.push((id, w.0));
        }
    }
    for (id, w) in finals {
        writeln!(out, "{id}\t{w}")?;
    }
    Ok((order.len(), arcs))
}

fn cmd_dump(input: &Path, output: &Path) -> anyhow::Result<()> {
    let mut header = [0u8; 8];
    let filled = std::fs::File::open(input)?.read(&mut header)?;
    let (states, arcs) = match TransducerFormat::detect(&header[..filled], input)? {
        TransducerFormat::Hfst => {
            let t = HfstTransducer::from_path(&Fs, input)?;
            let names = hfst_symbol_names(&t)?;
            dump(&t, &names, names.len(), output)?
        }
        TransducerFormat::Dhfst { .. } => {
            let t = DhfstTransducer::from_path(&Fs, input)?;
            let names = t.symbol_names().to_vec();
            dump(&t, &names, t.alphabet_len() as usize, output)?
        }
    };
    println!(
        "{}: {states} states, {arcs} arcs as the search reads them",
        output.display()
    );
    Ok(())
}

fn run() -> anyhow::Result<()> {
    match Opts::parse() {
        Opts::Write {
            input,
            output,
            max_depth,
            unbounded,
            threads,
        } => cmd_write(
            &input,
            &output,
            if unbounded { None } else { Some(max_depth) },
            threads,
        ),
        Opts::Check {
            source,
            dhfst,
            threads,
        } => cmd_check(&source, &dhfst, threads),
        Opts::Info { path } => cmd_info(&path),
        Opts::Bhfst {
            archive,
            dhfst,
            output,
        } => cmd_bhfst(&archive, &dhfst, &output),
        Opts::Stage {
            stored,
            table,
            reference,
            output,
            max_depth,
            threads,
        } => cmd_stage(&stored, &table, &reference, &output, max_depth, threads),
        Opts::Dump { input, output } => cmd_dump(&input, &output),
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err:?}");
            std::process::ExitCode::FAILURE
        }
    }
}
