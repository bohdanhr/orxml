//! Error types. We expose a single `orxml.ParseError` as a subclass of `ValueError`.

use pyo3::create_exception;
use pyo3::exceptions::PyValueError;

create_exception!(
    orxml,
    ParseError,
    PyValueError,
    "Raised when parsing XML fails or input is not valid."
);
