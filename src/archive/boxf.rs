//! Box-based archive stuff.
use std::sync::Arc;

use box_format::sync::BoxReader as BoxFileReader;

use super::error::SpellerArchiveError;
use super::{SpellerArchive, meta::SpellerMetadata};
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::{
    Transducer,
    thfst::{MmapThfstTransducer, chunked::MmapThfstChunkedTransducer},
};
use crate::vfs::Filesystem;
use crate::vfs::boxf::Filesystem as BoxFilesystem;

/// An archive with mmaped language and error model THFST automata archive.
pub type ThfstBoxSpellerArchive = BoxSpellerArchive<MmapThfstTransducer, MmapThfstTransducer>;

/// An archive with mmaped chunked language and error model THFST automata
/// file.
pub type ThfstChunkedBoxSpeller =
    HfstSpeller<MmapThfstChunkedTransducer, MmapThfstChunkedTransducer>;

/// An archive with mmaped language and error model THFST automata file.
pub type ThfstBoxSpeller = HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>;

/// An archive with mmaped chunked language and error model THFST automata
/// archive.
pub type ThfstChunkedBoxSpellerArchive =
    BoxSpellerArchive<MmapThfstChunkedTransducer, MmapThfstChunkedTransducer>;

/// The one key this loader reads from `meta.json` beyond [`SpellerMetadata`]:
/// the archive's bundled [`SpellerConfig`], under
/// [`BUNDLED_CONFIG_KEY`](super::BUNDLED_CONFIG_KEY).
///
/// Kept apart from `SpellerMetadata` because that type is also the shape of a
/// ZHFST `index.xml`, where the config lives in a member of its own. The config
/// is held as a raw value so that a malformed one warns and falls back to the
/// built-in defaults instead of failing the whole archive.
#[derive(serde::Deserialize)]
struct MetaJsonExtras {
    #[serde(
        default,
        rename = "spellerConfig",
        alias = "speller-config",
        alias = "speller_config"
    )]
    speller_config: Option<serde_json::Value>,
}

/// Read the bundled [`SpellerConfig`] out of an already-read `meta.json`.
fn read_bundled_config(meta_json: &[u8], path: &std::path::Path) -> Option<SpellerConfig> {
    let extras: MetaJsonExtras = match serde_json::from_slice(meta_json) {
        Ok(extras) => extras,
        Err(source) => {
            super::warn_malformed_config(path, &source);
            return None;
        }
    };

    match extras.speller_config.map(serde_json::from_value) {
        None => None,
        Some(Ok(config)) => Some(config),
        Some(Err(source)) => {
            super::warn_malformed_config(path, &source);
            None
        }
    }
}

/// Speller in box archive.
pub struct BoxSpellerArchive<T, U>
where
    T: Transducer,
    U: Transducer,
{
    metadata: Option<SpellerMetadata>,
    speller: Arc<HfstSpeller<T, U>>,
}

impl<T, U> BoxSpellerArchive<T, U>
where
    T: Transducer + Send + Sync + 'static,
    U: Transducer + Send + Sync + 'static,
{
    /// get the spell-checking component
    pub fn hfst_speller(&self) -> Arc<HfstSpeller<T, U>> {
        self.speller.clone()
    }
}

impl<T, U> SpellerArchive for BoxSpellerArchive<T, U>
where
    T: Transducer
        + crate::transducer::TransducerLoader<crate::vfs::boxf::File>
        + Send
        + Sync
        + 'static,
    U: Transducer
        + crate::transducer::TransducerLoader<crate::vfs::boxf::File>
        + Send
        + Sync
        + 'static,
{
    fn open(file_path: &std::path::Path) -> Result<BoxSpellerArchive<T, U>, SpellerArchiveError> {
        let archive = BoxFileReader::open(file_path).map_err(|e| SpellerArchiveError::Open {
            path: file_path.to_path_buf(),
            source: std::io::Error::other(e),
        })?;

        let fs = BoxFilesystem::new(&archive);

        let meta_json = match fs.open_file("meta.json") {
            Ok(mut f) => {
                use std::io::Read as _;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)
                    .map_err(|source| SpellerArchiveError::Io {
                        archive: file_path.to_path_buf(),
                        member: "meta.json".into(),
                        source,
                    })?;
                Some(buf)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(SpellerArchiveError::Io {
                    archive: file_path.to_path_buf(),
                    member: "meta.json".into(),
                    source,
                });
            }
        };

        let (metadata, bundled_config) = match meta_json {
            Some(buf) => {
                let metadata = serde_json::from_slice(&buf).map_err(|e| {
                    SpellerArchiveError::MetadataJson {
                        archive: file_path.to_path_buf(),
                        source: crate::util::JsonParseError::new(e, &buf),
                    }
                })?;
                (Some(metadata), read_bundled_config(&buf, file_path))
            }
            None => (None, None),
        };
        let errmodel = T::from_path(&fs, "errmodel.default.thfst").map_err(|source| {
            SpellerArchiveError::Transducer {
                archive: file_path.to_path_buf(),
                member: "errmodel.default.thfst".into(),
                source,
            }
        })?;
        let acceptor = U::from_path(&fs, "acceptor.default.thfst").map_err(|source| {
            SpellerArchiveError::Transducer {
                archive: file_path.to_path_buf(),
                member: "acceptor.default.thfst".into(),
                source,
            }
        })?;

        let speller = HfstSpeller::new_with_bundled_config(errmodel, acceptor, bundled_config);
        Ok(BoxSpellerArchive { speller, metadata })
    }

    fn speller(&self) -> Arc<dyn Speller + Send + Sync> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        self.metadata.as_ref()
    }

    fn bundled_config(&self) -> Option<&SpellerConfig> {
        self.speller.bundled_config()
    }
}

#[cfg(test)]
mod tests {
    use super::super::BUNDLED_CONFIG_KEY;
    use super::*;

    fn meta_json(speller_config: &str) -> Vec<u8> {
        format!(
            r#"{{"info": {{}}, "acceptor": {{}}, "{}": {}}}"#,
            BUNDLED_CONFIG_KEY, speller_config
        )
        .into_bytes()
    }

    fn read(speller_config: &str) -> Option<SpellerConfig> {
        read_bundled_config(
            &meta_json(speller_config),
            std::path::Path::new("test.bhfst"),
        )
    }

    #[test]
    fn metadata_without_the_key_bundles_nothing() {
        let without = br#"{"info": {}, "acceptor": {}}"#;
        assert!(read_bundled_config(without, std::path::Path::new("test.bhfst")).is_none());
    }

    #[test]
    fn the_key_is_read_as_a_speller_config() {
        let config = read(r#"{"n-best": 100, "beam": 14}"#).expect("bundled config");
        assert_eq!(config.n_best, Some(100));
        assert_eq!(config.beam, Some(crate::types::Weight(14.0)));
        assert_eq!(config.max_weight, SpellerConfig::default().max_weight);
    }

    #[test]
    fn a_malformed_config_falls_back_to_defaults() {
        assert!(read(r#"{"n-best": "ten"}"#).is_none());
        assert!(read("42").is_none());
    }
}
