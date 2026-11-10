//! Transducers loaded by path or from bytes, and lookup on them.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use box_format::sync::BoxReader;
use divvun_fst::transducer::dhfst::{DhfstTransducer, DhfstType, acceptor::DhfstAcceptor};
use divvun_fst::transducer::hfst::HfstTransducer;
use divvun_fst::transducer::thfst::{BytesThfstTransducer, MmapThfstTransducer};
use divvun_fst::transducer::{
    Lookup, LookupResult as Found, TransducerError, TransducerFormat, TransducerLoader as _,
};
use divvun_fst::vfs::{self, Filesystem};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::{PyBool, PyInt};

use crate::error::{Failure, chain, readable_file};
use crate::log;
use crate::record::record_repr;

/// The name a transducer that is the whole of the bytes goes by, in error
/// messages and in the file system it is read from.
const BYTES_NAME: &str = "transducer";

/// The first bytes of a box archive.
const BOX_MAGIC: &[u8] = b"\xffBOX";

/// The member name the root of a box goes by, for a THFST directory whose
/// files are at the root.
const ROOT: &str = ".";

/// The names `Transducer.format` gives, by machine, which
/// `TransducerFormat`, the `typing.Literal`, is made from.
pub const FORMATS: [&str; 4] = [
    "optimized-lookup",
    "thfst",
    "dhfst-acceptor",
    "dhfst-error-model",
];

enum Machine {
    Hfst(HfstTransducer),
    /// THFST tables mapped from a directory or a box archive's members.
    MappedThfst(MmapThfstTransducer),
    /// THFST tables read in place from bytes.
    Thfst(BytesThfstTransducer),
    DhfstAcceptor(DhfstAcceptor),
    DhfstErrorModel(DhfstTransducer),
}

impl Machine {
    fn format(&self) -> &'static str {
        match self {
            Machine::Hfst(_) => FORMATS[0],
            Machine::MappedThfst(_) | Machine::Thfst(_) => FORMATS[1],
            Machine::DhfstAcceptor(_) => FORMATS[2],
            Machine::DhfstErrorModel(_) => FORMATS[3],
        }
    }

    fn lookup(&self, tables: &mut Lookup, word: &str) -> Vec<Found> {
        match self {
            Machine::Hfst(t) => tables.lookup(t, word),
            Machine::MappedThfst(t) => tables.lookup(t, word),
            Machine::Thfst(t) => tables.lookup(t, word),
            Machine::DhfstAcceptor(t) => tables.lookup(t, word),
            Machine::DhfstErrorModel(t) => tables.lookup(t, word),
        }
    }
}

/// A transducer loaded for lookup: an analyser, a generator, or a speller's
/// lexicon or error model. It is immutable, so threads may look words up in
/// it at the same time, each lookup without the GIL.
#[pyclass(frozen, module = "divvun_fst")]
pub struct Transducer {
    machine: Machine,
    /// Lookup tables between lookups, so that each keeps its capacity. A
    /// lookup takes one, or makes one when every one is in use, and puts it
    /// back after, so lookups on several threads never wait on each other.
    spare: Mutex<Vec<Lookup>>,
}

impl Transducer {
    fn new(machine: Machine) -> Transducer {
        Transducer {
            machine,
            spare: Mutex::new(Vec::new()),
        }
    }
}

#[pymethods]
impl Transducer {
    /// The format the transducer was read in.
    #[getter]
    fn format(&self) -> &'static str {
        self.machine.format()
    }

    /// Every output the transducer gives `word`, cheapest first, then by
    /// output, with no two the same in output and weight: divvun-fst's
    /// `transducer::lookup`, or the first `limit` of them.
    #[pyo3(signature = (word, /, limit = None))]
    fn lookup(
        &self,
        py: Python<'_>,
        word: &str,
        limit: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Vec<LookupResult>> {
        let limit = limit.map(lookup_limit).transpose()?;
        let found = log::detached(py, || {
            let mut tables = self
                .spare
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop()
                .unwrap_or_default();
            let mut found = self.machine.lookup(&mut tables, word);
            self.spare
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(tables);
            if let Some(limit) = limit {
                found.truncate(limit);
            }
            found
        })?;
        Ok(found.iter().map(LookupResult::from).collect())
    }

    fn __repr__(&self) -> String {
        format!("<divvun_fst.Transducer: {}>", self.machine.format())
    }
}

/// A lookup's limit: a whole number 0 or more. One past what a lookup can
/// find is no limit.
fn lookup_limit(limit: &Bound<'_, PyAny>) -> PyResult<usize> {
    if !limit.is_instance_of::<PyInt>() || limit.is_instance_of::<PyBool>() {
        let given = limit
            .get_type()
            .name()
            .map_or_else(|_| "object".to_owned(), |name| name.to_string());
        return Err(PyTypeError::new_err(format!(
            "lookup() argument 'limit' must be int or None, not {given}"
        )));
    }
    if limit.lt(0)? {
        return Err(PyValueError::new_err(format!(
            "lookup() argument 'limit' must be 0 or more, not {limit}"
        )));
    }
    Ok(limit.extract::<usize>().unwrap_or(usize::MAX))
}

