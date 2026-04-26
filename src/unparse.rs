//! unparse(dict, **opts) -> str
//!
//! Hand-written XML emitter walking a Python dict. We don't use quick-xml's
//! Writer here because we want full control over attribute ordering (insertion
//! order, matching xmltodict) and over pretty-printing with arbitrary
//! `indent`/`newl` strings.
//!
//! Hot-path design notes:
//! * Scalar children (str/int/bool/bytes/None) take a dedicated leaf path that
//!   never constructs an intermediate PyDict and never collects children.
//! * For dict-shaped values we walk the mapping exactly once: attributes are
//!   streamed directly to the output buffer, while children + comments are
//!   captured into a single `OrderedChild` vector so their original insertion
//!   order is preserved without re-iterating the dict.
//! * Text/attribute escaping scans the raw UTF-8 bytes and bulk-copies clean
//!   ASCII runs, falling back to entity writes only for the escape triggers.

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
    // `key` may carry a namespace prefix that needs rewriting via opts.namespaces.
    let elem_name = process_namespace(key, opts, false);
    validate_name(&elem_name, "element")?;

    // Fast path: value isn't an iterable-of-instances, so we don't need the
    // expand_value() heap allocation.
    if !is_instance_iterable(value) {
        return emit_one(py, out, &elem_name, value, opts, depth);
    }

    let items: Vec<Bound<'_, PyAny>> = if let Ok(l) = value.cast::<PyList>() {
        l.iter().collect()
    } else if let Ok(t) = value.cast::<PyTuple>() {
        t.iter().collect()
    } else {
        // Defensive — is_instance_iterable already guarded this.
        return emit_one(py, out, &elem_name, value, opts, depth);
    };

    for (i, v) in items.iter().enumerate() {
        if opts.full_document && depth == 0 && i > 0 {
            return Err(PyValueError::new_err(
                "Document must have exactly one root.",
            ));
        }
        emit_one(py, out, &elem_name, v, opts, depth)?;
    }
    Ok(())
}

/// Entries captured during the single dict walk. Attributes stream directly
/// into the output buffer so they don't need an intermediate representation.
enum OrderedChild {
    Element(String, Py<PyAny>),
    Comment(Py<PyAny>),
}

