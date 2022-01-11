use std::path::{Path, PathBuf};
use structopt::StructOpt;

use divvunspell::archive::{
    boxf::ThfstBoxSpellerArchive, BoxSpellerArchive, SpellerArchive, ZipSpellerArchive,
};
use divvunspell::transducer::{
    convert::ConvertFile,
    hfst::HfstTransducer,
    thfst::{self, MemmapThfstTransducer},
    Transducer,
};

use box_format::{BoxFileWriter, BoxPath, Compression};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "thfst-tools",
    about = "Tromsø-Helsinki Finite State Transducer toolkit."
)]
enum Opts {
    #[structopt(about = "Convert an HFST file to THFST")]
    HfstToThfst {
        #[structopt(parse(from_os_str))]
        from: PathBuf,
    },

    #[structopt(about = "Convert a ZHFST file to BHFST")]
    ZhfstToBhfst {
        #[structopt(parse(from_os_str))]
        from: PathBuf,
    },

    #[structopt(about = "Convert a THFST acceptor/errmodel pair to BHFST")]
    ThfstsToBhfst {
        #[structopt(parse(from_os_str))]
        acceptor: PathBuf,

        #[structopt(parse(from_os_str))]
        errmodel: PathBuf,

        #[structopt(parse(from_os_str))]
        output: PathBuf,
    },

    #[structopt(about = "Print metadata for BHFST")]
    BhfstInfo {
        #[structopt(parse(from_os_str))]
        path: PathBuf,
    },
}

const ALIGNMENT: u64 = 8;

#[inline(always)]
fn boxpath(path: &Path, filename: &str) -> BoxPath {
    let path = Path::new(path.file_name().unwrap()).join(filename);
    BoxPath::new(path).unwrap()
}

#[inline(always)]
fn insert(
    boxfile: &mut BoxFileWriter,
    compression: Compression,
    path: &Path,
    name: &str,
) -> Result<(), std::io::Error> {
    use std::collections::HashMap;
    use std::io::BufReader;
    let file = std::fs::File::open(path.join(name))?;
    boxfile
        .insert(
            compression,
            boxpath(path, name),
            &mut BufReader::new(file),
            HashMap::new(),
        )
        .map(|_| ())
}

#[inline(always)]
fn insert_thfst_files(boxfile: &mut BoxFileWriter, path: &Path) -> Result<(), std::io::Error> {
    let boxpath = BoxPath::new(path.file_name().unwrap()).unwrap();
    println!("Inserting \"{}\"...", &boxpath);

    boxfile.mkdir(boxpath, std::collections::HashMap::new())?;
    insert(boxfile, Compression::Stored, path, "alphabet")?;
    insert(boxfile, Compression::Stored, path, "index")?;
    insert(boxfile, Compression::Stored, path, "transition")
}

fn convert_hfst_to_thfst(hfst_path: &Path) -> Result<(), std::io::Error> {
    let fs = divvunspell::vfs::Fs;
    let transducer = HfstTransducer::from_path(&fs, hfst_path).map_err(|e| e.into_io_error())?;
    println!(
        "Converting {:?} to {:?}...",
        &hfst_path.file_name().unwrap(),
        &hfst_path.with_extension("thfst").file_name().unwrap()
    );

    thfst::ThfstTransducer::convert_file(&transducer, hfst_path)?;
    Ok(())
}

fn convert_thfsts_to_bhfst(
    acceptor_path: &Path,
    errmodel_path: &Path,
    output_path: &Path,
) -> Result<(), std::io::Error> {
    let fs = divvunspell::vfs::Fs;
    let _acceptor_transducer =
        MemmapThfstTransducer::from_path(&fs, acceptor_path).map_err(|e| e.into_io_error())?;
    let _errmodel_transducer =
        MemmapThfstTransducer::from_path(&fs, errmodel_path).map_err(|e| e.into_io_error())?;

    let mut boxfile: BoxFileWriter = BoxFileWriter::create_with_alignment(output_path, ALIGNMENT)?;

    insert_thfst_files(&mut boxfile, acceptor_path)?;
    insert_thfst_files(&mut boxfile, errmodel_path)?;

    Ok(())
}

fn convert_zhfst_to_bhfst(zhfst_path: &Path) -> Result<(), std::io::Error> {
    let zhfst_path = std::fs::canonicalize(zhfst_path)?;
    let zhfst = ZipSpellerArchive::open(&zhfst_path).unwrap();

    let dir = tempfile::tempdir()?;
    println!(
        "Unzipping {:?} to temporary directory...",
        zhfst_path.file_name().unwrap()
    );
    std::process::Command::new("unzip")
        .current_dir(&dir)
        .args(&[&zhfst_path])
        .output()?;

    let bhfst_path = zhfst_path.with_extension("bhfst");
    let mut boxfile: BoxFileWriter = BoxFileWriter::create_with_alignment(&bhfst_path, ALIGNMENT)?;

    let meta_json = match zhfst.metadata() {
        Some(metadata) => {
            println!("Converting \"index.xml\" to \"meta.json\"...");
            let mut m = metadata.to_owned();
            m.acceptor.id = metadata.acceptor.id.replace(".hfst", ".thfst");
            m.errmodel.id = metadata.errmodel.id.replace(".hfst", ".thfst");
            Some(serde_json::to_string_pretty(&m)?)
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
        boxfile.insert(
            Compression::Stored,
            BoxPath::new("meta.json").unwrap(),
            &mut std::io::Cursor::new(v),
            std::collections::HashMap::new(),
        )?;
    }

    println!("Wrote to {:?}.", bhfst_path);

    Ok(())
}

fn main() -> Result<(), std::io::Error> {
    let opts = Opts::from_args();

    match opts {
        Opts::HfstToThfst { from } => convert_hfst_to_thfst(&from),
        Opts::ThfstsToBhfst {
            acceptor,
            errmodel,
            output,
        } => convert_thfsts_to_bhfst(&acceptor, &errmodel, &output),
        Opts::ZhfstToBhfst { from } => convert_zhfst_to_bhfst(&from),
        Opts::BhfstInfo { path } => {
            let ar: ThfstBoxSpellerArchive = BoxSpellerArchive::open(&path).unwrap();
            println!("{:#?}", ar.metadata());
            Ok(())
        }
    }
}
