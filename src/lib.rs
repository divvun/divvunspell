#![feature(arbitrary_self_types)]

#[macro_use]
extern crate serde_derive;
extern crate serde_xml_rs;
extern crate libc;
extern crate memmap;
extern crate byteorder;
extern crate zip;

#[macro_use]
extern crate log;
extern crate env_logger;

// #[macro_use]
// extern crate lazy_static;

pub mod archive;
pub mod c_api;
pub mod constants;
pub mod transducer;
pub mod types;
pub mod speller;
pub mod tokenizer;

use lazy_static::lazy_static;

// use std::sync::Mutex;

use std::sync::Mutex;
use std::collections::HashMap;

lazy_static! {
    pub static ref COUNTER: Mutex<HashMap<&'static str, u32>> = Mutex::new(HashMap::new());
}

#[test]
fn test_speller() {
    // use std::fs::File;
    // use std::io::BufReader;
    // use std::io::prelude::*;
    // use crate::speller::Speller;
    // use crate::transducer::Transducer;

    // // use COUNTER;

    // let acceptor = File::open("./test-align.hfst").unwrap();
    // let mut acceptor_buf = vec![];
    // let _ = BufReader::new(acceptor).read_to_end(&mut acceptor_buf);

    // let errmodel = File::open("./se/errmodel.default.hfst").unwrap();
    // let mut errmodel_buf = vec![];
    // let _ = BufReader::new(errmodel).read_to_end(&mut errmodel_buf);

    // let lexicon = Transducer::from_bytes(&acceptor_buf);
    // let mutator = Transducer::from_bytes(&errmodel_buf);

    // println!("{:#?}", lexicon);

    // let speller = Speller::new(mutator, lexicon);

    
    // println!("{:?}", speller.suggest("nuvviDspeller"));

    // println!("{:?}", *COUNTER.lock().unwrap());
}
