//! Zip archive stuff.
use ::zip::{CompressionMethod, ZipArchive};
use memmap2::MmapOptions;
use std::fs::File;
use std::io::Seek;
use std::io::prelude::*;
use std::sync::Arc;

use super::error::SpellerArchiveError;
use super::meta::SpellerMetadata;
use super::{
    BUNDLED_CONFIG_MEMBER, ErrmodelSource, MmapRef, OpenOptions, SpellerArchive, TempMmap,
    parse_bundled_config, readable_variant,
};
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::ErrorModel;
use crate::transducer::dhfst::DhfstTransducer;
use crate::transducer::hfst::HfstTransducer;
use crate::vfs::Fs;

/// Type alias for HFST-based speller loaded from a zip archive.
///
/// Uses memory-mapped HFST transducers for both the error model and lexicon.
pub type HfstZipSpeller = HfstSpeller<HfstTransducer, HfstTransducer>;

/// A speller from a zip archive whose error model is in the compact DHFST
/// format.
pub type DhfstZipSpeller = HfstSpeller<DhfstTransducer, HfstTransducer>;

/// Speller archive backed by a zip file.
///
/// This is the standard format for distributing spell-checkers (`.zhfst` files).
/// The archive contains metadata, an error model transducer, and a lexicon transducer.
///
/// The error model is read from the member `<errmodel id>` names, unless the
/// element also declares a `<variant>` in a format this reader knows (the
/// compact DHFST format) whose member is present, in which case that member is
/// read instead. Readers that know nothing of variants read `id` as always.
/// Either way the member's header, not its name, decides how it is read.
pub struct ZipSpellerArchive {
    metadata: SpellerMetadata,
    speller: Arc<dyn Speller + Send + Sync>,
    hfst_speller: Option<Arc<HfstZipSpeller>>,
    bundled_config: Option<SpellerConfig>,
    errmodel_source: ErrmodelSource,
}

/// Where the error model is to be read from.
enum ErrmodelInput<'a> {
    /// a file outside the archive
    External(&'a std::path::Path),
    /// a member of the archive, mapped
    Member(MmapRef),
}

fn mmap_by_name<R: Read + Seek>(
    zipfile: &mut File,
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<MmapRef, std::io::Error> {
    let mut index = archive.by_name(name)?;

    if index.compression() != CompressionMethod::Stored {
        let tempdir = tempfile::tempdir()?;
        let outpath = tempdir.path().join(index.mangled_name());

        let mut outfile = File::create(&outpath)?;
        std::io::copy(&mut index, &mut outfile)?;

        let outfile = File::open(&outpath)?;

        let mmap = unsafe { MmapOptions::new().map(&outfile) };

        return match mmap {
            Ok(v) => Ok(MmapRef::Temp(TempMmap {
                mmap: Arc::new(v),
                _tempdir: tempdir,
            })),
            Err(err) => return Err(err),
        };
    }

    let mmap = unsafe {
        MmapOptions::new()
            .offset(index.data_start())
            .len(index.size() as usize)
            .map(&*zipfile)
    };

    match mmap {
        Ok(v) => Ok(MmapRef::Direct(Arc::new(v))),
        Err(err) => Err(err),
    }
}

/// Read the archive's bundled [`SpellerConfig`] from
/// [`BUNDLED_CONFIG_MEMBER`], if it carries one.
///
/// Every failure short of a sound config is a `None` with a warning: an archive
/// without the member is the ordinary case, and one whose member cannot be read
/// or parsed still has a perfectly good speller in it.
pub(crate) fn read_bundled_config<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    path: &std::path::Path,
) -> Option<SpellerConfig> {
    if !archive
        .file_names()
        .any(|name| name == BUNDLED_CONFIG_MEMBER)
    {
        return None;
    }

    let mut buf = Vec::new();
    let read = archive
        .by_name(BUNDLED_CONFIG_MEMBER)
        .map_err(|e| e.to_string())
        .and_then(|mut member| member.read_to_end(&mut buf).map_err(|e| e.to_string()));

    if let Err(source) = read {
        super::warn_malformed_config(path, &source);
        return None;
    }

    parse_bundled_config(&buf, path)
}

