use std::fmt;
use std::io::Error;
use crate::transducer::TransducerError;

#[derive(Debug)]
pub enum SpellerArchiveError {
    File(Error),
    Io(Error),
    Transducer(TransducerError),
    UnsupportedCompressed,
    Unknown(u8),
}

impl std::error::Error for SpellerArchiveError {}

impl fmt::Display for SpellerArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{:?}", self)
    }
}