fn emit_one(
    py: Python<'_>,
    out: &mut String,
    elem_name: &str,
    v: &Bound<'_, PyAny>,
    opts: &UnparseOpts,
    depth: usize,
) -> PyResult<()> {
    // ---- scalar fast paths (no attrs, no children, possibly no text) ----
    if v.is_none() {
        emit_leaf(out, elem_name, None, opts, depth);
        return Ok(());
    }
    if let Ok(s) = v.cast::<PyString>() {
        emit_leaf(out, elem_name, Some(s.to_str()?), opts, depth);
        return Ok(());
    }
    if let Ok(b) = v.cast::<PyBool>() {
        // PyBool must be checked before PyInt since bool is-a int in Python.
        emit_leaf(
            out,
            elem_name,
            Some(if b.is_true() { "true" } else { "false" }),
            opts,
            depth,
        );
        return Ok(());
    }
    if let Ok(b) = v.cast::<PyBytes>() {
        let s = String::from_utf8_lossy(b.as_bytes());
        emit_leaf(out, elem_name, Some(&s), opts, depth);
        return Ok(());
    }
    if let Ok(ba) = v.cast::<PyByteArray>() {
        // SAFETY: GIL is held for the duration of this borrow and we copy
        // immediately.
        let bytes = unsafe { ba.as_bytes() };
        let s = String::from_utf8_lossy(bytes);
        emit_leaf(out, elem_name, Some(&s), opts, depth);
        return Ok(());
    }
    if v.cast::<PyInt>().is_ok() || v.cast::<PyFloat>().is_ok() {
        let py_s = v.str()?;
        emit_leaf(out, elem_name, Some(py_s.to_str()?), opts, depth);
        return Ok(());
    }

    // ---- dict path ----
    let v_dict: &Bound<'_, PyDict> = match v.cast::<PyDict>() {
        Ok(d) => d,
        Err(_) => {
            // Arbitrary object → str() and treat as scalar text.
            let py_s = v.str()?;
            emit_leaf(out, elem_name, Some(py_s.to_str()?), opts, depth);
            return Ok(());
        }
    };

    // Open tag is emitted up-front so we can stream attributes in place during
    // the dict walk.
    if opts.pretty {
        push_indent(out, &opts.indent, depth);
    }
    out.push('<');
    out.push_str(elem_name);

    let mut cdata_holder: Option<String> = None;
    let mut ordered: Vec<OrderedChild> = Vec::new();

    for (ik, iv) in v_dict.iter() {
        let key: String = ik.extract()?;

        if key == opts.cdata_key {
            cdata_holder = if iv.is_none() {
                None
            } else {
                Some(convert_value_to_string(&iv)?)
            };
            continue;
        }

        if key.starts_with(&opts.attr_prefix) {
            // @xmlns as a nested dict expands into xmlns / xmlns:prefix attrs.
            if key == opts.xmlns_attr_key {
                if let Ok(nsd) = iv.cast::<PyDict>() {
                    for (nk, nv) in nsd.iter() {
                        let prefix: String = nk.extract()?;
                        validate_name(&prefix, "attribute")?;
                        out.push(' ');
                        if prefix.is_empty() {
                            out.push_str("xmlns");
                        } else {
                            out.push_str("xmlns:");
                            out.push_str(&prefix);
                        }
                        out.push_str("=\"");
                        if !nv.is_none() {
                            write_attr_value(out, &nv)?;
                        }
                        out.push('"');
                    }
                    continue;
                }
                // Fallthrough: `@xmlns` with a non-dict value is treated as a
                // plain attribute below.
            }
            let attr_name_raw = &key[opts.attr_prefix.len()..];
            let attr_name = process_namespace(attr_name_raw, opts, true);
            validate_name(&attr_name, "attribute")?;
            out.push(' ');
            out.push_str(&attr_name);
            out.push_str("=\"");
            if !iv.is_none() {
                write_attr_value(out, &iv)?;
            }
            out.push('"');
            continue;
        }

        if key == opts.comment_key {
            ordered.push(OrderedChild::Comment(iv.unbind()));
            continue;
        }

        // Skip empty lists (xmltodict behavior).
        if let Ok(l) = iv.cast::<PyList>() {
            if l.is_empty() {
                continue;
            }
        }
        ordered.push(OrderedChild::Element(key, iv.unbind()));
    }

    let has_children_or_comments = !ordered.is_empty();
    let short_empty = opts.short_empty_elements
        && cdata_holder.is_none()
        && !has_children_or_comments;

    if short_empty {
        out.push_str("/>");
    } else {
        out.push('>');

        if opts.pretty && has_children_or_comments {
            out.push_str(&opts.newl);
        }

        for child in &ordered {
            match child {
                OrderedChild::Comment(item) => {
                    let bound = item.bind(py);
                    emit_comment(out, bound, opts, depth + 1)?;
                }
                OrderedChild::Element(name, item) => {
                    let bound = item.bind(py);
                    emit_element(py, out, name, bound, opts, depth + 1)?;
                }
            }
        }

        if let Some(ref cd) = cdata_holder {
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
        // Trailing newline acts as a separator between siblings. The root
        // element (depth == 0) is the last thing emitted and gets no trailing
        // newline, matching xmltodict's pretty output byte-for-byte.
        out.push_str(&opts.newl);
    }
    Ok(())
}

