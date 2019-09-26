use crate::transducer::TransducerError;
use std::fmt;
use std::io::Error;

#[derive(Debug)]
pub enum SpellerArchiveError {
    File(Error),
    Io(Error),
    Transducer(TransducerError),
    UnsupportedCompressed,
    Unknown(u8),
}

impl SpellerArchiveError {
    pub fn into_io_error(self) -> Error {
        match self {
            SpellerArchiveError::File(e) => e,
            SpellerArchiveError::Io(e) => e,
            SpellerArchiveError::Transducer(e) => e.into_io_error(),
            SpellerArchiveError::UnsupportedCompressed => {
                Error::new(std::io::ErrorKind::Other, "unsupported compression")
            }
            SpellerArchiveError::Unknown(n) => {
                Error::new(std::io::ErrorKind::Other, format!("unknown: {}", n))
            }
        }
    }
}

impl std::error::Error for SpellerArchiveError {}

impl fmt::Display for SpellerArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{:?}", self)
    }
}
