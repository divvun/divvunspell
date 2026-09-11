//! Handling of archives of spell-checking models.
use memmap2::Mmap;
use std::{ffi::OsString, path::Path, sync::Arc};

pub mod boxf;
pub mod error;
pub mod meta;
pub mod zip;

use self::{boxf::ThfstChunkedBoxSpellerArchive, meta::SpellerMetadata};
use crate::{
    archive::{error::SpellerArchiveError, zip::ZipSpellerArchive},
    speller::{Speller, SpellerConfig},
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

pub(crate) struct TempMmap {
    mmap: Arc<Mmap>,

    // Not really dead, needed to drop when TempMmap drops
    _tempdir: tempfile::TempDir,
}

pub(crate) enum MmapRef {
    Direct(Arc<Mmap>),
    Temp(TempMmap),
}

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
}

/// Reads a speller archive.
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
