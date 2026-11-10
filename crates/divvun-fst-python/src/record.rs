//! The repr of the plain result classes, written as a call that makes them.

/// `Name(field=value, ...)` for `$record`, each value as Python's repr
/// writes it. A raw identifier such as `r#type` is named without its `r#`,
/// as pyo3 names its getter.
macro_rules! record_repr {
    ($py:expr, $name:literal, $record:expr, [$($field:ident),* $(,)?]) => {{
        use pyo3::IntoPyObject as _;
        let py = $py;
        let parts: Vec<String> = vec![$(
            format!(
                "{}={}",
                stringify!($field).trim_start_matches("r#"),
                $record.$field.clone().into_pyobject(py)?.into_any().repr()?
            )
        ),*];
        Ok::<String, pyo3::PyErr>(format!(concat!($name, "({})"), parts.join(", ")))
    }};
}

pub(crate) use record_repr;
