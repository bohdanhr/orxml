//! unparse(dict, **opts) -> str
//!
//! Hand-written XML emitter walking a Python dict. We don't use quick-xml's
//! Writer here because we want full control over attribute ordering (insertion
//! order, matching xmltodict) and over pretty-printing with arbitrary
//! `indent`/`newl` strings.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple,
};

use crate::opts::UnparseOpts;

#[pyfunction]
#[pyo3(signature = (input_dict, **kwargs))]
pub fn unparse<'py>(
    py: Python<'py>,
    input_dict: &Bound<'py, PyAny>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<String> {
    let opts = UnparseOpts::from_kwargs(kwargs)?;
    let d = input_dict
        .cast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("orxml.unparse: input must be a dict"))?;

    let mut out = String::with_capacity(256);
    if opts.full_document {
        out.push_str("<?xml version=\"1.0\" encoding=\"");
        append_escaped_attr(&mut out, &opts.encoding);
        out.push_str("\"?>");
        if opts.pretty {
            out.push_str(&opts.newl);
        }
    }

    let mut seen_root = false;
    for (k, v) in d.iter() {
        let key_str: String = k.extract()?;
        if key_str == opts.comment_key {
            emit_comment(&mut out, &v, &opts, 0)?;
            continue;
        }
        if opts.full_document && seen_root {
            return Err(PyValueError::new_err(
                "Document must have exactly one root.",
            ));
        }
        emit_element(py, &mut out, &key_str, &v, &opts, 0)?;
        seen_root = true;
    }
    if opts.full_document && !seen_root {
        return Err(PyValueError::new_err(
            "Document must have exactly one root.",
        ));
    }
    Ok(out)
}

// ---------- element emission ----------

fn emit_element(
    py: Python<'_>,
    out: &mut String,
    key: &str,
    value: &Bound<'_, PyAny>,
    opts: &UnparseOpts,
    depth: usize,
) -> PyResult<()> {
    // key may carry a namespace prefix that needs rewriting via opts.namespaces.
    let elem_name = process_namespace(key, opts, false);
    validate_name(&elem_name, "element")?;

    // Expand value to an iterable of "instances" so repeated-key semantics work.
    let items = expand_value(py, value)?;
    for (i, v) in items.iter().enumerate() {
        if opts.full_document && depth == 0 && i > 0 {
            return Err(PyValueError::new_err("document with multiple roots"));
        }
        emit_one(py, out, &elem_name, v, opts, depth)?;
    }
    Ok(())
}

