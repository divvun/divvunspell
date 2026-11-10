//! A speller's config, as the fields of `divvun_fst.SpellerConfig` and
//! `divvun_fst.ReweightingConfig`, the dataclasses in
//! `python/divvun_fst/_config.py`.
//!
//! Reading builds divvun-fst's structs with every field named and no `..`
//! rest, and writing destructures them the same way, so a field gained, lost
//! or renamed there stops this crate compiling until the dataclasses say what
//! they do with it. A dataclass field no reader takes is refused when the
//! config is made, so the two sides cannot drift apart unseen either.

use std::cell::Cell;

use divvun_fst::speller::{ReweightingConfig, SpellerConfig};
use divvun_fst::types::Weight;
use pyo3::exceptions::{PyOverflowError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedStr;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyString, PyType};

const MODULE: &str = "divvun_fst._config";

static SPELLER_CONFIG: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static REWEIGHTING_CONFIG: PyOnceLock<Py<PyType>> = PyOnceLock::new();

fn speller_config_class(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    SPELLER_CONFIG.import(py, MODULE, "SpellerConfig")
}

fn reweighting_config_class(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    REWEIGHTING_CONFIG.import(py, MODULE, "ReweightingConfig")
}

/// The config a speller method was passed: `None` for the speller's own,
/// which is the archive's bundled config or else the built-in defaults.
pub fn of_call(config: Option<&Bound<'_, PyAny>>, method: &str) -> PyResult<Option<SpellerConfig>> {
    let Some(config) = config else {
        return Ok(None);
    };
    if !config.is_instance(speller_config_class(config.py())?)? {
        let given = type_name(config);
        return Err(PyTypeError::new_err(format!(
            "{method}() argument 'config' must be SpellerConfig or None, not {given}"
        )));
    }
    read_speller_config(config).map(Some)
}

/// The divvun-fst config a `SpellerConfig` stands for.
fn read_speller_config(config: &Bound<'_, PyAny>) -> PyResult<SpellerConfig> {
    let fields = Fields::of(config, "SpellerConfig")?;
    let read = SpellerConfig {
        n_best: fields.take("n_best")?.count()?,
        max_weight: fields.take("max_weight")?.weight()?,
        beam: fields.take("beam")?.weight()?,
        reweight: fields.take("reweight")?.reweighting()?,
        recase: fields.take("recase")?.flag()?,
        completion_marker: fields.take("completion_marker")?.text()?,
        astar_lookahead: fields.take("astar_lookahead")?.flag()?,
        search_dedup: fields.take("search_dedup")?.flag()?,
        mutator_subsets: fields.take("mutator_subsets")?.flag()?,
        search_budget: fields.take("search_budget")?.count()?,
        word_split_weight: fields.take("word_split_weight")?.weight()?,
        boundary_edit_weight: fields.take("boundary_edit_weight")?.weight()?,
        initial_case_weight: fields.take("initial_case_weight")?.weight()?,
        verbose: fields.take("verbose")?.flag()?,
    };
    fields.all_taken()?;
    Ok(read)
}

/// The divvun-fst penalties a `ReweightingConfig` stands for.
fn read_reweighting_config(config: &Bound<'_, PyAny>) -> PyResult<ReweightingConfig> {
    let fields = Fields::of(config, "ReweightingConfig")?;
    let read = ReweightingConfig {
        start_penalty: fields.take("start_penalty")?.penalty()?,
        end_penalty: fields.take("end_penalty")?.penalty()?,
        mid_penalty: fields.take("mid_penalty")?.penalty()?,
        curve: fields.take("curve")?.weight()?.map(|curve| curve.0),
    };
    fields.all_taken()?;
    Ok(read)
}

/// Checks a `SpellerConfig` as it is made, raising what a call reading its
/// fields would raise.
#[pyfunction]
#[pyo3(name = "_check_speller_config")]
pub fn check_speller_config(config: &Bound<'_, PyAny>) -> PyResult<()> {
    read_speller_config(config).map(drop)
}

/// Checks a `ReweightingConfig` as it is made.
#[pyfunction]
#[pyo3(name = "_check_reweighting_config")]
pub fn check_reweighting_config(config: &Bound<'_, PyAny>) -> PyResult<()> {
    read_reweighting_config(config).map(drop)
}