/// Emit a leaf element with optional text content, handling pretty-printing
/// and short_empty_elements.
fn emit_leaf(
    out: &mut String,
    elem_name: &str,
    text: Option<&str>,
    opts: &UnparseOpts,
    depth: usize,
) {
    if opts.pretty {
        push_indent(out, &opts.indent, depth);
    }
    match text {
        None if opts.short_empty_elements => {
            out.push('<');
            out.push_str(elem_name);
            out.push_str("/>");
        }
        _ => {
            out.push('<');
            out.push_str(elem_name);
            out.push('>');
            if let Some(t) = text {
                append_escaped_text(out, t);
            }
            out.push_str("</");
            out.push_str(elem_name);
            out.push('>');
        }
    }
    if opts.pretty && depth > 0 {
        // Newline is a sibling separator, not a terminator; see emit_one.
        out.push_str(&opts.newl);
    }
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

/// A value is "instance-iterable" iff it should be expanded into siblings by
/// emit_element (list/tuple of instances). Scalars/dicts/None stay as a single
/// instance.
fn is_instance_iterable(v: &Bound<'_, PyAny>) -> bool {
    v.cast::<PyList>().is_ok() || v.cast::<PyTuple>().is_ok()
}

/// Streaming attribute value writer that avoids allocating for PyString.
fn write_attr_value(out: &mut String, v: &Bound<'_, PyAny>) -> PyResult<()> {
    if let Ok(s) = v.cast::<PyString>() {
        append_escaped_attr(out, s.to_str()?);
        return Ok(());
    }
    if let Ok(b) = v.cast::<PyBool>() {
        out.push_str(if b.is_true() { "true" } else { "false" });
        return Ok(());
    }
    if let Ok(b) = v.cast::<PyBytes>() {
        let s = String::from_utf8_lossy(b.as_bytes());
        append_escaped_attr(out, &s);
        return Ok(());
    }
    if let Ok(ba) = v.cast::<PyByteArray>() {
        let bytes = unsafe { ba.as_bytes() };
        let s = String::from_utf8_lossy(bytes);
        append_escaped_attr(out, &s);
        return Ok(());
    }
    // Catch-all: str() via Python.
    let py_s = v.str()?;
    append_escaped_attr(out, py_s.to_str()?);
    Ok(())
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

/// Append `s` to `out`, escaping XML text-content specials (`& < >`).
///
/// Scans raw UTF-8 bytes for the ASCII-only escape triggers using memchr3,
/// bulk-copying clean runs via `push_str`. Slicing at ASCII-byte boundaries
/// always yields valid UTF-8, which is why `from_utf8_unchecked` is safe here.
fn append_escaped_text(out: &mut String, s: &str) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match memchr::memchr3(b'&', b'<', b'>', &bytes[i..]) {
            Some(rel) => {
                let pos = i + rel;
                // SAFETY: bytes[i..pos] ends just before an ASCII byte, so the
                // slice is valid UTF-8.
                out.push_str(unsafe { std::str::from_utf8_unchecked(&bytes[i..pos]) });
                out.push_str(match bytes[pos] {
                    b'&' => "&amp;",
                    b'<' => "&lt;",
                    _ => "&gt;",
                });
                i = pos + 1;
            }
            None => {
                // SAFETY: bytes[i..] starts on a UTF-8 boundary because we
                // only advanced past ASCII escape bytes.
                out.push_str(unsafe { std::str::from_utf8_unchecked(&bytes[i..]) });
                break;
            }
        }
    }
}

/// Append `s` to `out`, escaping XML attribute-value specials
/// (`& < > " \t \n \r`).
///
/// Byte-loop variant (7 needles — memchr only goes up to 3). Still bulk-copies
/// clean runs between escape points.
fn append_escaped_attr(out: &mut String, s: &str) {
    let bytes = s.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        let ent = match bytes[i] {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\t' => "&#9;",
            b'\n' => "&#10;",
            b'\r' => "&#13;",
            _ => {
                i += 1;
                continue;
            }
        };
        // SAFETY: bytes[last..i] ends just before an ASCII byte.
        out.push_str(unsafe { std::str::from_utf8_unchecked(&bytes[last..i]) });
        out.push_str(ent);
        i += 1;
        last = i;
    }
    // SAFETY: bytes[last..] starts on a UTF-8 boundary.
    out.push_str(unsafe { std::str::from_utf8_unchecked(&bytes[last..]) });
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