impl ZipSpellerArchive {
    /// Get a reference to the HFST speller.
    ///
    /// Returns the underlying `HfstSpeller` with its concrete transducer types,
    /// when both transducers are HFST optimized lookup; `None` when the error
    /// model was read in another format.
    pub fn hfst_speller(&self) -> Option<Arc<HfstSpeller<HfstTransducer, HfstTransducer>>> {
        self.hfst_speller.clone()
    }

    /// Open an archive, with the error model chosen by `options`.
    pub fn open_with(
        file_path: &std::path::Path,
        options: &OpenOptions,
    ) -> Result<ZipSpellerArchive, SpellerArchiveError> {
        let file = File::open(file_path).map_err(|source| SpellerArchiveError::Open {
            path: file_path.to_path_buf(),
            source,
        })?;
        let reader = std::io::BufReader::new(&file);
        let mut archive = ZipArchive::new(reader).map_err(|source| SpellerArchiveError::Zip {
            path: file_path.to_path_buf(),
            source,
        })?;

        // // Open file a second time to get around borrow checker
        let mut file = File::open(file_path).map_err(|source| SpellerArchiveError::Open {
            path: file_path.to_path_buf(),
            source,
        })?;

        let metadata_mmap =
            mmap_by_name(&mut file, &mut archive, "index.xml").map_err(|source| {
                SpellerArchiveError::Io {
                    archive: file_path.to_path_buf(),
                    member: "index.xml".into(),
                    source,
                }
            })?;
        let metadata = SpellerMetadata::from_bytes(&metadata_mmap.map()).map_err(|source| {
            SpellerArchiveError::MetadataXml {
                archive: file_path.to_path_buf(),
                source: Box::new(source),
            }
        })?;

        let bundled_config = read_bundled_config(&mut archive, file_path);

        let acceptor_id = metadata.acceptor().id().to_string();
        let errmodel_member = if options.primary_errmodel_only {
            metadata.errmodel().id().to_string()
        } else {
            let present: std::collections::HashSet<&str> = archive.file_names().collect();
            metadata
                .errmodel()
                .variants()
                .iter()
                .find(|v| {
                    readable_variant(&v.format, &v.version) && present.contains(v.id.as_str())
                })
                .map(|v| v.id.clone())
                .unwrap_or_else(|| metadata.errmodel().id().to_string())
        };

        let acceptor_mmap =
            mmap_by_name(&mut file, &mut archive, &acceptor_id).map_err(|source| {
                SpellerArchiveError::Io {
                    archive: file_path.to_path_buf(),
                    member: acceptor_id.clone(),
                    source,
                }
            })?;
        let errmodel_input = match &options.errmodel_path {
            Some(path) => ErrmodelInput::External(path),
            None => ErrmodelInput::Member(
                mmap_by_name(&mut file, &mut archive, &errmodel_member).map_err(|source| {
                    SpellerArchiveError::Io {
                        archive: file_path.to_path_buf(),
                        member: errmodel_member.clone(),
                        source,
                    }
                })?,
            ),
        };
        drop(archive);

        let acceptor =
            HfstTransducer::from_mapped_memory(acceptor_mmap.map(), file_path.join(&acceptor_id))
                .map_err(|source| SpellerArchiveError::Transducer {
                archive: file_path.to_path_buf(),
                member: acceptor_id.clone(),
                source,
            })?;

        let (errmodel, errmodel_source) = match errmodel_input {
            ErrmodelInput::External(path) => {
                let errmodel = ErrorModel::from_path(&Fs, path).map_err(|source| {
                    SpellerArchiveError::Transducer {
                        archive: file_path.to_path_buf(),
                        member: path.display().to_string(),
                        source,
                    }
                })?;
                let source =
                    ErrmodelSource::new(path.display().to_string(), true, errmodel.format());
                (errmodel, source)
            }
            ErrmodelInput::Member(mmap) => {
                let errmodel =
                    ErrorModel::from_mapped_memory(mmap.map(), file_path.join(&errmodel_member))
                        .map_err(|source| SpellerArchiveError::Transducer {
                            archive: file_path.to_path_buf(),
                            member: errmodel_member.clone(),
                            source,
                        })?;
                let source = ErrmodelSource::new(errmodel_member.clone(), false, errmodel.format());
                (errmodel, source)
            }
        };

        let (speller, hfst_speller): (Arc<dyn Speller + Send + Sync>, _) = match errmodel {
            ErrorModel::Hfst(errmodel) => {
                let speller = HfstSpeller::new_with_bundled_config(
                    errmodel,
                    acceptor,
                    bundled_config.clone(),
                );
                (speller.clone(), Some(speller))
            }
            ErrorModel::Dhfst(errmodel) => (
                HfstSpeller::new_with_bundled_config(errmodel, acceptor, bundled_config.clone()),
                None,
            ),
        };

        Ok(ZipSpellerArchive {
            metadata,
            speller,
            hfst_speller,
            bundled_config,
            errmodel_source,
        })
    }
}

