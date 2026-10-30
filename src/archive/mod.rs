//! Handling of archives of spell-checking models.
#[cfg(feature = "fs")]
use memmap2::Mmap;
#[cfg(feature = "fs")]
use std::ffi::OsString;
use std::{path::Path, sync::Arc};

pub mod boxf;
pub mod error;
pub mod meta;
pub mod zip;

#[cfg(feature = "fs")]
use self::boxf::ThfstChunkedBoxSpellerArchive;
use self::{boxf::ThfstBytesBoxSpellerArchive, meta::SpellerMetadata};
use crate::{
    archive::{error::SpellerArchiveError, zip::ZipSpellerArchive},
    speller::{Speller, SpellerConfig},
    transducer::TransducerFormat,
    vfs::Bytes,
};

/// Top-level, one-line hint printed by CLIs when an error chain indicates the
/// archive itself is corrupt or built in an incompatible way. Stable text so
/// scripts/tests can grep for it.
pub const REPORT_BUG_HINT: &str = "hint: this looks like a corrupt or incompatible archive. Please file an issue at https://github.com/divvun/divvunspell/issues and attach the archive if possible.";

/// The ZHFST member carrying the archive's own [`SpellerConfig`].
///
/// A speller's runtime parameters are measured against the language it ships
/// for, so they belong with it rather than in every client that loads it. The
/// member is deliberately not referenced from `index.xml`: readers that know
/// nothing about it — hfst-ospell among them — pass over an unexpected member
/// and carry on, so an archive carrying one stays readable everywhere.
///
/// The name must never begin with `acceptor.` or `errmodel.`, which is what
/// makes it unmistakably not a transducer.
pub const BUNDLED_CONFIG_MEMBER: &str = "speller-config.json";

/// The `meta.json` key carrying a BHFST archive's [`SpellerConfig`].
///
/// A box archive has no spare member to hide a config in the way a zip does, so
/// it rides along in the metadata the archive already carries.
pub const BUNDLED_CONFIG_KEY: &str = "spellerConfig";

/// Parse a bundled config, falling back to the built-in defaults when it is
/// malformed.
///
/// A typo in a config must never brick spelling in production: the archive is
/// otherwise sound, and a speller that refuses to run is worse than one running
/// on defaults. Build-time validation is where a bad config gets caught; the
/// library only has to survive one.
pub(crate) fn parse_bundled_config(bytes: &[u8], archive: &Path) -> Option<SpellerConfig> {
    match serde_json::from_slice(bytes) {
        Ok(config) => Some(config),
        Err(source) => {
            warn_malformed_config(archive, &source);
            None
        }
    }
}

/// Report a bundled config that could not be read, naming the archive it came
/// from so the warning is actionable without a debugger.
pub(crate) fn warn_malformed_config(archive: &Path, source: &dyn std::fmt::Display) {
    tracing::warn!(
        "ignoring malformed bundled config in archive '{}' ({}); using built-in defaults",
        archive.display(),
        source
    );
}

/// Which archive member a speller's error model was read from, and in what
/// format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrmodelSource {
    /// the archive member
    pub location: String,
    /// the format it was read in
    pub format: String,
}

impl std::fmt::Display for ErrmodelSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "member {} ({})", self.location, self.format)
    }
}

impl ErrmodelSource {
    pub(crate) fn new(location: impl Into<String>, format: TransducerFormat) -> Self {
        ErrmodelSource {
            location: location.into(),
            format: format.to_string(),
        }
    }
}

#[cfg(feature = "fs")]
pub(crate) struct TempMmap {
    mmap: Arc<Mmap>,

    // Not really dead, needed to drop when TempMmap drops
    _tempdir: tempfile::TempDir,
}

#[cfg(feature = "fs")]
pub(crate) enum MmapRef {
    Direct(Arc<Mmap>),
    Temp(TempMmap),
}

#[cfg(feature = "fs")]
impl MmapRef {
    pub fn map(&self) -> Arc<Mmap> {
        match self {
            MmapRef::Direct(mmap) => Arc::clone(mmap),
            MmapRef::Temp(tmmap) => Arc::clone(&tmmap.mmap),
        }
    }
}

/// Speller archive is a file read into spell-checker with metadata.
pub trait SpellerArchive {
    /// Read and parse a speller archive.
    #[cfg(feature = "fs")]
    fn open(path: &Path) -> Result<Self, SpellerArchiveError>
    where
        Self: Sized;

    /// Retrieve spell-checker.
    ///
    /// The returned speller can perform both spell checking and morphological analysis
    /// depending on the `OutputMode` passed to `suggest()`.
    fn speller(&self) -> Arc<dyn Speller + Send + Sync>;

    /// Retrieve metadata.
    fn metadata(&self) -> Option<&SpellerMetadata>;

    /// The configuration the archive bundles, if it bundles one.
    ///
    /// The speller returned by [`Self::speller`] already runs with it wherever
    /// a call names no config of its own; this is for callers that need to say
    /// which configuration a result was produced under.
    fn bundled_config(&self) -> Option<&SpellerConfig> {
        None
    }

    /// Where the speller's error model came from, and in what format.
    fn errmodel_source(&self) -> Option<&ErrmodelSource> {
        None
    }
}

/// Reads a speller archive.
#[cfg(feature = "fs")]
pub fn open<P>(path: P) -> Result<Arc<dyn SpellerArchive + Send + Sync>, SpellerArchiveError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    match path.extension() {
        Some(x) if x == "bhfst" => {
            ThfstChunkedBoxSpellerArchive::open(path).map(|x| Arc::new(x) as _)
        }
        Some(x) if x == "zhfst" => ZipSpellerArchive::open(path).map(|x| Arc::new(x) as _),
        unknown => Err(SpellerArchiveError::UnsupportedExt {
            path: path.to_path_buf(),
            ext: unknown.map(|x| x.to_owned()).unwrap_or_else(OsString::new),
        }),
    }
}

/// The first bytes of a box archive.
const BOX_MAGIC: &[u8] = b"\xffBOX";
/// The first bytes of a zip archive with members, and of an empty one.
const ZIP_MAGICS: [&[u8]; 2] = [b"PK\x03\x04", b"PK\x05\x06"];

/// Reads a speller archive held in memory: a BHFST or a ZHFST archive, told
/// apart by its first bytes rather than by a name.
///
/// `path` names the archive in errors. See
/// [`BoxSpellerArchive::from_bytes`](boxf::BoxSpellerArchive::from_bytes) and
/// [`ZipSpellerArchive::from_bytes`] for what each reads in place.
pub fn from_bytes(
    bytes: impl Into<Bytes>,
    path: impl AsRef<Path>,
) -> Result<Arc<dyn SpellerArchive + Send + Sync>, SpellerArchiveError> {
    let bytes = bytes.into();
    let path = path.as_ref();
    if bytes.starts_with(BOX_MAGIC) {
        return ThfstBytesBoxSpellerArchive::from_bytes(bytes, path).map(|x| Arc::new(x) as _);
    }
    if ZIP_MAGICS.iter().any(|magic| bytes.starts_with(magic)) {
        return ZipSpellerArchive::from_bytes(bytes, path).map(|x| Arc::new(x) as _);
    }
    Err(SpellerArchiveError::UnrecognisedArchive {
        path: path.to_path_buf(),
        start: bytes
            .iter()
            .take(4)
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
    })
}
