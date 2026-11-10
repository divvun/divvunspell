//! Spellers loaded from a speller archive, by path or from its bytes, and
//! what they give.

use std::path::PathBuf;
use std::sync::Arc;

use divvun_fst::archive::{self, SpellerArchive, meta};
use divvun_fst::speller::suggestion;
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;

use crate::config;
use crate::error::{Failure, readable_file};
use crate::log;
use crate::record::record_repr;

/// The name an archive read from bytes goes by in error messages: the bytes
/// have none.
const BYTES_NAME: &str = "speller";

/// A speller loaded from a BHFST or ZHFST archive, with the archive's
/// metadata and bundled config. It is immutable, so threads may use it at
/// the same time, each call without the GIL.
#[pyclass(frozen, module = "divvun_fst")]
pub struct Speller {
    archive: Arc<dyn SpellerArchive + Send + Sync>,
    speller: Arc<dyn divvun_fst::speller::Speller + Send + Sync>,
}

impl Speller {
    fn new(archive: Arc<dyn SpellerArchive + Send + Sync>) -> Speller {
        let speller = archive.speller();
        Speller { archive, speller }
    }

    /// Runs `call` on the speller without the GIL, with the config the call
    /// was passed, or `None` for the speller's own.
    fn run<T: Send>(
        &self,
        py: Python<'_>,
        method: &str,
        config: Option<&Bound<'_, PyAny>>,
        call: impl FnOnce(
            Arc<dyn divvun_fst::speller::Speller + Send + Sync>,
            Option<&divvun_fst::speller::SpellerConfig>,
        ) -> T
        + Send,
    ) -> PyResult<T> {
        let config = config::of_call(config, method)?;
        let speller = Arc::clone(&self.speller);
        log::detached(py, || call(speller, config.as_ref()))
    }
}

#[pymethods]
impl Speller {
    /// The archive's metadata, or None when it carries none, as a BHFST
    /// without meta.json does.
    #[getter]
    fn metadata(&self) -> Option<SpellerMetadata> {
        self.archive.metadata().map(SpellerMetadata::from)
    }

    /// The language the speller is for, from its metadata, or None when it
    /// has none.
    #[getter]
    fn locale(&self) -> Option<String> {
        self.archive
            .metadata()
            .map(|metadata| metadata.info().locale().to_owned())
    }

