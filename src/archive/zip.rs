//! Zip archive stuff.
use ::zip::{CompressionMethod, ZipArchive};
use memmap2::MmapOptions;
use std::fs::File;
use std::io::Seek;
use std::io::prelude::*;
use std::sync::Arc;

use super::error::SpellerArchiveError;
use super::meta::SpellerMetadata;
use super::{BUNDLED_CONFIG_MEMBER, MmapRef, SpellerArchive, TempMmap, parse_bundled_config};
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::hfst::HfstTransducer;

/// Type alias for HFST-based speller loaded from a zip archive.
///
/// Uses memory-mapped HFST transducers for both the error model and lexicon.
pub type HfstZipSpeller = HfstSpeller<HfstTransducer, HfstTransducer>;

/// Speller archive backed by a zip file.
///
/// This is the standard format for distributing spell-checkers (`.zhfst` files).
/// The archive contains metadata, an error model transducer, and a lexicon transducer.
pub struct ZipSpellerArchive {
    metadata: SpellerMetadata,
    speller: Arc<HfstZipSpeller>,
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
    /// Returns the underlying `HfstSpeller` with its concrete transducer types.
    /// This is useful when you need access to HFST-specific functionality.
    pub fn hfst_speller(&self) -> Arc<HfstSpeller<HfstTransducer, HfstTransducer>> {
        self.speller.clone()
    }
}

impl SpellerArchive for ZipSpellerArchive {
    fn open(file_path: &std::path::Path) -> Result<ZipSpellerArchive, SpellerArchiveError> {
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
        let errmodel_id = metadata.errmodel().id().to_string();

        let acceptor_mmap =
            mmap_by_name(&mut file, &mut archive, &acceptor_id).map_err(|source| {
                SpellerArchiveError::Io {
                    archive: file_path.to_path_buf(),
                    member: acceptor_id.clone(),
                    source,
                }
            })?;
        let errmodel_mmap =
            mmap_by_name(&mut file, &mut archive, &errmodel_id).map_err(|source| {
                SpellerArchiveError::Io {
                    archive: file_path.to_path_buf(),
                    member: errmodel_id.clone(),
                    source,
                }
            })?;
        drop(archive);

        let acceptor =
            HfstTransducer::from_mapped_memory(acceptor_mmap.map(), file_path.join(&acceptor_id))
                .map_err(|source| SpellerArchiveError::Transducer {
                archive: file_path.to_path_buf(),
                member: acceptor_id.clone(),
                source,
            })?;
        let errmodel =
            HfstTransducer::from_mapped_memory(errmodel_mmap.map(), file_path.join(&errmodel_id))
                .map_err(|source| SpellerArchiveError::Transducer {
                archive: file_path.to_path_buf(),
                member: errmodel_id.clone(),
                source,
            })?;

        let speller = HfstSpeller::new_with_bundled_config(errmodel, acceptor, bundled_config);

        Ok(ZipSpellerArchive { metadata, speller })
    }

    fn speller(&self) -> Arc<dyn Speller + Send + Sync> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        Some(&self.metadata)
    }

    fn bundled_config(&self) -> Option<&SpellerConfig> {
        self.speller.bundled_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
