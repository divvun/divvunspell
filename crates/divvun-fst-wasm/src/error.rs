//! Errors on their way to JavaScript.

use divvun_fst::archive::error::SpellerArchiveError;
use divvun_fst::transducer::TransducerError;
use wasm_bindgen::JsValue;

/// An error for the wrapper: a stable code and the message. Every export that
/// can fail returns it, and it reaches JavaScript as a plain object with
/// `code` and `message`, which the wrapper rethrows as an `FstError`.
#[derive(Debug)]
pub struct GlueError {
    code: &'static str,
    message: String,
}

impl GlueError {
    /// An error with one of the codes `js/fst/errors.ts` lists.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        GlueError {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// An error and every cause under it, each after a colon: the library puts
/// the detail, such as which table is truncated, in the causes.
fn chain(error: &dyn std::error::Error) -> String {
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

impl From<SpellerArchiveError> for GlueError {
    fn from(error: SpellerArchiveError) -> Self {
        GlueError::new(archive_code(&error), chain(&error))
    }
}

impl From<TransducerError> for GlueError {
    fn from(error: TransducerError) -> Self {
        GlueError::new(transducer_code(&error), chain(&error))
    }
}

impl From<GlueError> for JsValue {
    fn from(error: GlueError) -> Self {
        let object = js_sys::Object::new();
        set(&object, "code", &JsValue::from_str(error.code));
        set(&object, "message", &JsValue::from_str(&error.message));
        object.into()
    }
}

fn set(object: &js_sys::Object, key: &str, value: &JsValue) {
    js_sys::Reflect::set(object, &JsValue::from_str(key), value)
        .expect("a fresh plain object takes any property");
}

/// The wrapper's code for an archive error. `js/fst/errors.ts` documents
/// each code beside `FstErrorCode`, and a test here checks that it does.
pub fn archive_code(error: &SpellerArchiveError) -> &'static str {
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

/// The wrapper's code for a transducer error. Only a header the reader does
/// not know is an unsupported format: every other failure is in the bytes of
/// a format it reads.
pub fn transducer_code(error: &TransducerError) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn archive_errors_map_to_their_codes() {
        let path = || PathBuf::from("speller");
        let cases = [
            (
                SpellerArchiveError::UnrecognisedArchive {
                    path: path(),
                    start: "00 01".into(),
                },
                "unsupported-format",
            ),
            (
                SpellerArchiveError::NoMetadata { path: path() },
                "invalid-format",
            ),
            (
                SpellerArchiveError::Transducer {
                    archive: path(),
                    member: "acceptor.default.thfst".into(),
                    source: TransducerError::UnrecognisedFormat {
                        path: path(),
                        detail: "no".into(),
                    },
                },
                "unsupported-format",
            ),
            (
                SpellerArchiveError::Transducer {
                    archive: path(),
                    member: "acceptor.default.thfst".into(),
                    source: TransducerError::CorruptHeader {
                        path: path(),
                        offset: 3,
                    },
                },
                "invalid-format",
            ),
            (
                SpellerArchiveError::Zip {
                    path: path(),
                    source: ::zip::result::ZipError::UnsupportedArchive("bzip2"),
                },
                "unsupported-format",
            ),
            (
                SpellerArchiveError::Zip {
                    path: path(),
                    source: ::zip::result::ZipError::InvalidArchive("no end".into()),
                },
                "invalid-format",
            ),
        ];
        for (error, code) in cases {
            assert_eq!(archive_code(&error), code, "{error}");
        }
    }

    #[test]
    fn the_message_carries_every_cause() {
        let error = GlueError::from(SpellerArchiveError::Transducer {
            archive: PathBuf::from("speller"),
            member: "acceptor.default.thfst".into(),
            source: TransducerError::MissingComponent {
                path: PathBuf::from("acceptor.default.thfst"),
                component: "alphabet",
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
            },
        });
        assert_eq!(error.code(), "invalid-format");
        assert_eq!(
            error.message(),
            "failed to load transducer 'acceptor.default.thfst' from archive 'speller': \
             required transducer component 'alphabet' missing in 'acceptor.default.thfst': not found"
        );
    }

    #[test]
    fn the_wrapper_documents_every_code() {
        let errors_ts = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../js/fst/errors.ts"
        ))
        .expect("js/fst/errors.ts is in the repository");
        let codes = [
            "invalid-argument",
            "unsupported-format",
            "invalid-format",
            "internal",
            "failed",
        ];
        for code in codes {
            assert!(
                errors_ts.contains(&format!("\"{code}\"")),
                "js/fst/errors.ts lists {code}"
            );
        }
    }
}
