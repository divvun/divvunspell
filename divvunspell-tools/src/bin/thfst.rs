use std::path::{Path, PathBuf};
use structopt::StructOpt;

use divvunspell::transducer::{
    convert::ConvertFile,
    hfst::{self, HfstTransducer},
    thfst::{self, ThfstTransducer},
};

use box_format::{BoxFileWriter, BoxPath, Compression};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "thfst",
    about = "Tromsø-Helsinki Finite State Transducer toolkit."
)]
enum Opts {
    #[structopt(help = "Convert an HFST file to THFST")]
    HfstToThfst {
        #[structopt(parse(from_os_str))]
        from: PathBuf,
    },

    #[structopt(help = "Convert a ZHFST file to BHFST")]
    ZhfstToBhfst {
        #[structopt(parse(from_os_str))]
        from: PathBuf,

        #[structopt(parse(from_os_str))]
        output: PathBuf,
    },

    ThfstsToBhfst {
        #[structopt(parse(from_os_str))]
        acceptor: PathBuf,

        #[structopt(parse(from_os_str))]
        errmodel: PathBuf,

        #[structopt(parse(from_os_str))]
        output: PathBuf,
    },
}

use std::num::NonZeroU64;

const ALIGNMENT: NonZeroU64 = unsafe { std::num::NonZeroU64::new_unchecked(8) };

fn convert_hfst_to_thfst(hfst_path: &Path) -> Result<(), std::io::Error> {
    let fs = divvunspell::util::Fs;
    let transducer = HfstTransducer::from_path(&fs, hfst_path)?;
    thfst::ThfstTransducer::convert_file(&transducer, hfst_path)?;
    Ok(())
}

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

fn convert_thfsts_to_bhfst(
    acceptor_path: &Path,
    errmodel_path: &Path,
    output_path: &Path,
) -> Result<(), std::io::Error> {
    let fs = divvunspell::util::Fs;
    let _acceptor_transducer = ThfstTransducer::from_path(&fs, acceptor_path)?;
    let _errmodel_transducer = ThfstTransducer::from_path(&fs, errmodel_path)?;

    let mut boxfile: BoxFileWriter = BoxFileWriter::create_with_alignment(output_path, ALIGNMENT)?;

    // pub fn insert<R: Read>(
    //     &mut self,
    //     compression: Compression,
    //     path: BoxPath,
    //     value: &mut R,
    //     attrs: HashMap<String, Vec<u8>>
    //

    insert_thfst_files(&mut boxfile, acceptor_path)?;
    insert_thfst_files(&mut boxfile, errmodel_path)?;

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
        _ => Ok(()),
    }
}
