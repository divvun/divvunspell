use anyhow::{Context as _, bail};
use clap::Parser;
use std::path::{Path, PathBuf};

use box_format::{BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter};
use divvun_fst::archive::{
    SpellerArchive, boxf::BoxSpellerArchive, boxf::ThfstBoxSpellerArchive, zip::ZipSpellerArchive,
};
use divvun_fst::transducer::{
    TransducerLoader,
    convert::ConvertFile,
    hfst::HfstTransducer,
    thfst::{self, MmapThfstTransducer},
};

#[derive(Debug, Parser)]
#[command(
    name = "thfst-tools",
    about = "Tromsø-Helsinki Finite State Transducer toolkit."
)]
enum Opts {
    /// Convert an HFST file to THFST
    HfstToThfst { from: PathBuf },

    /// Convert a ZHFST file to BHFST
    ZhfstToBhfst { from: PathBuf },

    /// Convert a THFST acceptor/errmodel pair to BHFST
    ThfstsToBhfst {
        acceptor: PathBuf,
        errmodel: PathBuf,
        output: PathBuf,
    },

    /// Print metadata for BHFST
    BhfstInfo { path: PathBuf },
}

const ALIGNMENT: u32 = 8;

fn boxpath(dir: &Path, filename: &str) -> anyhow::Result<BoxPath<'static>> {
    let dir_name = dir
        .file_name()
        .with_context(|| format!("path '{}' has no file name component", dir.display()))?;
    BoxPath::new(Path::new(dir_name).join(filename))
        .with_context(|| format!("invalid box path from '{}/{}'", dir.display(), filename))
}

fn insert(
    boxfile: &mut BoxWriter,
    compression: Compression,
    dir: &Path,
    name: &str,
) -> anyhow::Result<()> {
    use std::io::BufReader;
    let entry_path = dir.join(name);
    let file = std::fs::File::open(&entry_path)
        .with_context(|| format!("failed to open '{}'", entry_path.display()))?;
    boxfile
        .insert(
            &CompressionConfig::new(compression),
            boxpath(dir, name)?,
            BufReader::new(file),
            BoxHashMap::new(),
        )
        .with_context(|| {
            format!(
                "failed to insert '{}' into box archive",
                entry_path.display()
            )
        })?;
    Ok(())
}

fn insert_thfst_files(boxfile: &mut BoxWriter, dir: &Path) -> anyhow::Result<()> {
    let file_name = dir
        .file_name()
        .with_context(|| format!("path '{}' has no file name component", dir.display()))?;
    let dir_path = BoxPath::new(file_name)
        .with_context(|| format!("invalid box path from '{}'", dir.display()))?;
    println!("Inserting \"{}\"...", &dir_path);

    boxfile.mkdir(dir_path, BoxHashMap::new())?;
    insert(boxfile, Compression::Stored, dir, "alphabet")?;
    insert(boxfile, Compression::Stored, dir, "index")?;
    insert(boxfile, Compression::Stored, dir, "transition")
}

fn convert_hfst_to_thfst(hfst_path: &Path) -> anyhow::Result<()> {
    let fs = divvun_fst::vfs::Fs;
    let transducer = HfstTransducer::from_path(&fs, hfst_path)
        .with_context(|| format!("failed to load HFST transducer '{}'", hfst_path.display()))?;
    let target = hfst_path.with_extension("thfst");
    println!(
        "Converting {:?} to {:?}...",
        hfst_path.file_name().unwrap_or_default(),
        target.file_name().unwrap_or_default()
    );

    thfst::ThfstTransducer::convert_file(&transducer, hfst_path)
        .with_context(|| format!("failed to write THFST output for '{}'", hfst_path.display()))?;
    Ok(())
}