/// One output a transducer gives a word.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct LookupResult {
    /// The output: the text of `symbols`, in order.
    output: String,
    /// The output symbols, by their text. Epsilons and flag diacritics are
    /// left out; an identity symbol writes the symbol it read.
    symbols: Vec<String>,
    /// The path's weight, arcs and final weight, as the float that holds its
    /// f32 exactly.
    weight: f64,
}

#[pymethods]
impl LookupResult {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(py, "LookupResult", self, [output, symbols, weight])
    }
}

impl From<&Found> for LookupResult {
    fn from(found: &Found) -> Self {
        LookupResult {
            output: found.output.to_string(),
            symbols: found.symbols.iter().map(|s| s.to_string()).collect(),
            weight: f64::from(found.weight.0),
        }
    }
}

/// Loads the transducer in `data`, told by its first bytes: HFST optimized
/// lookup, a DHFST acceptor or error model, or a box archive holding one, a
/// THFST directory or a single file. A box holding several, as a BHFST
/// does, needs `member` to say which: a THFST directory such as
/// `acceptor.default.thfst`, `.` for one whose files are at the root, or a
/// single file such as `acceptor.default.dhfst`.
#[pyfunction]
#[pyo3(signature = (data, member = None))]
pub fn read_transducer(
    py: Python<'_>,
    data: PyBackedBytes,
    member: Option<String>,
) -> PyResult<Transducer> {
    let data: &[u8] = &data;
    let loaded = log::detached(py, || from_bytes(data, member))?;
    loaded
        .map(Transducer::new)
        .map_err(|failure| failure.into_pyerr(py))
}

/// Loads the transducer at `path`, mapping it from the file: a THFST
/// directory, or a file read as `read_transducer` reads bytes, a box
/// archive's members mapped where they lie. A path that cannot be opened
/// raises what `open()` would.
#[pyfunction]
#[pyo3(signature = (path, member = None))]
pub fn load_transducer(
    py: Python<'_>,
    path: PathBuf,
    member: Option<String>,
) -> PyResult<Transducer> {
    let loaded = log::detached(py, || from_path(&path, member))?;
    loaded
        .map(Transducer::new)
        .map_err(|failure| failure.into_pyerr(py))
}

fn from_bytes(data: &[u8], member: Option<String>) -> Result<Machine, Failure> {
    if data.is_empty() {
        return Err(Failure::fst(
            "invalid-format",
            "a transducer cannot be empty",
        ));
    }
    if data.starts_with(BOX_MAGIC) {
        let fs = vfs::bytes::Filesystem::from_box(data.to_vec())
            .map_err(|error| Failure::fst("invalid-format", format!("a box archive: {error}")))?;
        let paths = fs.paths().map(str::to_owned).collect();
        return Archive {
            fs,
            paths,
            thfst: |fs: &vfs::bytes::Filesystem, dir: &Path| {
                BytesThfstTransducer::from_path(fs, dir).map(Machine::Thfst)
            },
        }
        .read(member);
    }
    no_member(member, "the bytes")?;
    let mut fs = vfs::bytes::Filesystem::new();
    fs.insert(BYTES_NAME, data.to_vec());
    read_file(&fs, Path::new(BYTES_NAME))
}

fn from_path(path: &Path, member: Option<String>) -> Result<Machine, Failure> {
    let metadata = std::fs::metadata(path).map_err(|error| Failure::os(error, path))?;
    if metadata.is_dir() {
        no_member(member, "a THFST directory")?;
        return Ok(Machine::MappedThfst(MmapThfstTransducer::from_path(
            &vfs::Fs,
            path,
        )?));
    }
    let mut head = Vec::with_capacity(8);
    readable_file(path)?
        .take(8)
        .read_to_end(&mut head)
        .map_err(|error| Failure::os(error, path))?;
    if head.is_empty() {
        return Err(Failure::fst(
            "invalid-format",
            format!("'{}' is empty, and a transducer cannot be", path.display()),
        ));
    }
    if head.starts_with(BOX_MAGIC) {
        let reader = BoxReader::open(path).map_err(|error| {
            Failure::fst(
                "invalid-format",
                format!("a box archive '{}': {}", path.display(), chain(&error)),
            )
        })?;
        let paths = box_files(&reader);
        return Archive {
            fs: vfs::boxf::Filesystem::new(&reader),
            paths,
            thfst: |fs: &vfs::boxf::Filesystem<'_>, dir: &Path| {
                MmapThfstTransducer::from_path(fs, dir).map(Machine::MappedThfst)
            },
        }
        .read(member);
    }
    no_member(member, "a file that is not a box archive")?;
    read_file(&vfs::Fs, path)
}