impl SpellerArchive for ZipSpellerArchive {
    fn open(file_path: &std::path::Path) -> Result<ZipSpellerArchive, SpellerArchiveError> {
        ZipSpellerArchive::open_with(file_path, &OpenOptions::default())
    }

    fn speller(&self) -> Arc<dyn Speller + Send + Sync> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        Some(&self.metadata)
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
    use super::*;
    use crate::archive::OpenOptions;
    use crate::archive::error::SpellerArchiveError;
    use ::zip::write::{SimpleFileOptions, ZipWriter};
    use std::io::Cursor;

    /// A zip in memory holding exactly the named members.
    fn zip_with(members: &[(&str, &str)]) -> Cursor<Vec<u8>> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, contents) in members {
            writer.start_file(*name, options).expect("start member");
            writer.write_all(contents.as_bytes()).expect("write member");
        }
        writer.finish().expect("finish zip")
    }

    fn read_config(members: &[(&str, &str)]) -> Option<SpellerConfig> {
        let mut archive = ZipArchive::new(zip_with(members)).expect("open zip");
        read_bundled_config(&mut archive, std::path::Path::new("test.zhfst"))
    }

    #[test]
    fn an_archive_without_the_member_bundles_nothing() {
        assert!(read_config(&[("index.xml", "<hfstspeller/>")]).is_none());
    }

    #[test]
    fn the_member_is_read_as_a_speller_config() {
        let config = read_config(&[
            ("index.xml", "<hfstspeller/>"),
            (
                BUNDLED_CONFIG_MEMBER,
                r#"{"n-best": 100, "beam": 14, "reweight": {"start-penalty": 3, "mid-penalty": 1, "end-penalty": 1}}"#,
            ),
        ])
        .expect("bundled config");

        assert_eq!(config.n_best, Some(100));
        assert_eq!(config.beam, Some(crate::types::Weight(14.0)));
        let reweight = config.reweight.expect("reweight");
        assert_eq!(reweight.start_penalty, 3.0);
        assert_eq!(reweight.mid_penalty, 1.0);
        assert_eq!(reweight.end_penalty, 1.0);
        // Unnamed fields keep their defaults rather than being zeroed.
        assert_eq!(config.max_weight, SpellerConfig::default().max_weight);
    }

    #[test]
    fn snake_case_keys_are_accepted_too() {
        let config =
            read_config(&[(BUNDLED_CONFIG_MEMBER, r#"{"n_best": 42}"#)]).expect("bundled config");
        assert_eq!(config.n_best, Some(42));
    }

    #[test]
    fn a_malformed_member_falls_back_to_defaults() {
        // Neither a broken document nor a well-formed one with a nonsense value
        // may take the speller down with it.
        assert!(read_config(&[(BUNDLED_CONFIG_MEMBER, "{ not json")]).is_none());
        assert!(read_config(&[(BUNDLED_CONFIG_MEMBER, r#"{"n-best": "ten"}"#)]).is_none());
    }

    mod dhfst_members {
        use super::*;
        use crate::archive::DHFST_FORMAT;
        use crate::speller::suggestion::Suggestion;
        use crate::transducer::dhfst::writer::{SourceModel, WriteOptions, write};
        use crate::transducer::hfst::test_support;

        fn map(bytes: &[u8]) -> Arc<memmap2::Mmap> {
            let mut map = memmap2::MmapMut::map_anon(bytes.len()).expect("anonymous map");
            map.copy_from_slice(bytes);
            Arc::new(map.make_read_only().expect("read-only map"))
        }

        fn dhfst_errmodel() -> Vec<u8> {
            let ol = HfstTransducer::from_mapped_memory(map(&test_support::errmodel()), "e")
                .expect("the test error model loads");
            let names = test_support::SYMBOLS
                .iter()
                .map(|s| s.to_string())
                .collect();
            let model = SourceModel::from_transducer(&ol, names).expect("it reads");
            write(
                &model,
                &WriteOptions {
                    threads: 1,
                    ..WriteOptions::default()
                },
            )
            .expect("it writes")
            .bytes
        }

        fn index_xml(errmodel_id: &str, variant: &str) -> String {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<hfstspeller dtdversion="1.0" hfstversion="3">
  <info><locale>se</locale><title>t</title><description>d</description><producer>p</producer></info>
  <acceptor type="general" id="acceptor.default.hfst"><title>a</title><description>a</description></acceptor>
  <errmodel id="{errmodel_id}">
    <title>e</title>
    <description>e</description>
    {variant}
  </errmodel>
</hfstspeller>"#
            )
        }

        fn variant(version: &str, id: &str) -> String {
            format!(r#"<variant format="{DHFST_FORMAT}" version="{version}" id="{id}"/>"#)
        }

        /// A ZHFST in `dir` with `index.xml` and the given members, the DHFST
        /// member stored and aligned so that it is read in place.
        fn zhfst(
            dir: &std::path::Path,
            xml: &str,
            members: &[(&str, Vec<u8>)],
        ) -> std::path::PathBuf {
            let path = dir.join(format!("test-{}.zhfst", members.len()));
            let mut writer = ZipWriter::new(File::create(&path).expect("create zip"));
            writer
                .start_file("index.xml", SimpleFileOptions::default())
                .expect("start index.xml");
            writer.write_all(xml.as_bytes()).expect("write index.xml");
            for (name, bytes) in members {
                let options = if name.ends_with(".dhfst") {
                    SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Stored)
                        .with_alignment(8)
                } else {
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
                };
                writer.start_file(*name, options).expect("start member");
                writer.write_all(bytes).expect("write member");
            }
            writer.finish().expect("finish zip");
            path
        }

        fn suggestions(archive: &ZipSpellerArchive) -> Vec<(String, Vec<(String, u32)>)> {
            let mut config = SpellerConfig::default();
            config.n_best = None;
            [
                "cat", "cet", "kat", "crt", "carte", "ct", "caat", "rac", "cae",
            ]
            .iter()
            .map(|word| {
                let s: Vec<Suggestion> = archive.speller().suggest_with_config(word, &config);
                (
                    word.to_string(),
                    s.into_iter()
                        .map(|s| (s.value.to_string(), s.weight.0.to_bits()))
                        .collect(),
                )
            })
            .collect()
        }

        fn standard_members() -> Vec<(&'static str, Vec<u8>)> {
            vec![
                ("acceptor.default.hfst", test_support::lexicon()),
                ("errmodel.default.hfst", test_support::errmodel()),
                ("errmodel.default.dhfst", dhfst_errmodel()),
            ]
        }

        #[test]
        fn a_readable_variant_is_read_instead_of_the_primary_member() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = zhfst(
                dir.path(),
                &index_xml(
                    "errmodel.default.hfst",
                    &variant("1", "errmodel.default.dhfst"),
                ),
                &standard_members(),
            );

            let dhfst = ZipSpellerArchive::open(&path).expect("opens");
            let source = dhfst.errmodel_source().expect("a source");
            assert_eq!(source.location, "errmodel.default.dhfst");
            assert_eq!(source.format, "DHFST version 1");
            assert!(dhfst.hfst_speller().is_none());

            let primary = ZipSpellerArchive::open_with(
                &path,
                &OpenOptions {
                    primary_errmodel_only: true,
                    ..OpenOptions::default()
                },
            )
            .expect("opens");
            assert_eq!(
                primary.errmodel_source().map(|s| s.location.as_str()),
                Some("errmodel.default.hfst")
            );
            assert!(primary.hfst_speller().is_some());

            let from_dhfst = suggestions(&dhfst);
            assert!(from_dhfst.iter().any(|(_, s)| !s.is_empty()));
            assert_eq!(from_dhfst, suggestions(&primary));
        }

        #[test]
        fn a_variant_this_reader_cannot_use_is_passed_over() {
            let dir = tempfile::tempdir().expect("tempdir");
            for declared in [
                variant("2", "errmodel.default.dhfst"),
                variant("1", "errmodel.missing.dhfst"),
                r#"<variant format="other" version="1" id="errmodel.default.dhfst"/>"#.to_string(),
                String::new(),
            ] {
                let path = zhfst(
                    dir.path(),
                    &index_xml("errmodel.default.hfst", &declared),
                    &standard_members(),
                );
                let archive = ZipSpellerArchive::open(&path).expect("opens");
                assert_eq!(
                    archive.errmodel_source().map(|s| s.location.as_str()),
                    Some("errmodel.default.hfst"),
                    "{declared}"
                );
            }
        }

        #[test]
        fn a_member_is_read_by_its_header_not_its_name() {
            let dir = tempfile::tempdir().expect("tempdir");
            // An archive for new readers only: `<errmodel id>` names the DHFST
            // member and there is no optimized-lookup one.
            let path = zhfst(
                dir.path(),
                &index_xml("errmodel.default.dhfst", ""),
                &[
                    ("acceptor.default.hfst", test_support::lexicon()),
                    ("errmodel.default.dhfst", dhfst_errmodel()),
                ],
            );
            let archive = ZipSpellerArchive::open(&path).expect("opens");
            assert_eq!(
                archive.errmodel_source().map(|s| s.format.as_str()),
                Some("DHFST version 1")
            );

            // A member whose header is neither format is refused, whatever
            // its name says.
            let path = zhfst(
                dir.path(),
                &index_xml("errmodel.default.hfst", ""),
                &[
                    ("acceptor.default.hfst", test_support::lexicon()),
                    ("errmodel.default.hfst", b"NOTAFST\0 at all".to_vec()),
                ],
            );
            assert!(matches!(
                ZipSpellerArchive::open(&path),
                Err(SpellerArchiveError::Transducer {
                    source: crate::transducer::TransducerError::UnrecognisedFormat { .. },
                    ..
                })
            ));
        }

        #[test]
        fn an_external_error_model_replaces_the_archives() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = zhfst(
                dir.path(),
                &index_xml("errmodel.default.hfst", ""),
                &[
                    ("acceptor.default.hfst", test_support::lexicon()),
                    ("errmodel.default.hfst", test_support::errmodel()),
                ],
            );
            let external = dir.path().join("external.dhfst");
            std::fs::write(&external, dhfst_errmodel()).expect("write");
            let archive = ZipSpellerArchive::open_with(
                &path,
                &OpenOptions {
                    errmodel_path: Some(external.clone()),
                    ..OpenOptions::default()
                },
            )
            .expect("opens");
            let source = archive.errmodel_source().expect("a source");
            assert!(source.external);
            assert_eq!(source.location, external.display().to_string());
            assert_eq!(
                suggestions(&archive),
                suggestions(&ZipSpellerArchive::open(&path).expect("opens"))
            );
        }
    }
}
