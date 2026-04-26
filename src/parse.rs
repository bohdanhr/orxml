//! parse(xml, **opts) -> dict

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyString};
use quick_xml::events::Event;
use quick_xml::name::{Namespace, QName, ResolveResult};
use quick_xml::NsReader;

use crate::errors::ParseError;
use crate::opts::{ForceList, ParseOpts};

/// Parse XML input into a Python dict.
#[pyfunction]
#[pyo3(signature = (xml_input, **kwargs))]
pub fn parse<'py>(
    py: Python<'py>,
    xml_input: &Bound<'py, PyAny>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let opts = ParseOpts::from_kwargs(kwargs)?;
    let bytes: Vec<u8> = if let Ok(s) = xml_input.cast::<PyString>() {
        s.to_str()?.as_bytes().to_vec()
    } else if let Ok(b) = xml_input.cast::<PyBytes>() {
        b.as_bytes().to_vec()
    } else {
        return Err(PyTypeError::new_err(
            "orxml.parse: xml_input must be str or bytes",
        ));
    };
    parse_bytes(py, &bytes, &opts)
}

/// Saved parent context for the currently-open element.
struct Frame {
    item: Option<Py<PyAny>>,
    data: Vec<String>,
    name: String,
}

/// Owned, reader-independent representation of a single XML event.
#[allow(clippy::large_enum_variant)]
enum RawEvent {
    Start {
        /// QName bytes of the element.
        qname: Vec<u8>,
        /// Resolved namespace URI for the element, if any.
        ns_uri: Option<Vec<u8>>,
        /// (attribute QName bytes, decoded value)
        attrs: Vec<(Vec<u8>, String)>,
    },
    End {
        qname: Vec<u8>,
        ns_uri: Option<Vec<u8>>,
    },
    Empty {
        qname: Vec<u8>,
        ns_uri: Option<Vec<u8>>,
        attrs: Vec<(Vec<u8>, String)>,
    },
    Text(String),
    Comment(String),
    DocType(String),
    GeneralRef(String),
    Ignore,
    Eof,
}

