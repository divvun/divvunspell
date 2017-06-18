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

#[test]
fn test_load_zhfst() {
    let zhfst = archive::SpellerArchive::new("./sma-store.zhfst");
    let two = zhfst.speller();
    let res = two.suggest("sami");
}

#[test]
fn test_speller() {
    use transducer::Transducer;
    use speller::Speller;

    let data = vec![];
    let acceptor = Transducer::from_bytes(&data);
    let errmodel = Transducer::from_bytes(&data);

    let speller = Speller::new(acceptor, errmodel);
    println!("{:?}", speller.suggest("sami"));
}
