//! Write, check and package error models in the compact DHFST format, and
//! acceptors in the DHFST acceptor format.

mod acceptor;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context as _, bail};
use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter};
use clap::Parser;
use divvun_fst::archive::boxf::DHFST_ERRMODEL_MEMBER;
use divvun_fst::archive::meta::SpellerMetadata;
use divvun_fst::transducer::Transducer;
use divvun_fst::transducer::dhfst::{
    self, DefaultKind, DhfstTransducer,
    writer::{
        ContextSpec, EditStageSpec, SourceArc, SourceModel, SourceState, StageKind, StageSpec,
        StagesSpec, TableSpec, WriteOptions, verify_reader, write,
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
    about = "Write, check and package error models in the compact DHFST format, and acceptors in the DHFST acceptor format."
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

    /// Describe a DHFST file, an error model or an acceptor
    Info {
        /// the DHFST file
        path: PathBuf,
    },

    /// Write a DHFST file that combines components at search time: a top
    /// level whose call arcs call stages, each component stored once
    ///
    /// The top level is an error model in which every component is one
    /// placeholder arc "<DHFST_CALL_NAME_IN>":"<DHFST_CALL_NAME_OUT>"
    /// ("<DHFST_CALL_IN>":"<DHFST_CALL_OUT>" for the empty name), whose
    /// target is where the component returns to. Each name is a stage: a
    /// stored component (`--stored NAME=FILE`, HFST optimized lookup) or an
    /// edit table (`--table NAME=FILE`, JSON). Other arcs on placeholder
    /// symbols (identities hfst's harmonisation adds to `?` loops) are
    /// dropped. A stored component is trimmed and weight-pushed towards its
    /// start with the least cost through it taken out, as the top level's
    /// placeholder carries it. The file keeps the reference model's symbol
    /// numbering, with the call symbols after it.
    Combine {
        /// the top level, HFST optimized lookup
        top: PathBuf,
        /// the original error model, HFST optimized lookup, for its symbols
        reference: PathBuf,
        /// the DHFST file to write
        output: PathBuf,
        /// a stored component, NAME=FILE
        #[arg(long = "stored", value_name = "NAME=FILE")]
        stored: Vec<String>,
        /// an edit table, NAME=FILE
        #[arg(long = "table", value_name = "NAME=FILE")]
        table: Vec<String>,
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

    /// Write an HFST optimized-lookup acceptor as a DHFST acceptor, checking
    /// every state's finality, distance to a final state, free arcs and
    /// answer for every symbol of the written file against the source
    Acceptor {
        /// the acceptor, HFST optimized lookup
        input: PathBuf,
        /// the DHFST acceptor to write
        output: PathBuf,
        /// a THFST copy of the acceptor to check the written file against too
        #[arg(long)]
        thfst: Option<PathBuf>,
        /// place the states that need the most slots first, instead of in
        /// depth-first order
        #[arg(long)]
        fan_out: bool,
        /// worker threads for the check (0: all cores)
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },

    /// Repackage a BHFST archive with a DHFST error model, taking its
    /// acceptor from a DHFST acceptor file, a THFST directory, or the archive
    /// itself
    Pack {
        /// the BHFST archive whose metadata and error model to use
        from: PathBuf,
        /// the BHFST archive to write
        output: PathBuf,
        /// a DHFST acceptor to store as acceptor.default.dhfst
        #[arg(long)]
        acceptor: Option<PathBuf>,
        /// a THFST directory to store as acceptor.default.thfst
        #[arg(long)]
        thfst: Option<PathBuf>,
        /// a DHFST error model to store instead of the archive's
        #[arg(long)]
        errmodel: Option<PathBuf>,
        /// a speller config to bundle instead of the archive's
        #[arg(long)]
        config: Option<PathBuf>,
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
    let mut header = [0u8; 8];
    let filled = std::fs::File::open(path)
        .with_context(|| format!("failed to open '{}'", path.display()))?
        .read(&mut header)?;
    if let TransducerFormat::Dhfst {
        kind: dhfst::DhfstType::Acceptor,
        ..
    } = TransducerFormat::detect(&header[..filled], path)?
    {
        return acceptor::cmd_acceptor_info(path);
    }
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
        "header: DHFST type {} ({}), version {}, flags {:#x}, max fallback depth {}",
        reader.kind().byte(),
        reader.kind(),
        reader.version(),
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
    // What each part of the model costs: 16 bytes per state record and 12
    // per entry.
    let part_bytes = |from: u32, to: u32| -> (u64, u64) {
        let mut entries = 0u64;
        for q in from..to {
            if let Some(state) = reader.stored_state(q) {
                entries += state.len as u64;
            }
        }
        (entries, 16 * (to - from) as u64 + 12 * entries)
    };
    let summary = reader.stage_summary();
    if !summary.is_empty() {
        let n_top = reader.top_level_len();
        let (entries, bytes) = part_bytes(0, n_top);
        println!(
            "top level: {n_top} states, {entries} entries, {bytes} bytes of states and entries"
        );
        for (k, stage) in summary.iter().enumerate() {
            match *stage {
                dhfst::StageSummary::Stored {
                    call, first, count, ..
                } => {
                    let (entries, bytes) = part_bytes(first, first + count);
                    println!(
                        "stage {k} ({}): stored, {count} states, {entries} entries, {bytes} bytes of states and entries",
                        reader.symbol_names()[call.0 as usize]
                    );
                }
                dhfst::StageSummary::EditTable {
                    call,
                    contexts,
                    tables,
                } => println!(
                    "stage {k} ({}): edit table, {contexts} contexts, {tables} tables",
                    reader.symbol_names()[call.0 as usize]
                ),
            }
        }
    }
    if let Some(meta) = reader.meta() {
        println!("meta: {meta}");
    }
    Ok(())
}

fn cmd_bhfst(archive_path: &Path, dhfst_path: &Path, output: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(dhfst_path)
        .with_context(|| format!("failed to read '{}'", dhfst_path.display()))?;
    DhfstTransducer::from_bytes(&bytes, dhfst_path)
        .with_context(|| format!("'{}' is not a DHFST error model", dhfst_path.display()))?;

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
        "wrote {}: {} bytes; acceptor as THFST, error model {} ({} bytes)",
        output.display(),
        std::fs::metadata(output)?.len(),
        DHFST_ERRMODEL_MEMBER,
        bytes.len()
    );
    Ok(())
}

/// The placeholder pair that stands for component `name` in the top level.
fn placeholder(name: &str) -> (String, String) {
    if name.is_empty() {
        ("<DHFST_CALL_IN>".into(), "<DHFST_CALL_OUT>".into())
    } else {
        (
            format!("<DHFST_CALL_{name}_IN>"),
            format!("<DHFST_CALL_{name}_OUT>"),
        )
    }
}

fn is_placeholder(name: &str) -> bool {
    name.starts_with("<DHFST_CALL_") && name.ends_with('>')
}

#[derive(serde::Deserialize)]
struct TableJson {
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
    #[serde(default)]
    swap_entry: Vec<(String, f32)>,
}

/// An edit table read from JSON, in the alphabet `sym` numbers.
fn read_table(
    path: &Path,
    sym: &dyn Fn(&str) -> anyhow::Result<u16>,
) -> anyhow::Result<EditStageSpec> {
    let spec: TableJson = serde_json::from_reader(std::fs::File::open(path)?)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
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
            swap_entry: t
                .swap_entry
                .iter()
                .map(|(x, w)| Ok((sym(x)?, *w)))
                .collect::<anyhow::Result<_>>()?,
        });
    }
    Ok(EditStageSpec {
        contexts,
        start: spec.start,
        tables,
    })
}

