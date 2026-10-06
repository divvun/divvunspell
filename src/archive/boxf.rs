//! Box-based archive stuff.
use std::sync::Arc;

use box_format::sync::BoxReader as BoxFileReader;

use super::error::SpellerArchiveError;
use super::{ErrmodelSource, SpellerArchive, meta::SpellerMetadata};
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::{
    ErrorModel, Transducer,
    dhfst::acceptor::DhfstAcceptor,
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

/// The single-file error-model member: a BHFST archive that carries it has
/// its error model there, read by the file's header, instead of in
/// [`THFST_ERRMODEL_MEMBER`].
pub const DHFST_ERRMODEL_MEMBER: &str = "errmodel.default.dhfst";

/// The THFST acceptor member every BHFST archive has carried.
pub const THFST_ACCEPTOR_MEMBER: &str = "acceptor.default.thfst";

/// The single-file acceptor member: a BHFST archive that carries it has its
/// acceptor there, as a DHFST acceptor, instead of in
/// [`THFST_ACCEPTOR_MEMBER`].
pub const DHFST_ACCEPTOR_MEMBER: &str = "acceptor.default.dhfst";

/// An acceptor in either of the forms a BHFST archive can hold it in.
enum Acceptor<U> {
    Thfst(U),
    Dhfst(DhfstAcceptor),
}

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
/// An archive carries exactly one error model: the single-file member
/// [`DHFST_ERRMODEL_MEMBER`], read in place by its header (DHFST or HFST
/// optimized lookup), or else the THFST directory [`THFST_ERRMODEL_MEMBER`]
/// every BHFST archive has carried. An archive with both is refused. It
/// carries exactly one acceptor the same way: the DHFST acceptor
/// [`DHFST_ACCEPTOR_MEMBER`], or else the THFST directory
/// [`THFST_ACCEPTOR_MEMBER`].
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
    /// get the spell-checking component, when its error model and its
    /// acceptor are THFST; `None` when the archive carries either in a
    /// single-file member
    pub fn hfst_speller(&self) -> Option<Arc<HfstSpeller<T, U>>> {
        self.typed.clone()
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
        let acceptor = if fs.open_file(DHFST_ACCEPTOR_MEMBER).is_ok() {
            if fs
                .open_file(format!("{THFST_ACCEPTOR_MEMBER}/alphabet"))
                .is_ok()
            {
                return Err(SpellerArchiveError::Io {
                    archive: file_path.to_path_buf(),
                    member: DHFST_ACCEPTOR_MEMBER.into(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "the archive also carries {THFST_ACCEPTOR_MEMBER}; it must carry one acceptor"
                        ),
                    ),
                });
            }
            let acceptor = <DhfstAcceptor as crate::transducer::TransducerLoader<_>>::from_path(
                &fs,
                DHFST_ACCEPTOR_MEMBER,
            )
            .map_err(|source| SpellerArchiveError::Transducer {
                archive: file_path.to_path_buf(),
                member: DHFST_ACCEPTOR_MEMBER.into(),
                source,
            })?;
            Acceptor::Dhfst(acceptor)
        } else {
            let acceptor = U::from_path(&fs, THFST_ACCEPTOR_MEMBER).map_err(|source| {
                SpellerArchiveError::Transducer {
                    archive: file_path.to_path_buf(),
                    member: THFST_ACCEPTOR_MEMBER.into(),
                    source,
                }
            })?;
            Acceptor::Thfst(acceptor)
        };

        let single_file = fs.open_file(DHFST_ERRMODEL_MEMBER).is_ok();
        let thfst = fs
            .open_file(format!("{THFST_ERRMODEL_MEMBER}/alphabet"))
            .is_ok();

        type Shared = Arc<dyn Speller + Send + Sync>;
        let (speller, typed, errmodel_source): (Shared, _, _) = if single_file {
            if thfst {
                return Err(SpellerArchiveError::TwoErrorModels {
                    path: file_path.to_path_buf(),
                });
            }
            let errmodel = ErrorModel::from_path(&fs, DHFST_ERRMODEL_MEMBER).map_err(|source| {
                SpellerArchiveError::Transducer {
                    archive: file_path.to_path_buf(),
                    member: DHFST_ERRMODEL_MEMBER.into(),
                    source,
                }
            })?;
            let source = ErrmodelSource::new(DHFST_ERRMODEL_MEMBER, errmodel.format());
            let config = bundled_config.clone();
            let speller: Shared = match (errmodel, acceptor) {
                (ErrorModel::Dhfst(e), Acceptor::Thfst(a)) => {
                    HfstSpeller::new_with_bundled_config(e, a, config)
                }
                (ErrorModel::Dhfst(e), Acceptor::Dhfst(a)) => {
                    HfstSpeller::new_with_bundled_config(e, a, config)
                }
                (ErrorModel::Hfst(e), Acceptor::Thfst(a)) => {
                    HfstSpeller::new_with_bundled_config(e, a, config)
                }
                (ErrorModel::Hfst(e), Acceptor::Dhfst(a)) => {
                    HfstSpeller::new_with_bundled_config(e, a, config)
                }
            };
            (speller, None, source)
        } else {
            let errmodel = T::from_path(&fs, THFST_ERRMODEL_MEMBER).map_err(|source| {
                SpellerArchiveError::Transducer {
                    archive: file_path.to_path_buf(),
                    member: THFST_ERRMODEL_MEMBER.into(),
                    source,
                }
            })?;
            let source = ErrmodelSource {
                location: THFST_ERRMODEL_MEMBER.into(),
                format: "THFST".into(),
            };
            match acceptor {
                Acceptor::Thfst(acceptor) => {
                    let speller = HfstSpeller::new_with_bundled_config(
                        errmodel,
                        acceptor,
                        bundled_config.clone(),
                    );
                    (speller.clone() as Shared, Some(speller), source)
                }
                Acceptor::Dhfst(acceptor) => {
                    let speller: Shared = HfstSpeller::new_with_bundled_config(
                        errmodel,
                        acceptor,
                        bundled_config.clone(),
                    );
                    (speller, None, source)
                }
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

        /// A BHFST with the fixture lexicon and the fixture error model as
        /// THFST, as a single DHFST file, or both.
        fn bhfst(dir: &Path, thfst: bool, dhfst: bool) -> PathBuf {
            let path = dir.join(format!("{thfst}-{dhfst}.bhfst"));
            let mut boxfile = BoxWriter::create_with_alignment(&path, 8).expect("create");
            insert_thfst(
                &mut boxfile,
                &fixture("lexicon.thfst"),
                "acceptor.default.thfst",
            );
            if thfst {
                insert_thfst(
                    &mut boxfile,
                    &fixture("mutator.thfst"),
                    THFST_ERRMODEL_MEMBER,
                );
            }
            if dhfst {
                boxfile
                    .insert(
                        &CompressionConfig::new(Compression::Stored),
                        BoxPath::new(DHFST_ERRMODEL_MEMBER).expect("box path"),
                        std::io::Cursor::new(dhfst_mutator()),
                        BoxHashMap::new(),
                    )
                    .expect("insert DHFST");
            }
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
        fn a_single_file_error_model_is_read_by_its_header() {
            let dir = tempfile::tempdir().expect("tempdir");
            let dhfst =
                ThfstBoxSpellerArchive::open(&bhfst(dir.path(), false, true)).expect("opens");
            let source = dhfst.errmodel_source().expect("a source");
            assert_eq!(source.location, DHFST_ERRMODEL_MEMBER);
            assert_eq!(source.format, "DHFST error model version 1");
            assert!(dhfst.hfst_speller().is_none());

            let thfst =
                ThfstBoxSpellerArchive::open(&bhfst(dir.path(), true, false)).expect("opens");
            assert_eq!(
                thfst.errmodel_source().map(|s| s.location.as_str()),
                Some(THFST_ERRMODEL_MEMBER)
            );
            assert!(thfst.hfst_speller().is_some());

            let from_dhfst = suggestions(&dhfst);
            assert!(from_dhfst.iter().any(|s| !s.is_empty()));
            assert_eq!(from_dhfst, suggestions(&thfst));
        }

        fn dhfst_lexicon() -> Vec<u8> {
            use crate::transducer::dhfst::acceptor_writer::{AcceptorOptions, write_acceptor};
            let thfst = MmapThfstTransducer::from_path(&crate::vfs::Fs, fixture("lexicon.thfst"))
                .expect("fixture loads");
            let mut names: Vec<String> = thfst
                .alphabet()
                .key_table()
                .iter()
                .map(|k| k.to_string())
                .collect();
            names[0] = "@_EPSILON_SYMBOL_@".to_string();
            write_acceptor(
                &thfst,
                &names,
                &AcceptorOptions {
                    threads: 1,
                    ..AcceptorOptions::default()
                },
            )
            .expect("fixture writes")
            .bytes
        }

        /// A BHFST with the fixture lexicon as THFST, as DHFST, or both, and
        /// the fixture error model as THFST or as DHFST.
        fn with_acceptor(dir: &Path, thfst: bool, dhfst: bool, thfst_errmodel: bool) -> PathBuf {
            let path = dir.join(format!("acceptor-{thfst}-{dhfst}-{thfst_errmodel}.bhfst"));
            let mut boxfile = BoxWriter::create_with_alignment(&path, 8).expect("create");
            if thfst {
                insert_thfst(
                    &mut boxfile,
                    &fixture("lexicon.thfst"),
                    THFST_ACCEPTOR_MEMBER,
                );
            }
            if thfst_errmodel {
                insert_thfst(
                    &mut boxfile,
                    &fixture("mutator.thfst"),
                    THFST_ERRMODEL_MEMBER,
                );
            }
            for (member, bytes) in [
                (DHFST_ACCEPTOR_MEMBER, dhfst.then(dhfst_lexicon)),
                (DHFST_ERRMODEL_MEMBER, (!thfst_errmodel).then(dhfst_mutator)),
            ] {
                if let Some(bytes) = bytes {
                    boxfile
                        .insert(
                            &CompressionConfig::new(Compression::Stored),
                            BoxPath::new(member).expect("box path"),
                            std::io::Cursor::new(bytes),
                            BoxHashMap::new(),
                        )
                        .expect("insert");
                }
            }
            boxfile.finish().expect("finish");
            path
        }

        #[test]
        fn a_dhfst_acceptor_member_is_read_with_either_error_model() {
            let dir = tempfile::tempdir().expect("tempdir");
            let mut want = None;
            for thfst_errmodel in [true, false] {
                let open = |thfst, dhfst| {
                    ThfstBoxSpellerArchive::open(&with_acceptor(
                        dir.path(),
                        thfst,
                        dhfst,
                        thfst_errmodel,
                    ))
                    .expect("opens")
                };
                let thfst = open(true, false);
                assert_eq!(thfst.hfst_speller().is_some(), thfst_errmodel);
                let dhfst = open(false, true);
                assert!(dhfst.hfst_speller().is_none());
                let from_thfst = suggestions(&thfst);
                assert!(from_thfst.iter().any(|s| !s.is_empty()));
                assert_eq!(
                    suggestions(&dhfst),
                    from_thfst,
                    "THFST error model: {thfst_errmodel}"
                );
                assert_eq!(want.get_or_insert(from_thfst), &suggestions(&dhfst));
            }
        }

        #[test]
        fn an_archive_with_two_acceptors_is_refused() {
            let dir = tempfile::tempdir().expect("tempdir");
            for thfst_errmodel in [false, true] {
                assert!(matches!(
                    ThfstBoxSpellerArchive::open(&with_acceptor(
                        dir.path(),
                        true,
                        true,
                        thfst_errmodel
                    )),
                    Err(SpellerArchiveError::Io { member, .. }) if member == DHFST_ACCEPTOR_MEMBER
                ));
            }
        }

        #[test]
        fn an_error_model_in_the_acceptor_member_is_refused() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("swapped.bhfst");
            let mut boxfile = BoxWriter::create_with_alignment(&path, 8).expect("create");
            for member in [DHFST_ACCEPTOR_MEMBER, DHFST_ERRMODEL_MEMBER] {
                boxfile
                    .insert(
                        &CompressionConfig::new(Compression::Stored),
                        BoxPath::new(member).expect("box path"),
                        std::io::Cursor::new(dhfst_mutator()),
                        BoxHashMap::new(),
                    )
                    .expect("insert");
            }
            boxfile.finish().expect("finish");
            assert!(matches!(
                ThfstBoxSpellerArchive::open(&path),
                Err(SpellerArchiveError::Transducer {
                    source: crate::transducer::TransducerError::WrongDhfstType { .. },
                    ..
                })
            ));
        }

        #[test]
        fn an_archive_with_two_error_models_is_refused() {
            let dir = tempfile::tempdir().expect("tempdir");
            assert!(matches!(
                ThfstBoxSpellerArchive::open(&bhfst(dir.path(), true, true)),
                Err(SpellerArchiveError::TwoErrorModels { .. })
            ));
        }
    }
}
