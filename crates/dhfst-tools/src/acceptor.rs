//! Write, check, describe and package DHFST acceptors.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context as _, bail};
use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter};
use divvun_fst::archive::boxf::{
    DHFST_ACCEPTOR_MEMBER, DHFST_ERRMODEL_MEMBER, THFST_ACCEPTOR_MEMBER, THFST_ERRMODEL_MEMBER,
};
use divvun_fst::transducer::TransducerLoader;
use divvun_fst::transducer::dhfst::acceptor::DhfstAcceptor;
use divvun_fst::transducer::dhfst::acceptor_writer::{
    AcceptorOptions, Placement, verify_acceptor, write_acceptor,
};
use divvun_fst::transducer::hfst::HfstTransducer;
use divvun_fst::transducer::thfst::MmapThfstTransducer;
use divvun_fst::vfs::{Filesystem as _, Fs};

use crate::{hfst_symbol_names, human};

/// Write an HFST optimized-lookup acceptor as a DHFST acceptor, checked
/// against the source and, when given, against a THFST copy of it.
pub fn cmd_acceptor(
    input: &Path,
    output: &Path,
    thfst: Option<&Path>,
    fan_out: bool,
    threads: usize,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let source = HfstTransducer::from_path(&Fs, input)
        .with_context(|| format!("failed to load '{}'", input.display()))?;
    let names = hfst_symbol_names(&source)?;
    let options = AcceptorOptions {
        placement: if fan_out {
            Placement::FanOut
        } else {
            Placement::DepthFirst
        },
        threads,
        source_name: input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let written = write_acceptor(&source, &names, &options)?;
    println!(
        "wrote {} states numbered below {}, {} arcs ({} free) in {} slots ({} used, {:.1}%), \
         {} list records, {} weights, {} free pairs, {}; checked {} (state, symbol) answers \
         against the source ({} free-symbol quirks) in {:.1} s",
        written.states,
        written.ids,
        written.arcs,
        written.free_arcs,
        written.slots,
        written.used_slots,
        100.0 * written.used_slots as f64 / written.slots as f64,
        written.list_records,
        written.weights,
        written.free_pairs,
        if written.distances {
            "distances to a final state stored"
        } else {
            "every distance to a final state 0, none stored"
        },
        written.checked,
        written.free_symbol_quirks,
        started.elapsed().as_secs_f64()
    );

    if let Some(dir) = thfst {
        let started = Instant::now();
        let thfst = MmapThfstTransducer::from_path(&Fs, dir)
            .with_context(|| format!("failed to load '{}'", dir.display()))?;
        let reader = DhfstAcceptor::from_bytes(&written.bytes, output)?;
        let (checked, quirks) = verify_acceptor(&thfst, &reader, &written, threads)?;
        println!(
            "checked {checked} (state, symbol) answers against THFST '{}' ({quirks} free-symbol quirks) in {:.1} s",
            dir.display(),
            started.elapsed().as_secs_f64()
        );
    }

    std::fs::write(output, &written.bytes)
        .with_context(|| format!("failed to write '{}'", output.display()))?;
    println!(
        "{}: {} bytes ({})",
        output.display(),
        written.bytes.len(),
        human(written.bytes.len() as u64)
    );
    Ok(())
}

/// Describe a DHFST acceptor.
pub fn cmd_acceptor_info(path: &Path) -> anyhow::Result<()> {
    let started = Instant::now();
    let reader = DhfstAcceptor::from_path(&Fs, path)
        .with_context(|| format!("failed to load '{}'", path.display()))?;
    let load = started.elapsed();
    let b = reader.buffer();
    println!(
        "{}: {} bytes ({}), loaded and validated in {:.1} ms",
        path.display(),
        b.len(),
        human(b.len() as u64),
        load.as_secs_f64() * 1e3
    );
    println!(
        "header: DHFST type 2 (acceptor), version {}, flags {:#x}",
        reader.version(),
        reader.flags()
    );
    for (tag, len) in reader.section_sizes() {
        println!("  section {tag}: {len} bytes ({})", human(len));
    }
    let info = reader.info();
    println!(
        "{} state numbers, {} final; {} arcs ({} free), {} free symbol and weight pairs, \
         {} regular arc weights",
        info.ids, info.finals, info.arcs, info.free_arcs, info.free_pairs, info.weights,
    );
    println!(
        "{} slots, {} used ({:.1}%), {}-byte checks, labels up to {}; {}-byte records \
         ({} + {} bits), {} list records; distances to a final state {}",
        info.slots,
        info.used_slots,
        100.0 * info.used_slots as f64 / info.slots as f64,
        info.check_bytes,
        info.label_max,
        info.record_bytes,
        info.target_bits,
        info.index_bits,
        info.list_records,
        if info.distances { "stored" } else { "all 0" },
    );
    if let Some(meta) = reader.meta() {
        println!("meta: {meta}");
    }
    Ok(())
}

fn read_member(fs: &divvun_fst::vfs::boxf::Filesystem<'_>, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut file = fs
        .open_file(name)
        .with_context(|| format!("the archive has no {name}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Repackage a BHFST archive: its metadata and DHFST error model, with the
/// acceptor taken from a DHFST acceptor file, a THFST directory, or the
/// archive itself.
pub fn cmd_pack(
    from: &Path,
    output: &Path,
    acceptor: Option<&Path>,
    thfst: Option<&Path>,
    errmodel: Option<&Path>,
    config: Option<&Path>,
) -> anyhow::Result<()> {
    let archive = box_format::sync::BoxReader::open(from)
        .map_err(|e| anyhow::anyhow!("failed to open '{}': {e}", from.display()))?;
    let fs = divvun_fst::vfs::boxf::Filesystem::new(&archive);

    let meta_bytes = read_member(&fs, "meta.json")?;
    let mut meta: serde_json::Value = serde_json::from_slice(&meta_bytes)?;
    let errmodel = match errmodel {
        Some(path) => {
            std::fs::read(path).with_context(|| format!("failed to read '{}'", path.display()))?
        }
        None => read_member(&fs, DHFST_ERRMODEL_MEMBER).with_context(|| {
            format!("only archives with a {DHFST_ERRMODEL_MEMBER} error model are supported")
        })?,
    };
    if let Some(path) = config {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)
            .with_context(|| format!("'{}' is not JSON", path.display()))?;
        if let Some(object) = meta.as_object_mut() {
            object.insert(divvun_fst::archive::BUNDLED_CONFIG_KEY.into(), value);
        }
    }
    if fs
        .open_file(format!("{THFST_ERRMODEL_MEMBER}/alphabet"))
        .is_ok()
    {
        bail!("the archive carries two error models");
    }

    let mut boxfile = BoxWriter::create_with_alignment(output, 8)
        .with_context(|| format!("failed to create '{}'", output.display()))?;
    let stored = CompressionConfig::new(Compression::Stored);
    let acceptor_id = match (acceptor, thfst) {
        (Some(_), Some(_)) => {
            bail!("give the acceptor as a DHFST file or a THFST directory, not both")
        }
        (Some(path), None) => {
            let bytes = std::fs::read(path)?;
            DhfstAcceptor::from_bytes(&bytes, path)
                .with_context(|| format!("'{}' is not a DHFST acceptor", path.display()))?;
            boxfile.insert(
                &stored,
                BoxPath::new(DHFST_ACCEPTOR_MEMBER)?,
                std::io::Cursor::new(bytes),
                BoxHashMap::new(),
            )?;
            DHFST_ACCEPTOR_MEMBER
        }
        (None, thfst) => {
            boxfile.mkdir(BoxPath::new(THFST_ACCEPTOR_MEMBER)?, BoxHashMap::new())?;
            for name in ["alphabet", "index", "transition"] {
                let bytes = match thfst {
                    Some(dir) => std::fs::read(dir.join(name))?,
                    None => read_member(&fs, &format!("{THFST_ACCEPTOR_MEMBER}/{name}"))?,
                };
                boxfile.insert(
                    &stored,
                    BoxPath::new(PathBuf::from(THFST_ACCEPTOR_MEMBER).join(name))?,
                    std::io::Cursor::new(bytes),
                    BoxHashMap::new(),
                )?;
            }
            THFST_ACCEPTOR_MEMBER
        }
    };
    boxfile.insert(
        &stored,
        BoxPath::new(DHFST_ERRMODEL_MEMBER)?,
        std::io::Cursor::new(errmodel),
        BoxHashMap::new(),
    )?;
    if let Some(id) = meta.pointer_mut("/acceptor/id") {
        *id = serde_json::Value::String(acceptor_id.into());
    }
    boxfile.insert(
        &stored,
        BoxPath::new("meta.json")?,
        std::io::Cursor::new(serde_json::to_string_pretty(&meta)?.into_bytes()),
        BoxHashMap::new(),
    )?;
    boxfile.finish()?;
    println!(
        "wrote {}: {} bytes; acceptor {acceptor_id}",
        output.display(),
        std::fs::metadata(output)?.len()
    );
    Ok(())
}
