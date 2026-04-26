//! Option-struct extraction from Python kwargs for parse() and unparse().
//!
//! Unsupported options raise `NotImplementedError` to make the scope explicit.

use pyo3::exceptions::{PyNotImplementedError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyList, PyString, PyTuple};

/// Options accepted by `parse()`.
#[derive(Debug, Clone)]
pub struct ParseOpts {
    pub attr_prefix: String,
    pub cdata_key: String,
    pub cdata_separator: String,
    pub strip_whitespace: bool,
    pub namespace_separator: String,
    pub process_namespaces: bool,
    pub namespaces: Option<Vec<(String, String)>>,
    pub process_comments: bool,
    pub comment_key: String,
    pub force_list: ForceList,
    pub force_cdata: ForceCdata,
    pub disable_entities: bool,
    pub xml_attribs: bool,
}

#[derive(Debug, Clone)]
pub enum ForceList {
    Off,
    All,
    Keys(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum ForceCdata {
    Off,
    All,
    Keys(Vec<String>),
}

impl Default for ParseOpts {
    fn default() -> Self {
        Self {
            attr_prefix: "@".into(),
            cdata_key: "#text".into(),
            cdata_separator: String::new(),
            strip_whitespace: true,
            namespace_separator: ":".into(),
            process_namespaces: false,
            namespaces: None,
            process_comments: false,
            comment_key: "#comment".into(),
            force_list: ForceList::Off,
            force_cdata: ForceCdata::Off,
            disable_entities: true,
            xml_attribs: true,
        }
    }
}

/// Options accepted by `unparse()`.
#[derive(Debug, Clone)]
pub struct UnparseOpts {
    pub attr_prefix: String,
    pub cdata_key: String,
    pub comment_key: String,
    pub namespaces: Option<Vec<(String, String)>>,
    pub namespace_separator: String,
    pub pretty: bool,
    pub newl: String,
    pub indent: String,
    pub full_document: bool,
    pub short_empty_elements: bool,
    pub encoding: String,
    /// Precomputed `attr_prefix + "xmlns"` for the hot path.
    pub xmlns_attr_key: String,
}

impl Default for UnparseOpts {
    fn default() -> Self {
        Self {
            attr_prefix: "@".into(),
            cdata_key: "#text".into(),
            comment_key: "#comment".into(),
            namespaces: None,
            namespace_separator: ":".into(),
            pretty: false,
            newl: "\n".into(),
            indent: "\t".into(),
            full_document: true,
            short_empty_elements: false,
            encoding: "utf-8".into(),
            xmlns_attr_key: "@xmlns".into(),
        }
    }
}

impl UnparseOpts {
    fn recompute_derived(&mut self) {
        self.xmlns_attr_key = format!("{}xmlns", self.attr_prefix);
    }
}

/// Keys that raise NotImplementedError when supplied to parse().
const PARSE_UNSUPPORTED: &[&str] = &[
    "postprocessor",
    "item_callback",
    "item_depth",
    "dict_constructor",
    "preprocessor",
    "expand_iter",
    "expat",
];

/// Keys that raise NotImplementedError when supplied to unparse().
const UNPARSE_UNSUPPORTED: &[&str] = &[
    "postprocessor",
    "preprocessor",
    "expand_iter",
    "dict_constructor",
];

/// All keys the parse() path will accept (everything else is rejected).
const PARSE_KNOWN: &[&str] = &[
    "attr_prefix",
    "cdata_key",
    "cdata_separator",
    "strip_whitespace",
    "namespace_separator",
    "process_namespaces",
    "namespaces",
    "process_comments",
    "comment_key",
    "force_list",
    "force_cdata",
    "disable_entities",
    "xml_attribs",
    "encoding",
];

/// All keys the unparse() path will accept.
const UNPARSE_KNOWN: &[&str] = &[
    "attr_prefix",
    "cdata_key",
    "comment_key",
    "namespaces",
    "namespace_separator",
    "pretty",
    "newl",
    "indent",
    "full_document",
    "short_empty_elements",
    "encoding",
    "bytes_errors",
];

impl ParseOpts {
    pub fn from_kwargs(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut out = ParseOpts::default();
        let Some(kwargs) = kwargs else {
            return Ok(out);
        };

        for (k, v) in kwargs.iter() {
            let key = k.extract::<String>()?;
            if PARSE_UNSUPPORTED.contains(&key.as_str()) {
                return Err(PyNotImplementedError::new_err(format!(
                    "orxml.parse: option `{key}` is not supported in v1"
                )));
            }
            if !PARSE_KNOWN.contains(&key.as_str()) {
                return Err(PyTypeError::new_err(format!(
                    "orxml.parse: unknown option `{key}`"
                )));
            }
            match key.as_str() {
                "attr_prefix" => out.attr_prefix = v.extract()?,
                "cdata_key" => out.cdata_key = v.extract()?,
                "cdata_separator" => out.cdata_separator = v.extract()?,
                "strip_whitespace" => out.strip_whitespace = v.extract()?,
                "namespace_separator" => out.namespace_separator = v.extract()?,
                "process_namespaces" => out.process_namespaces = v.extract()?,
                "namespaces" => out.namespaces = Some(extract_ns_map(&v)?),
                "process_comments" => out.process_comments = v.extract()?,
                "comment_key" => out.comment_key = v.extract()?,
                "force_list" => out.force_list = extract_force_list(&v)?,
                "force_cdata" => out.force_cdata = extract_force_cdata(&v)?,
                "disable_entities" => out.disable_entities = v.extract()?,
                "xml_attribs" => out.xml_attribs = v.extract()?,
                "encoding" => {
                    // accepted for API compat but ignored; quick-xml auto-detects.
                }
                _ => unreachable!(),
            }
        }
        Ok(out)
    }
}

impl UnparseOpts {
    pub fn from_kwargs(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut out = UnparseOpts::default();
        let Some(kwargs) = kwargs else {
            return Ok(out);
        };

        for (k, v) in kwargs.iter() {
            let key = k.extract::<String>()?;
            if UNPARSE_UNSUPPORTED.contains(&key.as_str()) {
                return Err(PyNotImplementedError::new_err(format!(
                    "orxml.unparse: option `{key}` is not supported in v1"
                )));
            }
            if !UNPARSE_KNOWN.contains(&key.as_str()) {
                return Err(PyTypeError::new_err(format!(
                    "orxml.unparse: unknown option `{key}`"
                )));
            }
            match key.as_str() {
                "attr_prefix" => out.attr_prefix = v.extract()?,
                "cdata_key" => out.cdata_key = v.extract()?,
                "comment_key" => out.comment_key = v.extract()?,
                "namespaces" => out.namespaces = Some(extract_ns_map(&v)?),
                "namespace_separator" => out.namespace_separator = v.extract()?,
                "pretty" => out.pretty = v.extract()?,
                "newl" => out.newl = v.extract()?,
                "indent" => {
                    if let Ok(n) = v.extract::<usize>() {
                        out.indent = " ".repeat(n);
                    } else {
                        out.indent = v.extract()?;
                    }
                }
                "full_document" => out.full_document = v.extract()?,
                "short_empty_elements" => out.short_empty_elements = v.extract()?,
                "encoding" => out.encoding = v.extract()?,
                "bytes_errors" => {
                    // Accepted but ignored; we don't emit from bytes with error handlers.
                }
                _ => unreachable!(),
            }
        }
        out.recompute_derived();
        Ok(out)
    }
}

fn extract_ns_map(v: &Bound<'_, PyAny>) -> PyResult<Vec<(String, String)>> {
    let d = v
        .cast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("`namespaces` must be a dict mapping URI -> prefix"))?;
    let mut out = Vec::with_capacity(d.len());
    for (k, val) in d.iter() {
        let ks: String = k.extract()?;
        let vs: String = val.extract()?;
        out.push((ks, vs));
    }
    Ok(out)
}

fn extract_force_list(v: &Bound<'_, PyAny>) -> PyResult<ForceList> {
    if v.is_none() {
        return Ok(ForceList::Off);
    }
    if v.cast::<PyBool>().is_ok() {
        let b: bool = v.extract()?;
        return Ok(if b { ForceList::All } else { ForceList::Off });
    }
    if v.is_callable() {
        return Err(PyNotImplementedError::new_err(
            "orxml.parse: `force_list` as callable is not supported in v1",
        ));
    }
    if let Ok(t) = v.cast::<PyTuple>() {
        let mut keys = Vec::with_capacity(t.len());
        for item in t.iter() {
            keys.push(item.extract::<String>()?);
        }
        return Ok(ForceList::Keys(keys));
    }
    if let Ok(l) = v.cast::<PyList>() {
        let mut keys = Vec::with_capacity(l.len());
        for item in l.iter() {
            keys.push(item.extract::<String>()?);
        }
        return Ok(ForceList::Keys(keys));
    }
    if let Ok(s) = v.cast::<PyString>() {
        return Ok(ForceList::Keys(vec![s.extract()?]));
    }
    Err(PyTypeError::new_err(
        "orxml.parse: `force_list` must be bool, tuple/list of str, or None",
    ))
}

fn extract_force_cdata(v: &Bound<'_, PyAny>) -> PyResult<ForceCdata> {
    if v.is_none() {
        return Ok(ForceCdata::Off);
    }
    if v.cast::<PyBool>().is_ok() {
        let b: bool = v.extract()?;
        return Ok(if b { ForceCdata::All } else { ForceCdata::Off });
    }
    if v.is_callable() {
        return Err(PyNotImplementedError::new_err(
            "orxml.parse: `force_cdata` as callable is not supported in v1",
        ));
    }
    if let Ok(t) = v.cast::<PyTuple>() {
        let mut keys = Vec::with_capacity(t.len());
        for item in t.iter() {
            keys.push(item.extract::<String>()?);
        }
        return Ok(ForceCdata::Keys(keys));
    }
    if let Ok(l) = v.cast::<PyList>() {
        let mut keys = Vec::with_capacity(l.len());
        for item in l.iter() {
            keys.push(item.extract::<String>()?);
        }
        return Ok(ForceCdata::Keys(keys));
    }
    if let Ok(s) = v.cast::<PyString>() {
        return Ok(ForceCdata::Keys(vec![s.extract()?]));
    }
    Err(PyTypeError::new_err(
        "orxml.parse: `force_cdata` must be bool, tuple/list of str, or None",
    ))
}

impl ForceList {
    pub fn contains(&self, key: &str) -> bool {
        match self {
            ForceList::Off => false,
            ForceList::All => true,
            ForceList::Keys(keys) => keys.iter().any(|k| k == key),
        }
    }
}

impl ForceCdata {
    pub fn contains(&self, key: &str) -> bool {
        match self {
            ForceCdata::Off => false,
            ForceCdata::All => true,
            ForceCdata::Keys(keys) => keys.iter().any(|k| k == key),
        }
    }
}
