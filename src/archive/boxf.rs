//! Box-based archive stuff.
use std::sync::Arc;

use box_format::sync::BoxReader as BoxFileReader;

use super::error::SpellerArchiveError;
use super::{SpellerArchive, meta::SpellerMetadata};
use crate::speller::{HfstSpeller, Speller};
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

        let metadata = match fs.open_file("meta.json") {
            Ok(mut f) => {
                use std::io::Read as _;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)
                    .map_err(|source| SpellerArchiveError::Io {
                        archive: file_path.to_path_buf(),
                        member: "meta.json".into(),
                        source,
                    })?;
                Some(serde_json::from_slice(&buf).map_err(|e| {
                    SpellerArchiveError::MetadataJson {
                        archive: file_path.to_path_buf(),
                        source: crate::util::JsonParseError::new(e, &buf),
                    }
                })?)
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

        let speller = HfstSpeller::new(errmodel, acceptor);
        Ok(BoxSpellerArchive { speller, metadata })
    }

    fn speller(&self) -> Arc<dyn Speller + Send + Sync> {
        self.speller.clone()
    }

    fn metadata(&self) -> Option<&SpellerMetadata> {
        self.metadata.as_ref()
    }
}