/// divvun-fst's `SpellerConfig::default()`, as a `SpellerConfig`, for the
/// tests that hold the dataclass defaults against it.
#[pyfunction]
#[pyo3(name = "_builtin_speller_config")]
pub fn builtin_speller_config(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    to_python(py, &SpellerConfig::default())
}

/// `config` read into divvun-fst's config and written back, for the tests
/// that show every field reaches divvun-fst as given.
#[pyfunction]
#[pyo3(name = "_resolve_speller_config")]
pub fn resolve_speller_config<'py>(config: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let read = of_call(Some(config), "_resolve_speller_config")?
        .ok_or_else(|| PyTypeError::new_err("a SpellerConfig to resolve"))?;
    to_python(config.py(), &read)
}

/// `config` as a `SpellerConfig`. Each weight is the shortest decimal that
/// reads back as the f32 divvun-fst holds, so a value written in a config
/// comes back as written: 0.1, not 0.10000000149011612.
pub fn to_python<'py>(py: Python<'py>, config: &SpellerConfig) -> PyResult<Bound<'py, PyAny>> {
    let SpellerConfig {
        n_best,
        max_weight,
        beam,
        reweight,
        recase,
        completion_marker,
        astar_lookahead,
        search_dedup,
        mutator_subsets,
        search_budget,
        word_split_weight,
        boundary_edit_weight,
        initial_case_weight,
        verbose,
    } = config;
    let weight = |weight: &Option<Weight>| weight.map(|weight| decimal(weight.0));
    let reweight = match reweight {
        Some(reweight) => Some(reweighting_to_python(py, reweight)?),
        None => None,
    };
    let fields = PyDict::new(py);
    fields.set_item("n_best", n_best)?;
    fields.set_item("max_weight", weight(max_weight))?;
    fields.set_item("beam", weight(beam))?;
    fields.set_item("reweight", reweight)?;
    fields.set_item("recase", recase)?;
    fields.set_item("completion_marker", completion_marker)?;
    fields.set_item("astar_lookahead", astar_lookahead)?;
    fields.set_item("search_dedup", search_dedup)?;
    fields.set_item("mutator_subsets", mutator_subsets)?;
    fields.set_item("search_budget", search_budget)?;
    fields.set_item("word_split_weight", weight(word_split_weight))?;
    fields.set_item("boundary_edit_weight", weight(boundary_edit_weight))?;
    fields.set_item("initial_case_weight", weight(initial_case_weight))?;
    fields.set_item("verbose", verbose)?;
    speller_config_class(py)?.call((), Some(&fields))
}

fn reweighting_to_python<'py>(
    py: Python<'py>,
    reweight: &ReweightingConfig,
) -> PyResult<Bound<'py, PyAny>> {
    let ReweightingConfig {
        start_penalty,
        end_penalty,
        mid_penalty,
        curve,
    } = reweight;
    let fields = PyDict::new(py);
    fields.set_item("start_penalty", decimal(*start_penalty))?;
    fields.set_item("end_penalty", decimal(*end_penalty))?;
    fields.set_item("mid_penalty", decimal(*mid_penalty))?;
    fields.set_item("curve", curve.map(decimal))?;
    reweighting_config_class(py)?.call((), Some(&fields))
}

/// The float whose shortest decimal is the shortest decimal of `value`.
fn decimal(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

fn type_name(value: &Bound<'_, PyAny>) -> String {
    value
        .get_type()
        .name()
        .map_or_else(|_| "object".to_owned(), |name| name.to_string())
}

/// A dataclass's fields by name, each to be taken by the reader that knows
/// what it means. A field left untaken is one no reader knows.
struct Fields<'py> {
    class: &'static str,
    fields: Vec<(PyBackedStr, Bound<'py, PyAny>, Cell<bool>)>,
}

impl<'py> Fields<'py> {
    fn of(object: &Bound<'py, PyAny>, class: &'static str) -> PyResult<Fields<'py>> {
        let declared = object
            .py()
            .import("dataclasses")?
            .call_method1("fields", (object,))?;
        let mut fields = Vec::new();
        for field in declared.try_iter()? {
            let name: PyBackedStr = field?.getattr("name")?.extract()?;
            let value = object.getattr(&*name)?;
            fields.push((name, value, Cell::new(false)));
        }
        Ok(Fields { class, fields })
    }

    fn take(&self, name: &'static str) -> PyResult<Value<'_, 'py>> {
        let (_, value, taken) = self
            .fields
            .iter()
            .find(|(field, _, _)| &**field == name)
            .ok_or_else(|| PyTypeError::new_err(format!("{} has no field '{name}'", self.class)))?;
        taken.set(true);
        Ok(Value {
            class: self.class,
            name,
            value,
        })
    }

    fn all_taken(&self) -> PyResult<()> {
        match self.fields.iter().find(|(_, _, taken)| !taken.get()) {
            Some((name, _, _)) => Err(PyTypeError::new_err(format!(
                "{} has a field '{}' no speller reads",
                self.class, &**name
            ))),
            None => Ok(()),
        }
    }
}

