#[global_allocator]
static GLOBAL: mimallocator::Mimalloc = mimallocator::Mimalloc;

#[macro_use]
extern crate serde_derive;

pub mod archive;
pub mod constants;

#[cfg(feature = "ffi")]
pub mod ffi;

pub mod speller;
pub mod tokenizer;
pub mod transducer;
pub mod types;
pub mod util;
