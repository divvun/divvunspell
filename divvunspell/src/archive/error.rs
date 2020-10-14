use crate::transducer::TransducerError;
use std::{ffi::OsString, io::Error};

#[derive(Debug, thiserror::Error)]
pub enum SpellerArchiveError {
    #[error("File error")]
    File(Error),

    #[error("IO error")]
    Io(String, Error),

    #[error("Transducer error")]
    Transducer(TransducerError),

    #[error("Missing metadata")]
    NoMetadata,

    #[error("Unsupported compression")]
    UnsupportedCompressed,

    #[error("Unknown error code {0}")]
    Unknown(u8),

    #[error("Unsupported file extension: {0:?}")]
    UnsupportedExt(OsString),
}