/// One field's value, named for the errors it raises.
struct Value<'a, 'py> {
    class: &'static str,
    name: &'static str,
    value: &'a Bound<'py, PyAny>,
}

impl Value<'_, '_> {
    fn wrong_type(&self, expected: &str) -> PyErr {
        PyTypeError::new_err(format!(
            "{}() argument '{}' must be {expected}, not {}",
            self.class,
            self.name,
            type_name(self.value)
        ))
    }

    /// A Python int, a bool aside although Python counts it as one.
    fn is_int(&self) -> bool {
        self.value.is_instance_of::<PyInt>() && !self.value.is_instance_of::<PyBool>()
    }

    fn flag(&self) -> PyResult<bool> {
        self.value
            .cast::<PyBool>()
            .map(|flag| flag.is_true())
            .map_err(|_| self.wrong_type("bool"))
    }

    fn text(&self) -> PyResult<Option<String>> {
        if self.value.is_none() {
            return Ok(None);
        }
        let text = self
            .value
            .cast::<PyString>()
            .map_err(|_| self.wrong_type("str or None"))?;
        Ok(Some(text.to_str()?.to_owned()))
    }

    /// A whole number from 0 to the most the field holds, or `None`.
    fn count<T: TryFrom<u64> + num::Bounded>(&self) -> PyResult<Option<T>> {
        if self.value.is_none() {
            return Ok(None);
        }
        if !self.is_int() {
            return Err(self.wrong_type("int or None"));
        }
        let count = self.value.extract::<u64>().ok();
        match count.and_then(|count| T::try_from(count).ok()) {
            Some(count) => Ok(Some(count)),
            None => Err(PyOverflowError::new_err(format!(
                "{}() argument '{}' must be from 0 to {}, not {}",
                self.class,
                self.name,
                T::MAX,
                self.value
            ))),
        }
    }

    /// A finite number that an f32 holds, or `None`. NaN and the infinities
    /// are refused rather than read as some weight: `None` is how a config
    /// says there is no limit.
    fn weight(&self) -> PyResult<Option<Weight>> {
        if self.value.is_none() {
            return Ok(None);
        }
        self.number("float or None")
            .map(|value| Some(Weight(value)))
    }

    fn penalty(&self) -> PyResult<f32> {
        self.number("float")
    }

    fn number(&self, expected: &str) -> PyResult<f32> {
        if !(self.is_int() || self.value.is_instance_of::<PyFloat>()) {
            return Err(self.wrong_type(expected));
        }
        let value: f64 = self.value.extract()?;
        if !value.is_finite() {
            return Err(PyValueError::new_err(format!(
                "{}() argument '{}' must be finite, not {value}",
                self.class, self.name
            )));
        }
        let narrowed = value as f32;
        if !narrowed.is_finite() {
            return Err(PyOverflowError::new_err(format!(
                "{}() argument '{}' must be within the range of a 32-bit float, not {value}",
                self.class, self.name
            )));
        }
        Ok(narrowed)
    }

    fn reweighting(&self) -> PyResult<Option<ReweightingConfig>> {
        if self.value.is_none() {
            return Ok(None);
        }
        if !self
            .value
            .is_instance(reweighting_config_class(self.value.py())?)?
        {
            return Err(self.wrong_type("ReweightingConfig or None"));
        }
        read_reweighting_config(self.value).map(Some)
    }
}

/// The largest value of the count types a config holds, for the message that
/// names the range.
mod num {
    pub trait Bounded: std::fmt::Display {
        const MAX: Self;
    }

    impl Bounded for usize {
        const MAX: usize = usize::MAX;
    }

    impl Bounded for u64 {
        const MAX: u64 = u64::MAX;
    }
}
