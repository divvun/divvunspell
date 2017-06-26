#[macro_use] extern crate serde_derive;
extern crate serde_xml_rs;
extern crate libc;
extern crate memmap;
extern crate byteorder;
extern crate zip;

pub mod archive;
pub mod constants;
pub mod ffi;
pub mod transducer;
pub mod types;
pub mod speller;

// #[test]
// fn test_load_zhfst() {
//     let zhfst = archive::SpellerArchive::new("./se-store.zhfst");
//     let two = zhfst.speller();
//     let res = two.suggest("sami");
//     println!("{:?}", res);
// }

#[test]
fn test_speller() {
    use std::fs::File;
    use std::io::BufReader;
    use std::io::prelude::*;
    use speller::Speller;
    use transducer::Transducer;

    let acceptor = File::open("./sp/acceptor.default.hfst").unwrap();
    let mut acceptor_buf = vec![];
    let _ = BufReader::new(acceptor).read_to_end(&mut acceptor_buf);

    let errmodel = File::open("./sp/errmodel.default.hfst").unwrap();
    let mut errmodel_buf = vec![];
    let _ = BufReader::new(errmodel).read_to_end(&mut errmodel_buf);

    let mutator = Transducer::from_bytes(&acceptor_buf);
    let lexicon = Transducer::from_bytes(&errmodel_buf);

    let speller = Speller::new(lexicon, mutator);//, lexicon);
    println!("{:?}", speller.suggest("ol"));
}