fn parse_bytes<'py>(
    py: Python<'py>,
    input: &[u8],
    opts: &ParseOpts,
) -> PyResult<Bound<'py, PyAny>> {
    let mut reader = NsReader::from_reader(input);
    {
        let cfg = reader.config_mut();
        cfg.trim_text(false);
        cfg.expand_empty_elements = false;
        cfg.check_end_names = true;
    }
    let decoder = reader.decoder();

    let mut buf = Vec::with_capacity(256);

    let mut stack: Vec<Frame> = Vec::with_capacity(16);
    let mut root_item: Option<Py<PyAny>> = None;
    let mut cur_item: Option<Py<PyAny>> = None;
    let mut cur_data: Vec<String> = Vec::new();
    let mut cur_name: Option<String> = None;

    loop {
        // Phase 1: read event and collect all needed info as owned data so the
        // reader borrow is released before we call other reader methods.
        let raw: RawEvent = {
            let ev = reader
                .read_resolved_event_into(&mut buf)
                .map_err(|e| ParseError::new_err(format!("{e}")))?;
            match ev {
                (res, Event::Start(start)) => {
                    let qname_bytes = start.name().as_ref().to_vec();
                    let ns_uri = resolution_to_owned(&res);
                    let attrs = collect_raw_attrs(&start, decoder)?;
                    RawEvent::Start {
                        qname: qname_bytes,
                        ns_uri,
                        attrs,
                    }
                }
                (res, Event::End(end)) => RawEvent::End {
                    qname: end.name().as_ref().to_vec(),
                    ns_uri: resolution_to_owned(&res),
                },
                (res, Event::Empty(start)) => {
                    let qname_bytes = start.name().as_ref().to_vec();
                    let ns_uri = resolution_to_owned(&res);
                    let attrs = collect_raw_attrs(&start, decoder)?;
                    RawEvent::Empty {
                        qname: qname_bytes,
                        ns_uri,
                        attrs,
                    }
                }
                (_, Event::Text(bt)) => {
                    let txt = bt
                        .decode()
                        .map_err(|e| ParseError::new_err(format!("{e}")))?
                        .into_owned();
                    RawEvent::Text(txt)
                }
                (_, Event::CData(cd)) => {
                    let txt = std::str::from_utf8(cd.as_ref())
                        .map_err(|e| ParseError::new_err(format!("{e}")))?
                        .to_owned();
                    RawEvent::Text(txt)
                }
                (_, Event::Comment(bc)) => {
                    let txt = bc
                        .decode()
                        .map_err(|e| ParseError::new_err(format!("{e}")))?
                        .into_owned();
                    RawEvent::Comment(txt)
                }
                (_, Event::DocType(dt)) => {
                    let s = dt
                        .decode()
                        .map_err(|e| ParseError::new_err(format!("{e}")))?
                        .into_owned();
                    RawEvent::DocType(s)
                }
                (_, Event::GeneralRef(gr)) => {
                    let s = gr
                        .decode()
                        .map_err(|e| ParseError::new_err(format!("{e}")))?
                        .into_owned();
                    RawEvent::GeneralRef(s)
                }
                (_, Event::Eof) => RawEvent::Eof,
                _ => RawEvent::Ignore,
            }
        };

        // Phase 2: process the owned event.
        match raw {
            RawEvent::Start {
                qname,
                ns_uri,
                attrs,
            } => {
                let name = build_elem_name(&qname, ns_uri.as_deref(), opts);
                let attrs_obj = build_attrs(py, &reader, &attrs, opts)?;
                stack.push(Frame {
                    item: cur_item.take(),
                    data: std::mem::take(&mut cur_data),
                    name: cur_name.take().unwrap_or_default(),
                });
                cur_item = attrs_obj;
                cur_data.clear();
                cur_name = Some(name);
            }
            RawEvent::End { qname, ns_uri } => {
                let name = build_elem_name(&qname, ns_uri.as_deref(), opts);
                let item_local = cur_item.take();
                let data_local = std::mem::take(&mut cur_data);
                let parent = stack.pop().unwrap_or(Frame {
                    item: None,
                    data: Vec::new(),
                    name: String::new(),
                });
                cur_item = parent.item;
                cur_data = parent.data;
                cur_name = if parent.name.is_empty() {
                    None
                } else {
                    Some(parent.name)
                };
                close_element(py, &name, item_local, data_local, &mut cur_item, opts)?;
                if stack.is_empty() {
                    root_item = cur_item.take();
                }
            }
            RawEvent::Empty {
                qname,
                ns_uri,
                attrs,
            } => {
                let name = build_elem_name(&qname, ns_uri.as_deref(), opts);
                let attrs_obj = build_attrs(py, &reader, &attrs, opts)?;
                // Synthesize Start+End in-place.
                stack.push(Frame {
                    item: cur_item.take(),
                    data: std::mem::take(&mut cur_data),
                    name: cur_name.take().unwrap_or_default(),
                });
                let item_local: Option<Py<PyAny>> = attrs_obj;
                let parent = stack.pop().unwrap();
                cur_item = parent.item;
                cur_data = parent.data;
                cur_name = if parent.name.is_empty() {
                    None
                } else {
                    Some(parent.name)
                };
                close_element(py, &name, item_local, Vec::new(), &mut cur_item, opts)?;
                if stack.is_empty() {
                    root_item = cur_item.take();
                }
            }
            RawEvent::Text(txt) => {
                if !txt.is_empty() {
                    cur_data.push(txt);
                }
            }
            RawEvent::Comment(mut txt) => {
                if opts.process_comments && cur_name.is_some() {
                    if opts.strip_whitespace {
                        txt = txt.trim().to_owned();
                    }
                    let ck = opts.comment_key.clone();
                    push_data(
                        py,
                        &mut cur_item,
                        &ck,
                        PyString::new(py, &txt).into_any().unbind(),
                        opts,
                    )?;
                }
            }
            RawEvent::DocType(s) => {
                if opts.disable_entities {
                    let lower = s.to_ascii_lowercase();
                    if lower.contains("<!entity") || lower.contains("!entity") {
                        return Err(ParseError::new_err("entities are disabled".to_string()));
                    }
                }
            }
            RawEvent::GeneralRef(raw) => {
                // Decode predefined and numeric character references inline;
                // only truly user-defined (DTD) entities are blocked by
                // disable_entities.
                if let Some(decoded) = decode_predefined_entity(&raw) {
                    cur_data.push(decoded);
                } else if opts.disable_entities {
                    return Err(ParseError::new_err("entities are disabled".to_string()));
                } else {
                    cur_data.push(format!("&{raw};"));
                }
            }
            RawEvent::Ignore => {}
            RawEvent::Eof => break,
        }

        buf.clear();
    }

    match root_item {
        Some(obj) => Ok(obj.into_bound(py)),
        None => Ok(py.None().into_bound(py)),
    }
}