    /// The config the archive bundles, which a call passed no config runs
    /// with, or None when it bundles none and such a call runs with the
    /// built-in defaults, SpellerConfig().
    #[getter]
    fn bundled_config<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.archive
            .bundled_config()
            .map(|bundled| config::to_python(py, bundled))
            .transpose()
    }

    /// Whether the lexicon accepts `word`, as typed or, with `recase`, in
    /// the lower-case and other case variants divvunspell tries. A word with
    /// no letters, the empty string among them, is correct.
    #[pyo3(signature = (word, /, config = None))]
    fn is_correct(
        &self,
        py: Python<'_>,
        word: &str,
        config: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        self.run(py, "is_correct", config, |speller, config| match config {
            Some(config) => speller.is_correct_with_config(word, config),
            None => speller.is_correct(word),
        })
    }

    /// Corrections for `word`, best first, as divvunspell's suggestion
    /// search finds them.
    #[pyo3(signature = (word, /, config = None))]
    fn suggest(
        &self,
        py: Python<'_>,
        word: &str,
        config: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Vec<Suggestion>> {
        let found = self.run(py, "suggest", config, |speller, config| match config {
            Some(config) => speller.suggest_with_config(word, config),
            None => speller.suggest(word),
        })?;
        Ok(found.iter().map(Suggestion::from).collect())
    }

    /// The lexicon's analyses of `word` as typed, with the lexicon's tags:
    /// no error model, no corrections. A speller whose lexicon is a plain
    /// word list has no tags to give.
    #[pyo3(signature = (word, /, config = None))]
    fn analyze_input(
        &self,
        py: Python<'_>,
        word: &str,
        config: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Vec<Suggestion>> {
        let found = self.run(
            py,
            "analyze_input",
            config,
            |speller, config| match config {
                Some(config) => speller.analyze_input_with_config(word, config),
                None => speller.analyze_input(word),
            },
        )?;
        Ok(found.iter().map(Suggestion::from).collect())
    }

    /// The suggestions for `word`, each with the lexicon's tags left in,
    /// best first.
    #[pyo3(signature = (word, /, config = None))]
    fn analyze_output(
        &self,
        py: Python<'_>,
        word: &str,
        config: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Vec<Suggestion>> {
        let found = self.run(
            py,
            "analyze_output",
            config,
            |speller, config| match config {
                Some(config) => speller.analyze_output_with_config(word, config),
                None => speller.analyze_output(word),
            },
        )?;
        Ok(found.iter().map(Suggestion::from).collect())
    }

    fn __repr__(&self) -> String {
        match self.archive.metadata() {
            Some(metadata) => format!("<divvun_fst.Speller: {}>", metadata.info().locale()),
            None => "<divvun_fst.Speller: no metadata>".to_owned(),
        }
    }
}

/// Loads the speller in a speller archive's bytes: a BHFST of either box
/// version, its error model and lexicon THFST or DHFST, or a ZHFST, its
/// members stored or deflated. The format is told by the first bytes, and
/// the archive's members are read in place from one copy of them. Bytes that
/// are no speller archive raise FstError with code "unsupported-format"; a
/// broken archive raises "invalid-format" with the reason.
#[pyfunction]
pub fn read_speller(py: Python<'_>, data: PyBackedBytes) -> PyResult<Speller> {
    let data: &[u8] = &data;
    let loaded = log::detached(py, || {
        if data.is_empty() {
            return Err(Failure::fst(
                "invalid-format",
                "a speller archive cannot be empty",
            ));
        }
        Ok(archive::from_bytes(data.to_vec(), BYTES_NAME)?)
    })?;
    loaded
        .map(Speller::new)
        .map_err(|failure| failure.into_pyerr(py))
}

/// Loads the speller in the archive at `path`, mapping its members from the
/// file as the divvunspell tools do: a BHFST by the extension `.bhfst`, a
/// ZHFST by `.zhfst`. A file that cannot be opened raises what `open()`
/// would.
#[pyfunction]
pub fn load_speller(py: Python<'_>, path: PathBuf) -> PyResult<Speller> {
    let loaded = log::detached(py, || -> Result<_, Failure> {
        readable_file(&path)?;
        Ok(archive::open(&path)?)
    })?;
    loaded
        .map(Speller::new)
        .map_err(|failure| failure.into_pyerr(py))
}

/// A suggestion, or an analysis, as divvunspell's `Suggestion` carries it.
/// Each weight is the f32 divvunspell computed, as the float that holds it
/// exactly.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct Suggestion {
    /// The suggested word form; for an analysis, the form with its tags.
    value: String,
    /// Its total weight: lower is better.
    weight: f64,
    /// Whether it is a whole word rather than a part, where the speller says.
    completed: Option<bool>,
    /// How the weight is made up, when the config sets `verbose`.
    weight_details: Option<WeightDetails>,
}

#[pymethods]
impl Suggestion {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(
            py,
            "Suggestion",
            self,
            [value, weight, completed, weight_details]
        )
    }
}

impl From<&suggestion::Suggestion> for Suggestion {
    fn from(found: &suggestion::Suggestion) -> Self {
        Suggestion {
            value: found.value().to_owned(),
            weight: f64::from(found.weight().0),
            completed: found.completed(),
            weight_details: found.weight_details().map(WeightDetails::from),
        }
    }
}

