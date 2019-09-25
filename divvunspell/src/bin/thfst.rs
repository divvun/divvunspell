use structopt::StructOpt;
use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{prelude::*, BufWriter};

use byteorder::{LittleEndian, WriteBytesExt};

use divvunspell::transducer::{
    hfst::{
        self,
        HfstTransducer
    },
    thfst::{
        self,
        ThfstTransducer
    },
    convert::ConvertFile
};

#[derive(Debug, StructOpt)]
#[structopt(name = "thfst", about = "Tromsø-Helsinki Finite State Transducer toolkit.")]
enum Opts {
    #[structopt(help = "Convert an HFST file to THFST")]
    ToThfst {
        #[structopt(parse(from_os_str))]
        from: PathBuf
    },

    #[structopt(help = "Convert a ZHFST file to BHFST")]
    ToBhfst {
        #[structopt(parse(from_os_str))]
        from: PathBuf
    }
}

fn convert_hfst_to_thfst(hfst_path: &Path) -> Result<(), std::io::Error> {
    let transducer = HfstTransducer::from_path(hfst_path)?;
    thfst::ThfstTransducer::convert_file(&transducer, hfst_path)?;
    Ok(())
}


fn main() -> Result<(), std::io::Error> {
    let opts = Opts::from_args();
    
    match opts {
        Opts::ToThfst { from } => {
            convert_hfst_to_thfst(&from)
        },
        _ => Ok(())
    }
}