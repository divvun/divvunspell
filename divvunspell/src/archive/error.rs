use crate::transducer::TransducerError;
use std::io::Error;

#[derive(Debug, thiserror::Error)]
pub enum SpellerArchiveError {
    #[error("File error")]
    File(Error),

    #[error("IO error")]
    Io(Error),

    #[error("Transducer error")]
    Transducer(TransducerError),

    #[error("Missing metadata")]
    NoMetadata,

    #[error("Unsupported compression")]
    UnsupportedCompressed,

    #[error("Unknown error code {0}")]
    Unknown(u8),
}
