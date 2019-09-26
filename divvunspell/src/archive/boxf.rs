use std::sync::Arc;

use box_format::BoxFileReader;

use super::error::SpellerArchiveError;
use super::meta::SpellerMetadata;
use crate::speller::Speller;
use crate::transducer::Transducer;
use crate::util::boxf::Filesystem;

pub struct BoxSpellerArchive<T: Transducer, U: Transducer> {
    // metadata: SpellerMetadata,
    speller: Arc<Speller<T, U>>,
}

impl<T: Transducer, U: Transducer> BoxSpellerArchive<T, U> {
    pub fn new(file_path: &str) -> Result<BoxSpellerArchive<T, U>, SpellerArchiveError> {
        let archive =
            BoxFileReader::open(file_path).map_err(SpellerArchiveError::OpenFileFailed)?;

        let fs = Filesystem::new(&archive);

        let errmodel = T::from_path(&fs, "errmodel.default.thfst").unwrap();
        let acceptor = U::from_path(&fs, "acceptor.default.thfst").unwrap();

        let speller = Speller::new(errmodel, acceptor);
        Ok(BoxSpellerArchive { speller })
    }

    pub fn speller(&self) -> Arc<Speller<T, U>> {
        self.speller.clone()
    }

    pub fn metadata(&self) -> &SpellerMetadata {
        // &self.metadata
        unimplemented!()
    }
}