fn emit_one(
    py: Python<'_>,
    out: &mut String,
    elem_name: &str,
    v: &Bound<'_, PyAny>,
    opts: &UnparseOpts,
    depth: usize,
) -> PyResult<()> {
    // Normalize v to a dict-shape: scalars -> {cdata_key: str}, None -> {}.
    let as_dict_holder: Py<PyDict>;
    let v_dict: &Bound<'_, PyDict> = if v.is_none() {
        as_dict_holder = PyDict::new(py).unbind();
        as_dict_holder.bind(py)
    } else if let Ok(d) = v.cast::<PyDict>() {
        d
    } else {
        let s = convert_value_to_string(v)?;
        let d = PyDict::new(py);
        d.set_item(&opts.cdata_key, s)?;
        as_dict_holder = d.unbind();
        as_dict_holder.bind(py)
    };

    // Walk the dict once to separate cdata, attributes, children (preserves
    // insertion order).
    let mut cdata: Option<String> = None;
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut children: Vec<(String, Py<PyAny>)> = Vec::new();
    let mut comments: Vec<Py<PyAny>> = Vec::new();

    for (ik, iv) in v_dict.iter() {
        let key: String = ik.extract()?;
        if key == opts.cdata_key {
            if iv.is_none() {
                cdata = None;
            } else {
                cdata = Some(convert_value_to_string(&iv)?);
            }
            continue;
        }
        if key.starts_with(&opts.attr_prefix) {
            // @xmlns as dict: expand into xmlns / xmlns:prefix attrs
            if key == format!("{}xmlns", opts.attr_prefix) {
                if let Ok(nsd) = iv.cast::<PyDict>() {
                    for (nk, nv) in nsd.iter() {
                        let prefix: String = nk.extract()?;
                        validate_name(&prefix, "attribute")?;
                        let uri = if nv.is_none() {
                            String::new()
                        } else {
                            convert_value_to_string(&nv)?
                        };
                        let aname = if prefix.is_empty() {
                            "xmlns".to_owned()
                        } else {
                            format!("xmlns:{prefix}")
                        };
                        attrs.push((aname, uri));
                    }
                    continue;
                }
            }
            let attr_name_raw = &key[opts.attr_prefix.len()..];
            let attr_name = process_namespace(attr_name_raw, opts, true);
            validate_name(&attr_name, "attribute")?;
            let val = if iv.is_none() {
                String::new()
            } else {
                convert_value_to_string(&iv)?
            };
            attrs.push((attr_name, val));
            continue;
        }
        if key == opts.comment_key {
            comments.push(iv.unbind());
            continue;
        }
        // Skip empty lists (xmltodict behavior).
        if let Ok(l) = iv.cast::<PyList>() {
            if l.is_empty() {
                continue;
            }
        }
        children.push((key, iv.unbind()));
    }

    let has_children_or_comments = !children.is_empty() || !comments.is_empty();

    // Opening tag
    if opts.pretty {
        push_indent(out, &opts.indent, depth);
    }
    out.push('<');
    out.push_str(elem_name);
    for (an, av) in &attrs {
        out.push(' ');
        out.push_str(an);
        out.push_str("=\"");
        append_escaped_attr(out, av);
        out.push('"');
    }
    let short_empty = opts.short_empty_elements && cdata.is_none() && !has_children_or_comments;
    if short_empty {
        out.push_str("/>");
    } else {
        out.push('>');
    }

    if !short_empty {
        if opts.pretty && has_children_or_comments {
            out.push_str(&opts.newl);
        }

        // Comments first (xmltodict emits them before the cdata in _emit order? Let's
        // look: _emit processes dict entries in iteration order, comments are
        // emitted via recursive _emit. Our simpler approach: emit comments in
        // their own pass, but that loses interleaving. For medium parity we emit
        // comments after children's emission is interleaved in the original.)
        // Simpler: walk dict again in-order and dispatch comments/children as we
        // go, since we already captured them.
        // --- second pass preserving order of children+comments using the
        // entries list we captured:
        // To preserve proper ordering we re-walk the original dict.
        // For simplicity and correctness here, we re-walk v_dict.
        for (ik, iv) in v_dict.iter() {
            let k: String = ik.extract()?;
            if k == opts.cdata_key
                || k.starts_with(&opts.attr_prefix)
                || (k == opts.comment_key && is_handled_as_comment())
            {
                // comments are handled below
            }
            if k == opts.comment_key {
                emit_comment(out, &iv, opts, depth + 1)?;
                continue;
            }
            if k == opts.cdata_key || k.starts_with(&opts.attr_prefix) {
                continue;
            }
            if let Ok(l) = iv.cast::<PyList>() {
                if l.is_empty() {
                    continue;
                }
            }
            emit_element(py, out, &k, &iv, opts, depth + 1)?;
        }

        if let Some(ref cd) = cdata {
            append_escaped_text(out, cd);
        }

        if opts.pretty && has_children_or_comments {
            push_indent(out, &opts.indent, depth);
        }

        out.push_str("</");
        out.push_str(elem_name);
        out.push('>');
    }

    if opts.pretty && depth > 0 {
        out.push_str(&opts.newl);
    } else if opts.pretty && depth == 0 {
        // trailing newline after root for pretty mode; match xmltodict
        out.push_str(&opts.newl);
    }
    Ok(())
}

fn is_handled_as_comment() -> bool {
    true
}

fn emit_comment(
    out: &mut String,
    value: &Bound<'_, PyAny>,
    opts: &UnparseOpts,
    depth: usize,
) -> PyResult<()> {
    // value may be a string or list of strings.
    let items: Vec<Py<PyAny>> = if let Ok(l) = value.cast::<PyList>() {
        l.iter().map(|x| x.unbind()).collect()
    } else {
        vec![value.clone().unbind()]
    };
    for item in items {
        let bound = item.into_bound(value.py());
        if bound.is_none() {
            continue;
        }
        let text = convert_value_to_string(&bound)?;
        if text.is_empty() {
            continue;
        }
        validate_comment(&text)?;
        if opts.pretty {
            push_indent(out, &opts.indent, depth);
        }
        out.push_str("<!--");
        out.push_str(&text);
        out.push_str("-->");
        if opts.pretty {
            out.push_str(&opts.newl);
        }
    }
    Ok(())
}

// ---------- helpers ----------

