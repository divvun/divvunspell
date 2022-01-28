use std::{ffi::OsString, io::Error};

#[cfg(feature = "gpt2")]
use rust_bert::RustBertError;

use crate::transducer::TransducerError;

#[derive(Debug, thiserror::Error)]
pub enum SpellerArchiveError {
    #[error("File error")]
    File(#[source] Error),

    #[error("IO error")]
    Io(String, #[source] eieio::Error),

    #[error("Transducer error")]
    Transducer(#[source] TransducerError),

    #[error("Missing metadata")]
    NoMetadata,

    #[error("Unsupported compression")]
    UnsupportedCompressed,

    #[error("Unknown error code {0}")]
    Unknown(u8),

    #[error("Unsupported file extension: {0:?}")]
    UnsupportedExt(OsString),
}

#[derive(Debug, thiserror::Error)]
pub enum PredictorArchiveError {
    #[error("File error")]
    File(#[source] Error),

    #[error("IO error")]
    Io(String, #[source] Error),

    #[cfg(feature = "gpt2")]
    #[error("Error loading bert model")]
    Bert(#[from] RustBertError),

    #[error("Error deserialising JSON")]
    Json(#[from] serde_json::Error),

    #[error("Missing metadata")]
    NoMetadata,

    #[error("Unsupported compression")]
    UnsupportedCompressed,

    #[error("Unknown error code {0}")]
    Unknown(u8),

    #[error("Unsupported file extension: {0:?}")]
    UnsupportedExt(OsString),
}
