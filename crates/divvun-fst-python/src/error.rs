//! The exceptions the library's failures reach Python as.

use std::path::{Path, PathBuf};

use divvun_fst::archive::error::SpellerArchiveError;
use divvun_fst::transducer::TransducerError;
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyOSError, PyValueError};
use pyo3::prelude::*;

create_exception!(
    divvun_fst,
    FstError,
    PyException,
    "A failure divvun-fst reports, such as bytes that are no speller archive or a \
     broken transducer. `code` says which kind, as @divvun/fst's FstError does."
);

/// The codes an `FstError` carries. Each failure gets the code @divvun/fst
/// gives it: `archive_code` and `transducer_code` map the library's errors as
/// `crates/divvun-fst-wasm/src/error.rs` does. @divvun/fst's
/// `invalid-argument` has no counterpart, because an argument this package
/// cannot use raises TypeError or ValueError, as any Python function does.
pub const CODES: [&str; 4] = ["unsupported-format", "invalid-format", "internal", "failed"];

/// Why a call made without the GIL failed, kept until the GIL is back to make
/// the exception from.
#[derive(Debug)]
pub enum Failure {
    /// A file that cannot be opened or read, raised as `open()` raises it.
    Os {
        error: std::io::Error,
        path: PathBuf,
    },
    /// What the library reports, raised as an `FstError` with `code`.
    Fst { code: &'static str, message: String },
    /// An argument this input cannot use, such as a box member it lacks.
    Value(String),
}

impl Failure {
    pub fn fst(code: &'static str, message: impl Into<String>) -> Failure {
        debug_assert!(CODES.contains(&code), "{code} is one of CODES");
        Failure::Fst {
            code,
            message: message.into(),
        }
    }

    pub fn os(error: std::io::Error, path: &Path) -> Failure {
        Failure::Os {
            error,
            path: path.to_path_buf(),
        }
    }

    pub fn into_pyerr(self, py: Python<'_>) -> PyErr {
        match self {
            Failure::Os { error, path } => os_error(py, error, &path),
            Failure::Fst { code, message } => fst_error(py, code, message),
            Failure::Value(message) => PyValueError::new_err(message),
        }
    }
}

impl From<SpellerArchiveError> for Failure {
    fn from(error: SpellerArchiveError) -> Self {
        Failure::fst(archive_code(&error), chain(&error))
    }
}

impl From<TransducerError> for Failure {
    fn from(error: TransducerError) -> Self {
        Failure::fst(transducer_code(&error), chain(&error))
    }
}

/// An `FstError` with `code` set on it, which every one this package raises
/// has.
fn fst_error(py: Python<'_>, code: &'static str, message: String) -> PyErr {
    let error = FstError::new_err(message);
    if let Err(failed) = error.value(py).setattr("code", code) {
        return failed;
    }
    error
}

/// An error and every cause under it, each after a colon: the library puts
/// the detail, such as which table is truncated, in the causes.
pub fn chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        let text = next.to_string();
        if !message.ends_with(&text) {
            message.push_str(": ");
            message.push_str(&text);
        }
        cause = next.source();
    }
    message
}

/// The code for an archive error.
fn archive_code(error: &SpellerArchiveError) -> &'static str {
    match error {
        SpellerArchiveError::Transducer { source, .. } => transducer_code(source),
        SpellerArchiveError::UnrecognisedArchive { .. }
        | SpellerArchiveError::UnsupportedCompression { .. }
        | SpellerArchiveError::UnsupportedExt { .. } => "unsupported-format",
        SpellerArchiveError::Zip {
            source: ::zip::result::ZipError::UnsupportedArchive(_),
            ..
        } => "unsupported-format",
        SpellerArchiveError::Open { .. }
        | SpellerArchiveError::Io { .. }
        | SpellerArchiveError::NoMetadata { .. }
        | SpellerArchiveError::Zip { .. }
        | SpellerArchiveError::MetadataXml { .. }
        | SpellerArchiveError::TwoErrorModels { .. }
        | SpellerArchiveError::MetadataJson { .. } => "invalid-format",
        _ => "failed",
    }
}

/// The code for a transducer error. Only a header the reader does not know
/// is an unsupported format: every other failure is in the bytes of a format
/// it reads.
fn transducer_code(error: &TransducerError) -> &'static str {
    match error {
        TransducerError::UnrecognisedFormat { .. } => "unsupported-format",
        TransducerError::Io { .. }
        | TransducerError::MissingComponent { .. }
        | TransducerError::AlphabetJson { .. }
        | TransducerError::AlphabetMalformed { .. }
        | TransducerError::CorruptHeader { .. }
        | TransducerError::CorruptTables { .. }
        | TransducerError::WrongDhfstType { .. } => "invalid-format",
        TransducerError::Memmap { .. } => "internal",
        _ => "failed",
    }
}

/// Fails as `open()` fails on `path`: when there is no file there, it may
/// not be read, or it is a directory.
pub fn readable_file(path: &Path) -> Result<std::fs::File, Failure> {
    let file = std::fs::File::open(path).map_err(|error| Failure::os(error, path))?;
    let metadata = file.metadata().map_err(|error| Failure::os(error, path))?;
    if metadata.is_dir() {
        return Err(Failure::os(
            std::io::Error::from_raw_os_error(libc::EISDIR),
            path,
        ));
    }
    Ok(file)
}

/// The `OSError` `open()` raises for `path`: `OSError` picks the subclass,
/// such as `FileNotFoundError`, from the errno.
fn os_error(py: Python<'_>, error: std::io::Error, path: &Path) -> PyErr {
    let Some(code) = error.raw_os_error() else {
        return error.into();
    };
    let filename = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        // On Windows the code is a Windows error, not an errno. Passed as
        // `winerror`, it is mapped to the errno that picks the subclass, and
        // the errno argument is ignored.
        let message = error.to_string();
        let message = match message.rfind(" (os error ") {
            Some(at) => message[..at].to_owned(),
            None => message,
        };
        return PyOSError::new_err((0, message, filename, code));
    }
    let strerror = py
        .import("os")
        .and_then(|os| os.call_method1("strerror", (code,)))
        .and_then(|s| s.extract::<String>())
        .unwrap_or_else(|_| error.to_string());
    PyOSError::new_err((code, strerror, filename))
}