// ---------- event helpers ----------

fn resolution_to_owned(res: &ResolveResult<'_>) -> Option<Vec<u8>> {
    match res {
        ResolveResult::Bound(ns) => Some(ns.as_ref().to_vec()),
        ResolveResult::Unbound | ResolveResult::Unknown(_) => None,
    }
}

fn collect_raw_attrs(
    start: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::Decoder,
) -> PyResult<Vec<(Vec<u8>, String)>> {
    let mut out = Vec::new();
    for res in start.attributes() {
        let a = res.map_err(|e| ParseError::new_err(format!("{e}")))?;
        let key = a.key.as_ref().to_vec();
        let val = a
            .decode_and_unescape_value(decoder)
            .map_err(|e| ParseError::new_err(format!("{e}")))?
            .into_owned();
        out.push((key, val));
    }
    Ok(out)
}

// ---------- element/attribute naming ----------

fn build_elem_name(qname_bytes: &[u8], ns_uri: Option<&[u8]>, opts: &ParseOpts) -> String {
    if !opts.process_namespaces {
        return std::str::from_utf8(qname_bytes).unwrap_or("").to_owned();
    }
    let q = QName(qname_bytes);
    let local = std::str::from_utf8(q.local_name().as_ref())
        .unwrap_or("")
        .to_owned();
    match ns_uri {
        Some(uri_bytes) => {
            let uri = std::str::from_utf8(uri_bytes).unwrap_or("");
            if let Some(short) = lookup_ns_short(&opts.namespaces, uri) {
                if short.is_empty() {
                    local
                } else {
                    format!("{short}{}{local}", opts.namespace_separator)
                }
            } else {
                format!("{uri}{}{local}", opts.namespace_separator)
            }
        }
        None => local,
    }
}

fn build_attrs<'py>(
    py: Python<'py>,
    reader: &NsReader<&[u8]>,
    attrs_raw: &[(Vec<u8>, String)],
    opts: &ParseOpts,
) -> PyResult<Option<Py<PyAny>>> {
    if !opts.xml_attribs {
        return Ok(None);
    }
    let d = PyDict::new(py);
    let mut any = false;
    let mut xmlns_entries: Vec<(String, String)> = Vec::new();

    for (key_bytes, val) in attrs_raw {
        let is_xmlns = key_bytes.as_slice() == b"xmlns" || key_bytes.starts_with(b"xmlns:");

        if is_xmlns && opts.process_namespaces {
            let prefix = if key_bytes.as_slice() == b"xmlns" {
                String::new()
            } else {
                std::str::from_utf8(&key_bytes[b"xmlns:".len()..])
                    .unwrap_or("")
                    .to_owned()
            };
            xmlns_entries.push((prefix, val.clone()));
            continue;
        }

        let name_str = resolve_attr_name(reader, key_bytes, opts);
        let key = format!("{}{name_str}", opts.attr_prefix);
        d.set_item(&key, val)?;
        any = true;
    }

    if !xmlns_entries.is_empty() {
        let xmlns_dict = PyDict::new(py);
        for (p, u) in xmlns_entries {
            xmlns_dict.set_item(p, u)?;
        }
        let key = format!("{}xmlns", opts.attr_prefix);
        d.set_item(&key, xmlns_dict)?;
        any = true;
    }

    if any {
        Ok(Some(d.into_any().unbind()))
    } else {
        Ok(None)
    }
}

fn resolve_attr_name(reader: &NsReader<&[u8]>, key_bytes: &[u8], opts: &ParseOpts) -> String {
    let qname = QName(key_bytes);
    if !opts.process_namespaces {
        return std::str::from_utf8(qname.as_ref())
            .unwrap_or("")
            .to_owned();
    }
    let (res, local) = reader.resolver().resolve_attribute(qname);
    let local_s = std::str::from_utf8(local.as_ref())
        .unwrap_or("")
        .to_owned();
    match res {
        ResolveResult::Bound(Namespace(uri_bytes)) => {
            let uri = std::str::from_utf8(uri_bytes).unwrap_or("");
            if let Some(short) = lookup_ns_short(&opts.namespaces, uri) {
                if short.is_empty() {
                    local_s
                } else {
                    format!("{short}{}{local_s}", opts.namespace_separator)
                }
            } else {
                format!("{uri}{}{local_s}", opts.namespace_separator)
            }
        }
        _ => local_s,
    }
}