/// How a model's symbol pair is renumbered: `None` drops the arc, an error
/// refuses the model.
type Renumber<'a> = &'a dyn Fn(&str, &str) -> anyhow::Result<Option<(u16, u16)>>;

/// An HFST optimized-lookup model's states with its symbols renumbered by
/// `number`, which answers `None` to drop an arc and an error to refuse it.
fn renumbered(path: &Path, number: Renumber<'_>) -> anyhow::Result<(Vec<SourceState>, u64)> {
    let t = HfstTransducer::from_path(&Fs, path)
        .with_context(|| format!("failed to load '{}'", path.display()))?;
    let names = hfst_symbol_names(&t)?;
    let model = SourceModel::from_transducer(&t, names.clone())?;
    let mut dropped = 0u64;
    let mut states = Vec::with_capacity(model.states().len());
    for state in model.states() {
        let mut arcs = Vec::with_capacity(state.arcs.len());
        for arc in &state.arcs {
            match number(&names[arc.input as usize], &names[arc.output as usize])
                .with_context(|| format!("in '{}'", path.display()))?
            {
                Some((input, output)) => arcs.push(SourceArc {
                    input,
                    output,
                    ..*arc
                }),
                None => dropped += 1,
            }
        }
        states.push(SourceState {
            final_weight: state.final_weight,
            arcs,
        });
    }
    Ok((states, dropped))
}

