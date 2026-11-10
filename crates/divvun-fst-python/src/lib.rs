//! The pyo3 glue behind `divvun-fst`, divvunspell for Python.
//!
//! maturin builds this crate as the extension module `divvun_fst._divvun_fst`.
//! The `divvun_fst` package in `python/divvun_fst/` re-exports it, with the
//! config dataclasses beside it, and is the public API, which mirrors the
//! npm package `@divvun/fst` in Python's idioms.

mod config;
mod error;
mod log;
mod record;
mod speller;
mod tokenizer;
mod transducer;

/// divvunspell for Python: loads BHFST and ZHFST spellers and checks and
/// corrects words with them, and looks words up in optimized-lookup, THFST
/// and DHFST transducers.
#[pyo3::pymodule(name = "_divvun_fst")]
mod module {
    use pyo3::prelude::*;
    use pyo3::types::PyTuple;

    #[pymodule_export]
    use crate::config::{
        builtin_speller_config, check_reweighting_config, check_speller_config,
        resolve_speller_config,
    };
    #[pymodule_export]
    use crate::error::FstError;
    #[pymodule_export]
    use crate::speller::{
        AcceptorMetadata, ErrorModelMetadata, Speller, SpellerInfo, SpellerMetadata, SpellerTitle,
        Suggestion, WeightDetails, load_speller, read_speller,
    };
    #[pymodule_export]
    use crate::tokenizer::{WordContext, cursor_context, word_bound_indices, word_indices};
    #[pymodule_export]
    use crate::transducer::{LookupResult, Transducer, load_transducer, read_transducer};

    #[pymodule_init]
    fn init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        // `TransducerFormat` and `FstErrorCode` are the `typing.Literal`s of
        // the names the module gives, made from the tables it gives them
        // from, so the two cannot disagree.
        let literal = m.py().import("typing")?.getattr("Literal")?;
        let formats = PyTuple::new(m.py(), crate::transducer::FORMATS)?;
        m.add("TransducerFormat", literal.get_item(formats)?)?;
        let codes = PyTuple::new(m.py(), crate::error::CODES)?;
        m.add("FstErrorCode", literal.get_item(codes)?)
    }
}
