//! Archive-related errors.
use std::{ffi::OsString, path::PathBuf};

use crate::transducer::TransducerError;

/// Errors that can occur when opening or using a speller archive.
///
/// Every variant names the archive (and, where applicable, the member inside it)
/// and preserves its underlying cause via `#[source]`, so the full chain is
/// walkable with [`std::error::Error::source`] (or via `anyhow::Error`'s
/// `Debug` renderer).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpellerArchiveError {
    /// Failed to open the archive file itself.
    #[error("failed to open archive '{}'", path.display())]
    Open {
        /// archive path
        path: PathBuf,
        /// underlying I/O error
        #[source]
        source: std::io::Error,
    },

    /// I/O error while reading a member from inside the archive.
    #[error("I/O error reading '{member}' in archive '{}'", archive.display())]
    Io {
        /// archive path
        archive: PathBuf,
        /// name of the member inside the archive
        member: String,
        /// underlying I/O error
        #[source]
        source: std::io::Error,
    },

    /// Loading or parsing a transducer from the archive failed.
    #[error("failed to load transducer '{member}' from archive '{}'", archive.display())]
    Transducer {
        /// archive path
        archive: PathBuf,
        /// transducer member being loaded (e.g. `acceptor.default.thfst`)
        member: String,
        /// underlying transducer error
        #[source]
        source: TransducerError,
    },

    /// The archive is missing required metadata (meta.json or index.xml).
    #[error("archive '{}' is missing required metadata (meta.json or index.xml)", path.display())]
    NoMetadata {
        /// archive path
        path: PathBuf,
    },

    /// The archive uses a compression method this loader cannot handle.
    #[error("archive '{}' uses unsupported compression", path.display())]
    UnsupportedCompression {
        /// archive path
        path: PathBuf,
    },

    /// The file does not have a recognised archive extension.
    #[error(
        "unsupported archive extension '{}' for '{}': expected .zhfst or .bhfst",
        ext.to_string_lossy(),
        path.display()
    )]
    UnsupportedExt {
        /// archive path
        path: PathBuf,
        /// the extension that was present (empty if none)
        ext: OsString,
    },

    /// Error reading the underlying zip container.
    #[error("failed to read zip archive '{}'", path.display())]
    Zip {
        /// archive path
        path: PathBuf,
        /// underlying zip error
        #[source]
        source: ::zip::result::ZipError,
    },

    /// Parsing the `index.xml` metadata in a ZHFST archive failed.
    #[error("failed to parse index.xml in archive '{}'", archive.display())]
    MetadataXml {
        /// archive path
        archive: PathBuf,
        /// underlying XML parse error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Parsing the `meta.json` metadata in a BHFST archive failed.
    #[error("failed to parse meta.json in archive '{}'", archive.display())]
    MetadataJson {
        /// archive path
        archive: PathBuf,
        /// JSON parse error with a source-snippet at the failure location
        #[source]
        source: crate::util::JsonParseError,
    },
}