/// Decode a predefined XML entity name or numeric character reference.
///
/// Returns `None` if `name` is a truly user-defined (DTD) entity reference.
fn decode_predefined_entity(name: &str) -> Option<String> {
    match name {
        "lt" => Some("<".to_owned()),
        "gt" => Some(">".to_owned()),
        "amp" => Some("&".to_owned()),
        "quot" => Some("\"".to_owned()),
        "apos" => Some("'".to_owned()),
        _ => {
            // Numeric char refs: &#N; (decimal) or &#xN; (hex)
            if let Some(rest) = name.strip_prefix('#') {
                let code = if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    rest.parse::<u32>().ok()?
                };
                return char::from_u32(code).map(|c| c.to_string());
            }
            None
        }
    }
}

fn lookup_ns_short<'a>(table: &'a Option<Vec<(String, String)>>, uri: &str) -> Option<&'a str> {
    let table = table.as_ref()?;
    for (k, v) in table {
        if k == uri {
            return Some(v);
        }
    }
    None
}

// ---------- element close / push_data ----------

fn close_element(
    py: Python<'_>,
    name: &str,
    item_local: Option<Py<PyAny>>,
    data_local: Vec<String>,
    cur_item: &mut Option<Py<PyAny>>,
    opts: &ParseOpts,
) -> PyResult<()> {
    let mut data_str: Option<String> = if data_local.is_empty() {
        None
    } else {
        Some(data_local.join(&opts.cdata_separator))
    };
    if opts.strip_whitespace {
        if let Some(s) = data_str.as_ref() {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                data_str = None;
            } else if trimmed.len() != s.len() {
                data_str = Some(trimmed.to_owned());
            }
        }
    }

    let force_this = opts.force_cdata.contains(name);

    let mut item = item_local;
    if let Some(ref text) = data_str {
        if force_this && item.is_none() {
            item = Some(PyDict::new(py).into_any().unbind());
        }
        if let Some(dict_obj) = item.as_ref() {
            let k = opts.cdata_key.clone();
            let pytext = PyString::new(py, text).into_any().unbind();
            let mut holder = Some(dict_obj.clone_ref(py));
            push_data(py, &mut holder, &k, pytext, opts)?;
            // (holder is the same dict mutated in place; item stays as-is.)
        }
    }

    if let Some(dict_obj) = item {
        push_data(py, cur_item, name, dict_obj, opts)?;
    } else if let Some(text) = data_str {
        push_data(
            py,
            cur_item,
            name,
            PyString::new(py, &text).into_any().unbind(),
            opts,
        )?;
    } else {
        push_data(py, cur_item, name, py.None(), opts)?;
    }
    Ok(())
}

fn push_data(
    py: Python<'_>,
    container: &mut Option<Py<PyAny>>,
    key: &str,
    value: Py<PyAny>,
    opts: &ParseOpts,
) -> PyResult<()> {
    if container.is_none() {
        *container = Some(PyDict::new(py).into_any().unbind());
    }
    let owner = container.as_ref().unwrap().clone_ref(py);
    let bound = owner.into_bound(py);
    let d = bound
        .cast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("internal error: container is not a dict"))?;

    let key_py = PyString::new(py, key);
    if let Some(existing) = d.get_item(&key_py)? {
        if let Ok(lst) = existing.cast::<PyList>() {
            lst.append(value.into_bound(py))?;
        } else {
            let new_list = PyList::empty(py);
            new_list.append(existing)?;
            new_list.append(value.into_bound(py))?;
            d.set_item(&key_py, new_list)?;
        }
    } else if matches!(opts.force_list, ForceList::All) || opts.force_list.contains(key) {
        let new_list = PyList::empty(py);
        new_list.append(value.into_bound(py))?;
        d.set_item(&key_py, new_list)?;
    } else {
        d.set_item(&key_py, value.into_bound(py))?;
    }
    Ok(())
}