/// How a suggestion's weight is made up.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct WeightDetails {
    /// What the lexicon charged for the word form.
    lexicon_weight: f64,
    /// What the error model charged for reaching it.
    mutator_weight: f64,
    /// The reweighting penalty for edits at the start of the word.
    reweight_start: f64,
    /// The reweighting penalty for edits in the middle, or -1 when the word
    /// has no middle.
    reweight_mid: f64,
    /// The reweighting penalty for edits at the end of the word.
    reweight_end: f64,
}

#[pymethods]
impl WeightDetails {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(
            py,
            "WeightDetails",
            self,
            [
                lexicon_weight,
                mutator_weight,
                reweight_start,
                reweight_mid,
                reweight_end
            ]
        )
    }
}

impl From<&suggestion::WeightDetails> for WeightDetails {
    fn from(details: &suggestion::WeightDetails) -> Self {
        WeightDetails {
            lexicon_weight: f64::from(details.lexicon_weight.0),
            mutator_weight: f64::from(details.mutator_weight.0),
            reweight_start: f64::from(details.reweight_start),
            reweight_mid: f64::from(details.reweight_mid),
            reweight_end: f64::from(details.reweight_end),
        }
    }
}

/// A title in some language; `lang` is None when the archive names none.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct SpellerTitle {
    lang: Option<String>,
    value: String,
}

#[pymethods]
impl SpellerTitle {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(py, "SpellerTitle", self, [lang, value])
    }
}

fn titles(titles: &[meta::SpellerTitle]) -> Vec<SpellerTitle> {
    titles
        .iter()
        .map(|title| SpellerTitle {
            lang: title.lang.clone(),
            value: title.value.clone(),
        })
        .collect()
}

/// What an archive says about its speller.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct SpellerInfo {
    /// The language the speller is for, as a BCP 47 tag such as `se`.
    locale: String,
    title: Vec<SpellerTitle>,
    description: String,
    producer: String,
}

#[pymethods]
impl SpellerInfo {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(
            py,
            "SpellerInfo",
            self,
            [locale, title, description, producer]
        )
    }
}

/// What an archive says about its lexicon.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct AcceptorMetadata {
    r#type: String,
    id: String,
    title: Vec<SpellerTitle>,
    description: String,
    /// The marker for incomplete strings, or None.
    continuation: Option<String>,
}

#[pymethods]
impl AcceptorMetadata {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(
            py,
            "AcceptorMetadata",
            self,
            [r#type, id, title, description, continuation]
        )
    }
}

/// What an archive says about its error model.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct ErrorModelMetadata {
    id: String,
    title: Vec<SpellerTitle>,
    description: String,
}

#[pymethods]
impl ErrorModelMetadata {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(py, "ErrorModelMetadata", self, [id, title, description])
    }
}

/// An archive's metadata: index.xml in a ZHFST, meta.json in a BHFST.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct SpellerMetadata {
    info: SpellerInfo,
    acceptor: AcceptorMetadata,
    error_model: ErrorModelMetadata,
}

#[pymethods]
impl SpellerMetadata {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(py, "SpellerMetadata", self, [info, acceptor, error_model])
    }
}

impl From<&meta::SpellerMetadata> for SpellerMetadata {
    fn from(metadata: &meta::SpellerMetadata) -> Self {
        let (info, acceptor, errmodel) =
            (metadata.info(), metadata.acceptor(), metadata.errmodel());
        SpellerMetadata {
            info: SpellerInfo {
                locale: info.locale().to_owned(),
                title: titles(info.title()),
                description: info.description().to_owned(),
                producer: info.producer().to_owned(),
            },
            acceptor: AcceptorMetadata {
                r#type: acceptor.type_().to_owned(),
                id: acceptor.id().to_owned(),
                title: titles(acceptor.title()),
                description: acceptor.description().to_owned(),
                continuation: acceptor.continuation().map(str::to_owned),
            },
            error_model: ErrorModelMetadata {
                id: errmodel.id().to_owned(),
                title: titles(errmodel.title()),
                description: errmodel.description().to_owned(),
            },
        }
    }
}