fn convert_thfsts_to_bhfst(
    acceptor_path: &Path,
    errmodel_path: &Path,
    output_path: &Path,
) -> anyhow::Result<()> {
    let fs = divvun_fst::vfs::Fs;
    let _acceptor_transducer = MmapThfstTransducer::from_path(&fs, acceptor_path)
        .with_context(|| format!("failed to load acceptor '{}'", acceptor_path.display()))?;
    let _errmodel_transducer = MmapThfstTransducer::from_path(&fs, errmodel_path)
        .with_context(|| format!("failed to load errmodel '{}'", errmodel_path.display()))?;

    let mut boxfile = BoxWriter::create_with_alignment(output_path, ALIGNMENT)
        .with_context(|| format!("failed to create box archive '{}'", output_path.display()))?;

    insert_thfst_files(&mut boxfile, acceptor_path)?;
    insert_thfst_files(&mut boxfile, errmodel_path)?;

    boxfile.finish().context("failed to finalise box archive")?;
    Ok(())
}

fn convert_zhfst_to_bhfst(zhfst_path: &Path) -> anyhow::Result<()> {
    let zhfst_path = std::fs::canonicalize(zhfst_path)
        .with_context(|| format!("failed to resolve '{}'", zhfst_path.display()))?;
    let zhfst = ZipSpellerArchive::open(&zhfst_path)
        .with_context(|| format!("failed to open ZHFST archive '{}'", zhfst_path.display()))?;

    let dir = tempfile::tempdir().context("failed to create temporary directory")?;
    println!(
        "Unzipping {:?} to temporary directory...",
        zhfst_path.file_name().unwrap_or_default()
    );
    std::process::Command::new("unzip")
        .current_dir(&dir)
        .args([&zhfst_path])
        .output()
        .with_context(|| format!("failed to run unzip on '{}'", zhfst_path.display()))?;

    let bhfst_path = zhfst_path.with_extension("bhfst");
    let mut boxfile = BoxWriter::create_with_alignment(&bhfst_path, ALIGNMENT)
        .with_context(|| format!("failed to create box archive '{}'", bhfst_path.display()))?;

    let meta_json = match zhfst.metadata() {
        Some(metadata) => {
            println!("Converting \"index.xml\" to \"meta.json\"...");
            let mut m = metadata.to_owned();
            m.acceptor_mut()
                .set_id(metadata.acceptor().id().replace(".hfst", ".thfst"));
            m.errmodel_mut()
                .set_id(metadata.errmodel().id().replace(".hfst", ".thfst"));
            Some(serde_json::to_string_pretty(&m).context("failed to serialise meta.json")?)
        }
        None => None,
    };

    let acceptor_path = dir.as_ref().join("acceptor.default.hfst");
    convert_hfst_to_thfst(&acceptor_path)?;
    insert_thfst_files(&mut boxfile, &acceptor_path.with_extension("thfst"))?;

    let errmodel_path = dir.as_ref().join("errmodel.default.hfst");
    convert_hfst_to_thfst(&errmodel_path)?;
    insert_thfst_files(&mut boxfile, &errmodel_path.with_extension("thfst"))?;

    if let Some(v) = meta_json {
        println!("Inserting \"meta.json\"...");
        boxfile
            .insert(
                &CompressionConfig::new(Compression::Stored),
                BoxPath::new("meta.json").context("failed to construct meta.json path")?,
                std::io::Cursor::new(v.into_bytes()),
                BoxHashMap::new(),
            )
            .context("failed to insert meta.json")?;
    }

    boxfile.finish().context("failed to finalise bhfst")?;
    println!("Wrote to {:?}.", bhfst_path);

    Ok(())
}

fn run() -> anyhow::Result<()> {
    let opts = Opts::parse();

    match opts {
        Opts::HfstToThfst { from } => convert_hfst_to_thfst(&from),
        Opts::ThfstsToBhfst {
            acceptor,
            errmodel,
            output,
        } => convert_thfsts_to_bhfst(&acceptor, &errmodel, &output),
        Opts::ZhfstToBhfst { from } => convert_zhfst_to_bhfst(&from),
        Opts::BhfstInfo { path } => {
            let ar: ThfstBoxSpellerArchive = BoxSpellerArchive::open(&path)
                .with_context(|| format!("failed to open BHFST archive '{}'", path.display()))?;
            match ar.metadata() {
                Some(md) => println!("{md:#?}"),
                None => bail!("archive '{}' contains no metadata", path.display()),
            }
            Ok(())
        }
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