/// A component trimmed to the states on some path from its start (state 0)
/// to a final state, numbered from its start in breadth-first order, and
/// weight-pushed towards the start: each arc `s -> t` gains `d(t) - d(s)`
/// and each final weight loses `d(s)`, `d` being the least cost from a state
/// to the end. Answers the states and `d(start)`, the cost taken out.
fn pushed(states: &[SourceState]) -> anyhow::Result<(Vec<SourceState>, f64)> {
    let n = states.len();
    // Least cost to the end, by Bellman-Ford over the reversed arcs; the
    // components are acyclic or have nonnegative cycles.
    let mut d: Vec<f64> = states
        .iter()
        .map(|s| s.final_weight.map_or(f64::INFINITY, f64::from))
        .collect();
    let mut changed = true;
    let mut rounds = 0;
    while changed {
        changed = false;
        rounds += 1;
        if rounds > n + 1 {
            bail!("a component has a negative cycle");
        }
        for (q, state) in states.iter().enumerate() {
            for arc in &state.arcs {
                let via = arc.weight as f64 + d[arc.target as usize];
                if via < d[q] {
                    d[q] = via;
                    changed = true;
                }
            }
        }
    }
    if !d[0].is_finite() {
        bail!("a component accepts nothing");
    }
    let mut id: Vec<Option<u32>> = vec![None; n];
    let mut order: Vec<usize> = vec![0];
    id[0] = Some(0);
    let mut at = 0;
    while at < order.len() {
        let q = order[at];
        at += 1;
        for arc in &states[q].arcs {
            let t = arc.target as usize;
            if d[t].is_finite() && id[t].is_none() {
                id[t] = Some(order.len() as u32);
                order.push(t);
            }
        }
    }
    let mut out = Vec::with_capacity(order.len());
    for &q in &order {
        let state = &states[q];
        let mut arcs = Vec::with_capacity(state.arcs.len());
        for arc in &state.arcs {
            let t = arc.target as usize;
            if let Some(target) = id[t] {
                arcs.push(SourceArc {
                    target,
                    weight: (arc.weight as f64 + d[t] - d[q]) as f32,
                    ..*arc
                });
            }
        }
        out.push(SourceState {
            final_weight: state.final_weight.map(|f| (f as f64 - d[q]) as f32),
            arcs,
        });
    }
    Ok((out, d[0]))
}

/// Split `NAME=FILE`.
fn named(arg: &str) -> anyhow::Result<(String, PathBuf)> {
    let (name, file) = arg
        .split_once('=')
        .with_context(|| format!("{arg:?} is not NAME=FILE"))?;
    Ok((name.to_string(), PathBuf::from(file)))
}

