//! Box-based archive stuff.
use std::sync::Arc;

use box_format::sync::BoxReader as BoxFileReader;

use super::error::SpellerArchiveError;
use super::{
    DHFST_FORMAT, ErrmodelSource, OpenOptions, SpellerArchive, meta::SpellerMetadata,
    readable_dhfst_version,
};
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::{
    ErrorModel, Transducer,
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

/// The THFST error-model member every BHFST archive has carried.
pub const THFST_ERRMODEL_MEMBER: &str = "errmodel.default.thfst";

/// The member a BHFST archive carries a compact DHFST error model in, unless
/// its `meta.json` names another.
pub const DHFST_ERRMODEL_MEMBER: &str = "errmodel.default.dhfst";

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
///
/// The error model is the THFST member [`THFST_ERRMODEL_MEMBER`], unless
/// `meta.json` declares `"format": "dhfst"` on its `errmodel`, in which case it
/// is the single-file member the `errmodel` `id` names, read in place. The
/// member's header, not its name, decides how it is read.
pub struct BoxSpellerArchive<T, U>
where
    T: Transducer,
    U: Transducer,
{
    metadata: Option<SpellerMetadata>,
    speller: Arc<dyn Speller + Send + Sync>,
    typed: Option<Arc<HfstSpeller<T, U>>>,
    bundled_config: Option<SpellerConfig>,
    errmodel_source: ErrmodelSource,
}

impl<T, U> BoxSpellerArchive<T, U>
where
    T: Transducer + Send + Sync + 'static,
    U: Transducer + Send + Sync + 'static,
{
    /// get the spell-checking component, when its error model is the
    /// archive's THFST one; `None` when it was read in another format
    pub fn hfst_speller(&self) -> Option<Arc<HfstSpeller<T, U>>> {
        self.typed.clone()
    }
}

impl<T, U> BoxSpellerArchive<T, U>
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
    /// Open an archive, with the error model chosen by `options`.
    pub fn open_with(
        file_path: &std::path::Path,
        options: &OpenOptions,
    ) -> Result<BoxSpellerArchive<T, U>, SpellerArchiveError> {
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
                let metadata: SpellerMetadata = serde_json::from_slice(&buf).map_err(|e| {
                    SpellerArchiveError::MetadataJson {
                        archive: file_path.to_path_buf(),
                        source: crate::util::JsonParseError::new(e, &buf),
                    }
                })?;
                (Some(metadata), read_bundled_config(&buf, file_path))
            }
            None => (None, None),
        };
        let acceptor = U::from_path(&fs, "acceptor.default.thfst").map_err(|source| {
            SpellerArchiveError::Transducer {
                archive: file_path.to_path_buf(),
                member: "acceptor.default.thfst".into(),
                source,
            }
        })?;

        // A compact error model is one file, read by its header; a THFST one
        // is the directory every BHFST has carried.
        let compact_member = metadata
            .as_ref()
            .map(|m| m.errmodel())
            .filter(|e| {
                !options.primary_errmodel_only
                    && e.format() == Some(DHFST_FORMAT)
                    && readable_dhfst_version(e.format_version().unwrap_or("1"))
            })
            .map(|e| {
                if e.id().is_empty() {
                    DHFST_ERRMODEL_MEMBER.to_string()
                } else {
                    e.id().to_string()
                }
            });

        let external = match &options.errmodel_path {
            Some(path) => Some((
                path.display().to_string(),
                ErrorModel::from_path(&crate::vfs::Fs, path).map_err(|source| {
                    SpellerArchiveError::Transducer {
                        archive: file_path.to_path_buf(),
                        member: path.display().to_string(),
                        source,
                    }
                })?,
            )),
            None => None,
        };

        let (speller, typed, errmodel_source): (Arc<dyn Speller + Send + Sync>, _, _) =
            match (external, compact_member) {
                (Some((location, errmodel)), _) => {
                    let source = ErrmodelSource::new(location, true, errmodel.format());
                    let speller: Arc<dyn Speller + Send + Sync> = match errmodel {
                        ErrorModel::Hfst(errmodel) => HfstSpeller::new_with_bundled_config(
                            errmodel,
                            acceptor,
                            bundled_config.clone(),
                        ),
                        ErrorModel::Dhfst(errmodel) => HfstSpeller::new_with_bundled_config(
                            errmodel,
                            acceptor,
                            bundled_config.clone(),
                        ),
                    };
                    (speller, None, source)
                }
                (None, Some(member)) => {
                    let errmodel = ErrorModel::from_path(&fs, &member).map_err(|source| {
                        SpellerArchiveError::Transducer {
                            archive: file_path.to_path_buf(),
                            member: member.clone(),
                            source,
                        }
                    })?;
                    let source = ErrmodelSource::new(member.clone(), false, errmodel.format());
                    let speller: Arc<dyn Speller + Send + Sync> = match errmodel {
                        ErrorModel::Dhfst(errmodel) => HfstSpeller::new_with_bundled_config(
                            errmodel,
                            acceptor,
                            bundled_config.clone(),
                        ),
                        ErrorModel::Hfst(errmodel) => HfstSpeller::new_with_bundled_config(
                            errmodel,
                            acceptor,
                            bundled_config.clone(),
                        ),
                    };
                    (speller, None, source)
                }
                (None, None) => {
                    let errmodel = T::from_path(&fs, THFST_ERRMODEL_MEMBER).map_err(|source| {
                        SpellerArchiveError::Transducer {
                            archive: file_path.to_path_buf(),
                            member: THFST_ERRMODEL_MEMBER.into(),
                            source,
                        }
                    })?;
                    let speller = HfstSpeller::new_with_bundled_config(
                        errmodel,
                        acceptor,
                        bundled_config.clone(),
                    );
                    let source = ErrmodelSource {
                        location: THFST_ERRMODEL_MEMBER.into(),
                        external: false,
                        format: "THFST".into(),
                    };
                    (
                        speller.clone() as Arc<dyn Speller + Send + Sync>,
                        Some(speller),
                        source,
                    )
                }
            };

        Ok(BoxSpellerArchive {
            metadata,
            speller,
            typed,
            bundled_config,
            errmodel_source,
        })
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
        BoxSpellerArchive::open_with(file_path, &OpenOptions::default())
    }

    fn speller(&self) -> Arc<dyn Speller + Send + Sync> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        self.metadata.as_ref()
    }

    fn bundled_config(&self) -> Option<&SpellerConfig> {
        self.bundled_config.as_ref()
    }

    fn errmodel_source(&self) -> Option<&ErrmodelSource> {
        Some(&self.errmodel_source)
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

    mod dhfst_member {
        use super::super::super::OpenOptions;
        use super::*;
        use crate::speller::suggestion::Suggestion;
        use crate::transducer::TransducerLoader;
        use crate::transducer::dhfst::writer::{SourceModel, WriteOptions, write};
        use box_format::{
            BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter,
        };
        use std::path::{Path, PathBuf};

        fn fixture(name: &str) -> PathBuf {
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
        }

        fn dhfst_mutator() -> Vec<u8> {
            let thfst = MmapThfstTransducer::from_path(&crate::vfs::Fs, fixture("mutator.thfst"))
                .expect("fixture loads");
            let names = thfst
                .alphabet()
                .key_table()
                .iter()
                .map(|k| k.to_string())
                .collect();
            let model = SourceModel::from_transducer(&thfst, names).expect("fixture reads");
            write(
                &model,
                &WriteOptions {
                    threads: 1,
                    ..WriteOptions::default()
                },
            )
            .expect("fixture writes")
            .bytes
        }

        fn insert_thfst(boxfile: &mut BoxWriter, source: &Path, name: &str) {
            boxfile
                .mkdir(BoxPath::new(name).expect("box path"), BoxHashMap::new())
                .expect("mkdir");
            for component in ["alphabet", "index", "transition"] {
                let file = std::fs::File::open(source.join(component)).expect("open component");
                boxfile
                    .insert(
                        &CompressionConfig::new(Compression::Stored),
                        BoxPath::new(Path::new(name).join(component)).expect("box path"),
                        std::io::BufReader::new(file),
                        BoxHashMap::new(),
                    )
                    .expect("insert component");
            }
        }

        /// A BHFST with the fixture lexicon, the fixture error model both as
        /// THFST and as DHFST, and a `meta.json` whose `errmodel` carries
        /// `extra`.
        fn bhfst(dir: &Path, extra: &str) -> PathBuf {
            let path = dir.join("dhfst.bhfst");
            let mut boxfile = BoxWriter::create_with_alignment(&path, 8).expect("create");
            insert_thfst(
                &mut boxfile,
                &fixture("lexicon.thfst"),
                "acceptor.default.thfst",
            );
            insert_thfst(
                &mut boxfile,
                &fixture("mutator.thfst"),
                THFST_ERRMODEL_MEMBER,
            );
            boxfile
                .insert(
                    &CompressionConfig::new(Compression::Stored),
                    BoxPath::new(DHFST_ERRMODEL_MEMBER).expect("box path"),
                    std::io::Cursor::new(dhfst_mutator()),
                    BoxHashMap::new(),
                )
                .expect("insert DHFST");
            let meta = format!(
                r#"{{"info": {{"locale": "se", "title": [], "description": "", "producer": ""}},
                    "acceptor": {{"type": "general", "id": "acceptor.default.thfst", "title": [], "description": ""}},
                    "errmodel": {{"id": "{DHFST_ERRMODEL_MEMBER}", "title": [], "description": ""{extra}}}}}"#
            );
            boxfile
                .insert(
                    &CompressionConfig::new(Compression::Stored),
                    BoxPath::new("meta.json").expect("box path"),
                    std::io::Cursor::new(meta.into_bytes()),
                    BoxHashMap::new(),
                )
                .expect("insert meta.json");
            boxfile.finish().expect("finish");
            path
        }

        fn suggestions(archive: &ThfstBoxSpellerArchive) -> Vec<Vec<(String, u32)>> {
            let mut config = SpellerConfig::default();
            config.n_best = None;
            ["kat", "cet", "car", "cäät", "katt"]
                .iter()
                .map(|word| {
                    let s: Vec<Suggestion> = archive.speller().suggest_with_config(word, &config);
                    s.into_iter()
                        .map(|s| (s.value.to_string(), s.weight.0.to_bits()))
                        .collect()
                })
                .collect()
        }

        #[test]
        fn the_format_key_selects_the_dhfst_member() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = bhfst(dir.path(), r#", "format": "dhfst", "formatVersion": "1""#);

            let dhfst = ThfstBoxSpellerArchive::open(&path).expect("opens");
            let source = dhfst.errmodel_source().expect("a source");
            assert_eq!(source.location, DHFST_ERRMODEL_MEMBER);
            assert_eq!(source.format, "DHFST version 1");
            assert!(dhfst.hfst_speller().is_none());

            let thfst = ThfstBoxSpellerArchive::open_with(
                &path,
                &OpenOptions {
                    primary_errmodel_only: true,
                    ..OpenOptions::default()
                },
            )
            .expect("opens");
            assert_eq!(
                thfst.errmodel_source().map(|s| s.location.as_str()),
                Some(THFST_ERRMODEL_MEMBER)
            );
            assert!(thfst.hfst_speller().is_some());

            let from_dhfst = suggestions(&dhfst);
            assert!(from_dhfst.iter().any(|s| !s.is_empty()));
            assert_eq!(from_dhfst, suggestions(&thfst));
        }

        #[test]
        fn without_the_key_the_thfst_member_is_read() {
            let dir = tempfile::tempdir().expect("tempdir");
            for (i, extra) in ["", r#", "format": "dhfst", "formatVersion": "3""#]
                .iter()
                .enumerate()
            {
                let sub = dir.path().join(i.to_string());
                std::fs::create_dir(&sub).expect("mkdir");
                let path = bhfst(&sub, extra);
                let archive = ThfstBoxSpellerArchive::open(&path).expect("opens");
                assert_eq!(
                    archive.errmodel_source().map(|s| s.location.as_str()),
                    Some(THFST_ERRMODEL_MEMBER),
                    "{extra}"
                );
            }
        }
    }
}