fn expand_value<'py>(py: Python<'py>, v: &Bound<'py, PyAny>) -> PyResult<Vec<Bound<'py, PyAny>>> {
    // Wrap scalars/dicts as single-element list; iterables of items stay multi.
    if v.is_none()
        || v.cast::<PyString>().is_ok()
        || v.cast::<PyBytes>().is_ok()
        || v.cast::<PyByteArray>().is_ok()
        || v.cast::<PyDict>().is_ok()
        || v.cast::<PyBool>().is_ok()
        || v.cast::<PyInt>().is_ok()
        || v.cast::<PyFloat>().is_ok()
    {
        return Ok(vec![v.clone()]);
    }
    if let Ok(l) = v.cast::<PyList>() {
        return Ok(l.iter().collect());
    }
    if let Ok(t) = v.cast::<PyTuple>() {
        return Ok(t.iter().collect());
    }
    // Fallback: try to convert to string.
    let s = convert_value_to_string(v)?;
    Ok(vec![PyString::new(py, &s).into_any()])
}

fn convert_value_to_string(v: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(s) = v.cast::<PyString>() {
        return Ok(s.to_str()?.to_owned());
    }
    if let Ok(b) = v.cast::<PyBool>() {
        // Must check bool BEFORE int since bool is-a int in Python.
        return Ok(if b.is_true() {
            "true".to_owned()
        } else {
            "false".to_owned()
        });
    }
    if let Ok(b) = v.cast::<PyBytes>() {
        return Ok(String::from_utf8_lossy(b.as_bytes()).into_owned());
    }
    if let Ok(ba) = v.cast::<PyByteArray>() {
        // SAFETY: We copy immediately and don't hold the GIL elsewhere.
        let bytes = unsafe { ba.as_bytes() };
        return Ok(String::from_utf8_lossy(bytes).into_owned());
    }
    // Fallback: Python str()
    Ok(v.str()?.to_str()?.to_owned())
}

fn validate_name(name: &str, kind: &str) -> PyResult<()> {
    if name.starts_with('?') || name.starts_with('!') {
        return Err(PyValueError::new_err(format!(
            "Invalid {kind} name: cannot start with \"?\" or \"!\""
        )));
    }
    for ch in name.chars() {
        match ch {
            '<' | '>' => {
                return Err(PyValueError::new_err(format!(
                    "Invalid {kind} name: \"<\" or \">\" not allowed"
                )));
            }
            '/' => {
                return Err(PyValueError::new_err(format!(
                    "Invalid {kind} name: \"/\" not allowed"
                )));
            }
            '"' | '\'' => {
                return Err(PyValueError::new_err(format!(
                    "Invalid {kind} name: quotes not allowed"
                )));
            }
            '=' => {
                return Err(PyValueError::new_err(format!(
                    "Invalid {kind} name: \"=\" not allowed"
                )));
            }
            c if c.is_whitespace() => {
                return Err(PyValueError::new_err(format!(
                    "Invalid {kind} name: whitespace not allowed"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_comment(text: &str) -> PyResult<()> {
    if text.contains("--") {
        return Err(PyValueError::new_err("Comment text cannot contain '--'"));
    }
    if text.ends_with('-') {
        return Err(PyValueError::new_err("Comment text cannot end with '-'"));
    }
    Ok(())
}

fn push_indent(out: &mut String, indent: &str, depth: usize) {
    for _ in 0..depth {
        out.push_str(indent);
    }
}

fn append_escaped_text(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn append_escaped_attr(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(ch),
        }
    }
}

/// Rewrite `name` via the namespaces dict if it has a namespace prefix.
/// This is the inverse of xmltodict's `_process_namespace`: `name` = `prefix:local`
/// where `prefix` is a shortened key; we look it up and substitute the URI.
fn process_namespace(name: &str, opts: &UnparseOpts, is_attr: bool) -> String {
    let Some(ref ns_table) = opts.namespaces else {
        return name.to_owned();
    };
    let (ns_part, local) = match name.rsplit_once(&opts.namespace_separator) {
        Some((n, l)) => (n, l),
        None => return name.to_owned(),
    };
    let lookup_key = if is_attr {
        ns_part.trim_start_matches(opts.attr_prefix.as_str())
    } else {
        ns_part
    };
    let mut resolved: Option<&str> = None;
    for (uri, short) in ns_table {
        if short == lookup_key {
            resolved = Some(uri.as_str());
            break;
        }
    }
    match resolved {
        Some(uri) => {
            if is_attr && ns_part.starts_with(opts.attr_prefix.as_str()) {
                format!(
                    "{}{}{}{}",
                    opts.attr_prefix, uri, opts.namespace_separator, local
                )
            } else {
                format!("{}{}{}", uri, opts.namespace_separator, local)
            }
        }
        None => name.to_owned(),
    }
}