#[allow(clippy::too_many_arguments)]
fn cmd_combine(
    top: &Path,
    reference: &Path,
    output: &Path,
    stored: &[String],
    tables: &[String],
    max_depth: u32,
    threads: usize,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let reference_t = HfstTransducer::from_path(&Fs, reference)
        .with_context(|| format!("failed to load '{}'", reference.display()))?;
    let mut symbols = hfst_symbol_names(&reference_t)?;
    let n_alphabet = symbols.len() as u16;
    let alphabet = symbols.clone();
    let index_of = |name: &str| alphabet.iter().position(|s| s == name).map(|i| i as u16);

    // The stages in the order given: stored components, then tables.
    let mut parts: Vec<(String, PathBuf, bool)> = Vec::new();
    for arg in stored {
        let (name, file) = named(arg)?;
        parts.push((name, file, true));
    }
    for arg in tables {
        let (name, file) = named(arg)?;
        parts.push((name, file, false));
    }
    if parts.is_empty() {
        bail!("no stages: give --stored or --table");
    }
    let mut calls: Vec<(String, String, u16, u16)> = Vec::new();
    for (k, (name, _, _)) in parts.iter().enumerate() {
        let (i, o) = placeholder(name);
        if calls.iter().any(|c| c.0 == i) {
            bail!("stage {name:?} is given twice");
        }
        let call_in = symbols.len() as u16;
        symbols.push(format!("@DHFST_CALL_{}_IN@", k + 1));
        symbols.push(format!("@DHFST_CALL_{}_OUT@", k + 1));
        calls.push((i, o, call_in, call_in + 1));
    }

    let plain = |i: &str, o: &str| -> anyhow::Result<Option<(u16, u16)>> {
        if is_placeholder(i) || is_placeholder(o) {
            return Ok(None);
        }
        match (index_of(i), index_of(o)) {
            (Some(i), Some(o)) => Ok(Some((i, o))),
            _ => bail!("{i:?}:{o:?} is not in the reference alphabet"),
        }
    };
    let call_counts: std::cell::RefCell<Vec<u64>> = std::cell::RefCell::new(vec![0; calls.len()]);
    let in_top = |i: &str, o: &str| -> anyhow::Result<Option<(u16, u16)>> {
        if let Some(k) = calls.iter().position(|c| c.0 == i && c.1 == o) {
            call_counts.borrow_mut()[k] += 1;
            return Ok(Some((calls[k].2, calls[k].3)));
        }
        if (is_placeholder(i) || is_placeholder(o)) && i != o {
            bail!("placeholder {i:?}:{o:?} names no stage");
        }
        plain(i, o)
    };
    let (mut states, top_dropped) = renumbered(top, &in_top)?;
    let n_top = states.len() as u32;
    let call_counts = call_counts.into_inner();
    for (k, (name, _, _)) in parts.iter().enumerate() {
        if call_counts[k] == 0 {
            bail!("the top level never calls {name:?}");
        }
    }
    println!(
        "top level {}: {} states, {} arcs, calls {:?}, {} harmonisation arcs dropped",
        top.display(),
        n_top,
        states.iter().map(|s| s.arcs.len()).sum::<usize>(),
        parts
            .iter()
            .zip(&call_counts)
            .map(|((n, _, _), c)| format!("{n}x{c}"))
            .collect::<Vec<_>>(),
        top_dropped
    );

    let sym = |name: &str| -> anyhow::Result<u16> {
        index_of(name)
            .with_context(|| format!("table symbol {name:?} is not in the reference alphabet"))
    };
    let mut stages: Vec<StageSpec> = Vec::new();
    for (k, (name, file, is_stored)) in parts.iter().enumerate() {
        let (_, _, call_input, call_output) = calls[k];
        let kind = if *is_stored {
            let (component, dropped) = renumbered(file, &plain)?;
            let (component, removed) = pushed(&component)?;
            let first = states.len() as u32;
            let count = component.len() as u32;
            println!(
                "stage {name:?}: stored {}: {} states, {} arcs, least cost {} taken out, {} harmonisation arcs dropped",
                file.display(),
                count,
                component.iter().map(|s| s.arcs.len()).sum::<usize>(),
                removed,
                dropped
            );
            for state in component {
                states.push(SourceState {
                    final_weight: state.final_weight,
                    arcs: state
                        .arcs
                        .into_iter()
                        .map(|a| SourceArc {
                            target: a.target + first,
                            ..a
                        })
                        .collect(),
                });
            }
            StageKind::Stored {
                start: first,
                first,
                count,
            }
        } else {
            let table = read_table(file, &sym)?;
            println!(
                "stage {name:?}: edit table {}: {} contexts, {} tables",
                file.display(),
                table.contexts.len(),
                table.tables.len()
            );
            StageKind::Table(table)
        };
        stages.push(StageSpec {
            call_input,
            call_output,
            kind,
        });
    }
    let model = SourceModel::new(symbols, states)?;
    let written = write(
        &model,
        &WriteOptions {
            max_fallback_depth: Some(max_depth),
            threads,
            source_name: top
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            stages: Some(StagesSpec {
                n_alphabet: n_alphabet as u32,
                n_top,
                stages,
            }),
        },
    )?;
    std::fs::write(output, &written.bytes)?;
    let r = &written.report;
    println!(
        "checked: {} (state, pair) resolutions, {} stored (state, input) queries, {} stage (state, input) queries",
        r.pairs_checked, r.queries_checked, r.stage_queries_checked
    );
    println!(
        "wrote {}: {} bytes ({}), {} states, of which STAG {} bytes ({:.1} s)",
        output.display(),
        written.bytes.len(),
        human(written.bytes.len() as u64),
        model.states().len(),
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
        Opts::Combine {
            top,
            reference,
            output,
            stored,
            table,
            max_depth,
            threads,
        } => cmd_combine(
            &top, &reference, &output, &stored, &table, max_depth, threads,
        ),
        Opts::Dump { input, output } => cmd_dump(&input, &output),
        Opts::Acceptor {
            input,
            output,
            thfst,
            fan_out,
            threads,
        } => acceptor::cmd_acceptor(&input, &output, thfst.as_deref(), fan_out, threads),
        Opts::Pack {
            from,
            output,
            acceptor,
            thfst,
            errmodel,
            config,
        } => acceptor::cmd_pack(
            &from,
            &output,
            acceptor.as_deref(),
            thfst.as_deref(),
            errmodel.as_deref(),
            config.as_deref(),
        ),
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
