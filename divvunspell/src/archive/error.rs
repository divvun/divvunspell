//! Archive-related errors.
use std::{ffi::OsString, io::Error};

use crate::transducer::TransducerError;

/// Errors that can occur when opening or using a speller archive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpellerArchiveError {
    /// Error opening or reading the archive file
    #[error("File error")]
    File(#[source] Error),

    /// I/O error while reading archive contents
    #[error("IO error")]
    Io(String, #[source] eieio::Error),

    /// Error loading or parsing a transducer from the archive
    #[error("Transducer error")]
    Transducer(#[source] TransducerError),

    /// Archive is missing required metadata
    #[error("Missing metadata")]
    NoMetadata,

    /// Archive uses unsupported compression
    #[error("Unsupported compression")]
    UnsupportedCompressed,

    /// Unknown error code encountered
    #[error("Unknown error code {0}")]
    Unknown(u8),

    /// File has an unsupported extension (expected .zhfst or .bhfst)
    #[error("Unsupported file extension: {0:?}")]
    UnsupportedExt(OsString),
}
