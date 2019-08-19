#[global_allocator]
static GLOBAL: mimallocator::Mimalloc = mimallocator::Mimalloc;

#[macro_use]
extern crate serde_derive;
extern crate byteorder;
extern crate libc;
extern crate memmap;
extern crate serde_xml_rs;
extern crate zip;

pub mod archive;
pub mod constants;
pub mod ffi;
pub mod speller;
pub mod tokenizer;
pub mod transducer;
pub mod types;
