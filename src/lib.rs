//! orxml: Rust-backed xmltodict-compatible XML <-> dict conversion.

use pyo3::prelude::*;

mod errors;
mod opts;
mod parse;
mod unparse;

pub use errors::ParseError;

#[pymodule]
fn _core(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("ParseError", py.get_type::<ParseError>())?;
    m.add_function(wrap_pyfunction!(parse::parse, m)?)?;
    m.add_function(wrap_pyfunction!(unparse::unparse, m)?)?;
    Ok(())
}
