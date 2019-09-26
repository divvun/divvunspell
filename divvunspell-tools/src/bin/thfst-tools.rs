use std::path::{Path, PathBuf};
use structopt::StructOpt;

use divvunspell::archive::{BoxSpellerArchive, ZipSpellerArchive};
use divvunspell::transducer::{
    convert::ConvertFile,
    hfst::HfstTransducer,
    thfst::{self, ThfstTransducer},
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
    }
}

use std::num::NonZeroU64;

const ALIGNMENT: NonZeroU64 = unsafe { std::num::NonZeroU64::new_unchecked(8) };

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
    boxfile.mkdir(
        BoxPath::new(path.file_name().unwrap()).unwrap(),
        std::collections::HashMap::new(),
    )?;
    insert(boxfile, Compression::Stored, path, "alphabet")?;
    insert(boxfile, Compression::Stored, path, "index")?;
    insert(boxfile, Compression::Stored, path, "transition")
}

fn convert_hfst_to_thfst(hfst_path: &Path) -> Result<(), std::io::Error> {
    let fs = divvunspell::util::Fs;
    let transducer = HfstTransducer::from_path(&fs, hfst_path).map_err(|e| e.into_io_error())?;
    thfst::ThfstTransducer::convert_file(&transducer, hfst_path)?;
    Ok(())
}

fn convert_thfsts_to_bhfst(
    acceptor_path: &Path,
    errmodel_path: &Path,
    output_path: &Path,
) -> Result<(), std::io::Error> {
    let fs = divvunspell::util::Fs;
    let _acceptor_transducer =
        ThfstTransducer::from_path(&fs, acceptor_path).map_err(|e| e.into_io_error())?;
    let _errmodel_transducer =
        ThfstTransducer::from_path(&fs, errmodel_path).map_err(|e| e.into_io_error())?;

    let mut boxfile: BoxFileWriter = BoxFileWriter::create_with_alignment(output_path, ALIGNMENT)?;

    insert_thfst_files(&mut boxfile, acceptor_path)?;
    insert_thfst_files(&mut boxfile, errmodel_path)?;

    Ok(())
}

fn convert_zhfst_to_bhfst(zhfst_path: &Path) -> Result<(), std::io::Error> {
    let zhfst_path = std::fs::canonicalize(zhfst_path)?;
    let zhfst = ZipSpellerArchive::open(&zhfst_path).map_err(|e| e.into_io_error())?;

    let meta_json = match zhfst.metadata() {
        Some(metadata) => {
            let mut metadata = metadata.to_owned();
            metadata.acceptor.id = metadata.acceptor.id.replace(".hfst", ".thfst");
            metadata.errmodel.id = metadata.errmodel.id.replace(".hfst", ".thfst");
            Some(serde_json::to_string_pretty(&metadata)?)
        }
        None => None
    };
    
    let dir = tempdir::TempDir::new("zhfst")?;

    std::process::Command::new("unzip")
        .current_dir(&dir)
        .args(&[&zhfst_path])
        .output()?;


    let acceptor_path = dir.as_ref().join("acceptor.default.hfst");
    let errmodel_path = dir.as_ref().join("errmodel.default.hfst");
    convert_hfst_to_thfst(&acceptor_path)?;
    convert_hfst_to_thfst(&errmodel_path)?;

    let bhfst_path = zhfst_path.with_extension("bhfst");
    let mut boxfile: BoxFileWriter = BoxFileWriter::create_with_alignment(bhfst_path, ALIGNMENT)?;

    if let Some(v) = meta_json {
        boxfile.insert(
            Compression::Stored,
            BoxPath::new("meta.json").unwrap(),
            &mut std::io::Cursor::new(v),
            std::collections::HashMap::new(),
        )?;
    }

    insert_thfst_files(&mut boxfile, &acceptor_path.with_extension("thfst"))?;
    insert_thfst_files(&mut boxfile, &errmodel_path.with_extension("thfst"))?;

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
            let ar: BoxSpellerArchive<ThfstTransducer, ThfstTransducer> =
                BoxSpellerArchive::open(&path).map_err(|e| e.into_io_error())?;
            println!("{:#?}", ar.metadata());
            Ok(())
        }
    }
}