/// The path of every file in a box archive, its components joined with `/`.
fn box_files(reader: &BoxReader) -> Vec<String> {
    reader
        .metadata()
        .iter()
        .filter(|item| item.record.as_file().is_some())
        .map(|item| item.path.iter().collect::<Vec<_>>().join("/"))
        .collect()
}

fn no_member(member: Option<String>, what: &str) -> Result<(), Failure> {
    match member {
        Some(member) => Err(Failure::Value(format!(
            "only a box archive has members, so {what} has no member {member:?} to read"
        ))),
        None => Ok(()),
    }
}

/// The format a file's first bytes declare, or `None` when they declare none
/// this library reads.
fn file_format<FS: Filesystem>(fs: &FS, path: &Path) -> Option<TransducerFormat> {
    let mut head = Vec::with_capacity(8);
    fs.open_file(path)
        .ok()?
        .take(8)
        .read_to_end(&mut head)
        .ok()?;
    TransducerFormat::detect(&head, path).ok()
}

/// The transducer in the file at `path`, in the format its first bytes
/// declare: a member's name never decides how it is read.
fn read_file<FS: Filesystem>(fs: &FS, path: &Path) -> Result<Machine, Failure> {
    let mut head = Vec::with_capacity(8);
    fs.open_file(path)
        .and_then(|file| file.take(8).read_to_end(&mut head))
        .map_err(|error| Failure::fst("invalid-format", format!("{}: {error}", path.display())))?;
    Ok(match TransducerFormat::detect(&head, path)? {
        TransducerFormat::Hfst => Machine::Hfst(HfstTransducer::from_path(fs, path)?),
        TransducerFormat::Dhfst {
            kind: DhfstType::Acceptor,
            ..
        } => Machine::DhfstAcceptor(DhfstAcceptor::from_path(fs, path)?),
        TransducerFormat::Dhfst {
            kind: DhfstType::ErrorModel,
            ..
        } => Machine::DhfstErrorModel(DhfstTransducer::from_path(fs, path)?),
    })
}

/// A box archive, in memory or mapped from its file: the files it holds,
/// and how a THFST directory in it is read.
struct Archive<FS, T> {
    fs: FS,
    paths: Vec<String>,
    thfst: T,
}

impl<FS, T> Archive<FS, T>
where
    FS: Filesystem,
    T: Fn(&FS, &Path) -> Result<Machine, TransducerError>,
{
    /// The transducer `member` names, or the only one the box holds.
    fn read(&self, member: Option<String>) -> Result<Machine, Failure> {
        let member = match member {
            Some(member) => member,
            None => self.only_member()?,
        };
        let directory = Path::new(&member);
        if self.fs.open_file(directory.join("alphabet")).is_ok() {
            return Ok((self.thfst)(&self.fs, directory)?);
        }
        if self.fs.open_file(&member).is_ok() {
            return read_file(&self.fs, Path::new(&member));
        }
        let found = self.members();
        let holds = match found.is_empty() {
            true => "no transducer".to_owned(),
            false => found.join(", "),
        };
        Err(Failure::Value(format!(
            "the box archive has no member {member:?}; it holds {holds}"
        )))
    }

    /// The THFST directories and the transducer files the box holds, by
    /// member name, sorted. A directory is a THFST one when it holds an
    /// `alphabet`; the files inside it are its tables, not transducers of
    /// their own.
    fn members(&self) -> Vec<String> {
        let mut directories: Vec<String> = self
            .paths
            .iter()
            .filter_map(|path| match path.rsplit_once('/') {
                Some((directory, "alphabet")) => Some(directory.to_owned()),
                None if path == "alphabet" => Some(ROOT.to_owned()),
                _ => None,
            })
            .collect();
        let inside = |path: &str| {
            directories.iter().any(|directory| {
                directory == ROOT && !path.contains('/')
                    || path
                        .strip_prefix(directory.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        };
        let files: Vec<String> = self
            .paths
            .iter()
            .filter(|path| !inside(path) && file_format(&self.fs, Path::new(path)).is_some())
            .cloned()
            .collect();
        directories.extend(files);
        directories.sort();
        directories
    }

    fn only_member(&self) -> Result<String, Failure> {
        let mut found = self.members();
        match found.len() {
            0 => Err(Failure::fst(
                "unsupported-format",
                "the box archive holds no transducer: no THFST directory and no HFST or DHFST file",
            )),
            1 => Ok(found.remove(0)),
            count => Err(Failure::Value(format!(
                "the box archive holds {count} transducers ({}); name the one to read with member=",
                found.join(", ")
            ))),
        }
    }
}
