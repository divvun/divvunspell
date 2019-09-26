use std::sync::Arc;

use box_format::BoxFileReader;

use crate::speller::Speller;
use crate::transducer::{thfst::ThfstTransducer, Transducer};
use crate::util::boxf::Filesystem;
use super::meta::SpellerMetadata;
use super::error::SpellerArchiveError;

pub struct BoxSpellerArchive<T: Transducer, U: Transducer> {
    // metadata: SpellerMetadata,
    speller: Arc<Speller<T, U>>,
}

impl BoxSpellerArchive<ThfstTransducer, ThfstTransducer> {
    pub fn new(file_path: &str) -> Result<BoxSpellerArchive<ThfstTransducer, ThfstTransducer>, SpellerArchiveError> {
        let archive =
            BoxFileReader::open(file_path).map_err(SpellerArchiveError::OpenFileFailed)?;

        let fs = Filesystem::new(&archive);

        let acceptor = ThfstTransducer::from_path(&fs, "acceptor.default.thfst").unwrap();
        let errmodel = ThfstTransducer::from_path(&fs, "errmodel.default.thfst").unwrap();

        let speller = Speller::new(errmodel, acceptor);
        Ok(BoxSpellerArchive { speller })
    }

    pub fn speller(&self) -> Arc<Speller<ThfstTransducer, ThfstTransducer>> {
        self.speller.clone()
    }

    pub fn metadata(&self) -> &SpellerMetadata {
        // &self.metadata
        unimplemented!()
    }
}
