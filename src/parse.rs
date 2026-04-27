//! parse(xml, **opts) -> dict

use std::collections::HashMap;

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
    // Borrow input bytes directly from the PyString/PyBytes. The borrow lives
    // inside this function (GIL held throughout), so no copy is needed.
    if let Ok(s) = xml_input.cast::<PyString>() {
        parse_bytes(py, s.to_str()?.as_bytes(), &opts)
    } else if let Ok(b) = xml_input.cast::<PyBytes>() {
        parse_bytes(py, b.as_bytes(), &opts)
    } else {
        Err(PyTypeError::new_err(
            "orxml.parse: xml_input must be str or bytes",
        ))
    }
}

/// Saved parent context for the currently-open element.
struct Frame {
    item: Option<Py<PyAny>>,
    data: String,
    data_segments: u32,
    name: String,
}

/// PyString instances for option-controlled dict keys, bound once at the start
/// of a parse so `push_data` doesn't re-intern them on every call.
struct DerivedKeys<'py> {
    cdata: Bound<'py, PyString>,
    comment: Bound<'py, PyString>,
}

/// Cache of element/attribute name `PyString`s keyed by their Rust `&str`
/// form. Hot docs (e.g. RSS, NCPDP, any tabular XML) repeat a small set of
/// names thousands of times; interning saves a `PyString::new` allocation on
/// every repeat.
struct NameCache<'py> {
    map: HashMap<String, Bound<'py, PyString>>,
}

impl<'py> NameCache<'py> {
    fn new() -> Self {
        Self {
            map: HashMap::with_capacity(32),
        }
    }

    #[inline]
    fn get_or_intern(&mut self, py: Python<'py>, name: &str) -> Bound<'py, PyString> {
        if let Some(existing) = self.map.get(name) {
            return existing.clone();
        }
        let s = PyString::new(py, name);
        self.map.insert(name.to_owned(), s.clone());
        s
    }
}

/// Owned, reader-independent representation of a single XML event.
///
/// Only the *structural* events (Start/End/Empty) need to outlive the reader
/// borrow — text/comment/general-ref events are processed inline during phase
/// 1 where the `Event`'s borrow on `buf` is still live, avoiding the
/// `.into_owned()` copy. Attributes are likewise built into their final
/// `PyDict` form inline so there's no `Vec<(Vec<u8>, String)>` intermediate.
#[allow(clippy::large_enum_variant)]
enum RawEvent {
    Start {
        /// QName bytes of the element.
        qname: Vec<u8>,
        /// Resolved namespace URI for the element, if any.
        ns_uri: Option<Vec<u8>>,
        /// Pre-built attrs dict (None iff there were no attrs to emit).
        attrs: Option<Py<PyDict>>,
    },
    End {
        qname: Vec<u8>,
        ns_uri: Option<Vec<u8>>,
    },
    Empty {
        qname: Vec<u8>,
        ns_uri: Option<Vec<u8>>,
        attrs: Option<Py<PyDict>>,
    },
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

    let keys = DerivedKeys {
        cdata: PyString::new(py, &opts.cdata_key),
        comment: PyString::new(py, &opts.comment_key),
    };
    let mut names = NameCache::new();

    let mut stack: Vec<Frame> = Vec::with_capacity(16);
    let mut root_item: Option<Py<PyAny>> = None;
    let mut cur_item: Option<Py<PyAny>> = None;
    let mut cur_data: String = String::new();
    // Number of distinct text events already pushed into `cur_data`. Used to
    // insert `cdata_separator` between segments so we match xmltodict's
    // `Vec<String>::join(separator)` semantics without actually allocating a
    // Vec per element.
    let mut cur_segments: u32 = 0;
    let mut cur_name: Option<String> = None;

    loop {
        // Phase 1: read event. Structural events (Start/End/Empty) become
        // owned `RawEvent`s so we can call `&reader` methods on them in phase
        // 2. Text-like events are handled inline here — the `Event` still
        // borrows from `buf` but we only need to copy bytes out, not re-borrow
        // the reader.
        let raw: RawEvent = {
            let ev = reader
                .read_resolved_event_into(&mut buf)
                .map_err(|e| ParseError::new_err(format!("{e}")))?;
            match ev {
                (res, Event::Start(start)) => {
                    let qname_bytes = start.name().as_ref().to_vec();
                    let ns_uri = resolution_to_owned(&res);
                    let attrs = build_attrs_inline(py, &reader, &start, decoder, opts)?;
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
                    let attrs = build_attrs_inline(py, &reader, &start, decoder, opts)?;
                    RawEvent::Empty {
                        qname: qname_bytes,
                        ns_uri,
                        attrs,
                    }
                }
                (_, Event::Text(bt)) => {
                    let txt = bt
                        .decode()
                        .map_err(|e| ParseError::new_err(format!("{e}")))?;
                    if !txt.is_empty() {
                        append_text_segment(
                            &mut cur_data,
                            &mut cur_segments,
                            &opts.cdata_separator,
                            &txt,
                        );
                    }
                    RawEvent::Ignore
                }
                (_, Event::CData(cd)) => {
                    let txt = std::str::from_utf8(cd.as_ref())
                        .map_err(|e| ParseError::new_err(format!("{e}")))?;
                    if !txt.is_empty() {
                        append_text_segment(
                            &mut cur_data,
                            &mut cur_segments,
                            &opts.cdata_separator,
                            txt,
                        );
                    }
                    RawEvent::Ignore
                }
                (_, Event::Comment(bc)) => {
                    if opts.process_comments && cur_name.is_some() {
                        let decoded = bc
                            .decode()
                            .map_err(|e| ParseError::new_err(format!("{e}")))?;
                        let text = if opts.strip_whitespace {
                            decoded.trim()
                        } else {
                            &decoded
                        };
                        let val = PyString::new(py, text).into_any().unbind();
                        push_data(py, &mut cur_item, &keys.comment, val, opts)?;
                    }
                    RawEvent::Ignore
                }
                (_, Event::DocType(dt)) => {
                    if opts.disable_entities {
                        let s = dt
                            .decode()
                            .map_err(|e| ParseError::new_err(format!("{e}")))?;
                        if doctype_has_entity_decl(&s) {
                            return Err(ParseError::new_err(
                                "entities are disabled".to_string(),
                            ));
                        }
                    }
                    RawEvent::Ignore
                }
                (_, Event::GeneralRef(gr)) => {
                    let raw = gr
                        .decode()
                        .map_err(|e| ParseError::new_err(format!("{e}")))?;
                    // Decode predefined and numeric character references
                    // inline; only truly user-defined (DTD) entities are
                    // blocked by `disable_entities`.
                    if let Some(decoded) = decode_predefined_entity(&raw) {
                        append_text_segment(
                            &mut cur_data,
                            &mut cur_segments,
                            &opts.cdata_separator,
                            &decoded,
                        );
                    } else if opts.disable_entities {
                        return Err(ParseError::new_err("entities are disabled".to_string()));
                    } else {
                        // Pass the entity reference through literally.
                        let mut literal = String::with_capacity(raw.len() + 2);
                        literal.push('&');
                        literal.push_str(&raw);
                        literal.push(';');
                        append_text_segment(
                            &mut cur_data,
                            &mut cur_segments,
                            &opts.cdata_separator,
                            &literal,
                        );
                    }
                    RawEvent::Ignore
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
                stack.push(Frame {
                    item: cur_item.take(),
                    data: std::mem::take(&mut cur_data),
                    data_segments: std::mem::take(&mut cur_segments),
                    name: cur_name.take().unwrap_or_default(),
                });
                cur_item = attrs.map(|d| d.into_any());
                cur_name = Some(name);
            }
            RawEvent::End { qname, ns_uri } => {
                let name = build_elem_name(&qname, ns_uri.as_deref(), opts);
                let item_local = cur_item.take();
                let data_local = std::mem::take(&mut cur_data);
                // Segment count for the closed element isn't needed downstream
                // (close_element just consumes the already-assembled string),
                // so we don't thread it through.
                let _ = std::mem::take(&mut cur_segments);
                let parent = stack.pop().unwrap_or(Frame {
                    item: None,
                    data: String::new(),
                    data_segments: 0,
                    name: String::new(),
                });
                cur_item = parent.item;
                cur_data = parent.data;
                cur_segments = parent.data_segments;
                cur_name = if parent.name.is_empty() {
                    None
                } else {
                    Some(parent.name)
                };
                close_element(
                    py,
                    &name,
                    item_local,
                    data_local,
                    &mut cur_item,
                    opts,
                    &keys,
                    &mut names,
                )?;
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
                // No text belongs to an empty element, so we don't touch
                // `cur_data` / `cur_segments`.
                let item_local: Option<Py<PyAny>> = attrs.map(|d| d.into_any());
                close_element(
                    py,
                    &name,
                    item_local,
                    String::new(),
                    &mut cur_item,
                    opts,
                    &keys,
                    &mut names,
                )?;
                if stack.is_empty() {
                    root_item = cur_item.take();
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

/// Append a text segment to the running cdata accumulator, inserting the
/// cdata separator between distinct segments (to match xmltodict's
/// `cdata_separator.join(list)` semantics).
#[inline]
fn append_text_segment(data: &mut String, segments: &mut u32, separator: &str, text: &str) {
    if *segments > 0 && !separator.is_empty() {
        data.push_str(separator);
    }
    data.push_str(text);
    *segments += 1;
}

/// Case-insensitive scan for `<!ENTITY` / `!ENTITY` substrings in a DOCTYPE
/// declaration body, without copying the whole string to lowercase.
fn doctype_has_entity_decl(s: &str) -> bool {
    let bytes = s.as_bytes();
    const NEEDLE: &[u8] = b"!entity";
    if bytes.len() < NEEDLE.len() {
        return false;
    }
    let end = bytes.len() - NEEDLE.len() + 1;
    let mut i = 0;
    while i < end {
        let window = &bytes[i..i + NEEDLE.len()];
        let mut j = 0;
        while j < NEEDLE.len() {
            if !window[j].eq_ignore_ascii_case(&NEEDLE[j]) {
                break;
            }
            j += 1;
        }
        if j == NEEDLE.len() {
            return true;
        }
        i += 1;
    }
    false
}

fn resolution_to_owned(res: &ResolveResult<'_>) -> Option<Vec<u8>> {
    match res {
        ResolveResult::Bound(ns) => Some(ns.as_ref().to_vec()),
        ResolveResult::Unbound | ResolveResult::Unknown(_) => None,
    }
}

/// Walk the attribute iterator on a Start/Empty event and build the final
/// Python attrs dict directly — no `Vec<(Vec<u8>, String)>` intermediate,
/// and no `.into_owned()` on the decoded values (the `Cow` is passed straight
/// to `PyString::new` via `set_item(&str)`).
fn build_attrs_inline<'py>(
    py: Python<'py>,
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::Decoder,
    opts: &ParseOpts,
) -> PyResult<Option<Py<PyDict>>> {
    if !opts.xml_attribs {
        return Ok(None);
    }

    let d = PyDict::new(py);
    let mut any = false;
    let mut xmlns_dict: Option<Bound<'py, PyDict>> = None;

    for res in start.attributes() {
        let a = res.map_err(|e| ParseError::new_err(format!("{e}")))?;
        let key_bytes = a.key.as_ref();
        let is_xmlns = key_bytes == b"xmlns" || key_bytes.starts_with(b"xmlns:");

        if is_xmlns && opts.process_namespaces {
            // xmlns bindings are collapsed into a nested `@xmlns` dict when
            // process_namespaces=True, matching xmltodict's model.
            let xd = xmlns_dict.get_or_insert_with(|| PyDict::new(py));
            let prefix_bytes: &[u8] = if key_bytes == b"xmlns" {
                &[]
            } else {
                &key_bytes[b"xmlns:".len()..]
            };
            let prefix = std::str::from_utf8(prefix_bytes).unwrap_or("");
            let value = a
                .decode_and_unescape_value(decoder)
                .map_err(|e| ParseError::new_err(format!("{e}")))?;
            xd.set_item(prefix, value.as_ref())?;
            continue;
        }

        let name_str = resolve_attr_name(reader, key_bytes, opts);
        let mut key_buf = String::with_capacity(opts.attr_prefix.len() + name_str.len());
        key_buf.push_str(&opts.attr_prefix);
        key_buf.push_str(&name_str);
        let value = a
            .decode_and_unescape_value(decoder)
            .map_err(|e| ParseError::new_err(format!("{e}")))?;
        d.set_item(&key_buf, value.as_ref())?;
        any = true;
    }

    if let Some(xd) = xmlns_dict {
        let mut key_buf = String::with_capacity(opts.attr_prefix.len() + "xmlns".len());
        key_buf.push_str(&opts.attr_prefix);
        key_buf.push_str("xmlns");
        d.set_item(&key_buf, xd)?;
        any = true;
    }

    if any {
        Ok(Some(d.unbind()))
    } else {
        Ok(None)
    }
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

fn resolve_attr_name(reader: &NsReader<&[u8]>, key_bytes: &[u8], opts: &ParseOpts) -> String {
    let qname = QName(key_bytes);
    if !opts.process_namespaces {
        return std::str::from_utf8(qname.as_ref()).unwrap_or("").to_owned();
    }
    let (res, local) = reader.resolver().resolve_attribute(qname);
    let local_s = std::str::from_utf8(local.as_ref()).unwrap_or("").to_owned();
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
                let code =
                    if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
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

#[allow(clippy::too_many_arguments)]
fn close_element<'py>(
    py: Python<'py>,
    name: &str,
    item_local: Option<Py<PyAny>>,
    mut data_local: String,
    cur_item: &mut Option<Py<PyAny>>,
    opts: &ParseOpts,
    keys: &DerivedKeys<'py>,
    names: &mut NameCache<'py>,
) -> PyResult<()> {
    // Trim in place to avoid a second allocation when strip_whitespace shaves
    // a few leading/trailing bytes.
    if opts.strip_whitespace && !data_local.is_empty() {
        let trimmed = data_local.trim();
        if trimmed.is_empty() {
            data_local.clear();
        } else {
            let start = trimmed.as_ptr() as usize - data_local.as_ptr() as usize;
            let end = start + trimmed.len();
            if end < data_local.len() {
                data_local.truncate(end);
            }
            if start > 0 {
                data_local.drain(..start);
            }
        }
    }
    let data_str: Option<String> = if data_local.is_empty() {
        None
    } else {
        Some(data_local)
    };

    let force_this = opts.force_cdata.contains(name);

    let mut item = item_local;
    if let Some(ref text) = data_str {
        if force_this && item.is_none() {
            item = Some(PyDict::new(py).into_any().unbind());
        }
        if let Some(dict_obj) = item.as_ref() {
            let d = dict_obj
                .bind(py)
                .cast::<PyDict>()
                .map_err(|_| PyTypeError::new_err("internal error: item is not a dict"))?;
            let pytext = PyString::new(py, text).into_any().unbind();
            dict_push(py, d, &keys.cdata, pytext, opts)?;
        }
    }

    let name_key = names.get_or_intern(py, name);
    if let Some(dict_obj) = item {
        push_data(py, cur_item, &name_key, dict_obj, opts)?;
    } else if let Some(text) = data_str {
        push_data(
            py,
            cur_item,
            &name_key,
            PyString::new(py, &text).into_any().unbind(),
            opts,
        )?;
    } else {
        push_data(py, cur_item, &name_key, py.None(), opts)?;
    }
    Ok(())
}

/// Insert (or list-append) a (key, value) pair into an already-bound dict,
/// honoring `force_list` semantics for first-sight keys.
fn dict_push<'py>(
    py: Python<'py>,
    d: &Bound<'py, PyDict>,
    key: &Bound<'py, PyString>,
    value: Py<PyAny>,
    opts: &ParseOpts,
) -> PyResult<()> {
    if let Some(existing) = d.get_item(key)? {
        if let Ok(lst) = existing.cast::<PyList>() {
            lst.append(value.into_bound(py))?;
        } else {
            let new_list = PyList::empty(py);
            new_list.append(existing)?;
            new_list.append(value.into_bound(py))?;
            d.set_item(key, new_list)?;
        }
        return Ok(());
    }

    // force_list check: fast-path Off avoids a UTF-8 conversion on the key.
    let force = match &opts.force_list {
        ForceList::Off => false,
        ForceList::All => true,
        ForceList::Keys(_) => opts.force_list.contains(key.to_str()?),
    };
    if force {
        let new_list = PyList::empty(py);
        new_list.append(value.into_bound(py))?;
        d.set_item(key, new_list)?;
    } else {
        d.set_item(key, value.into_bound(py))?;
    }
    Ok(())
}

fn push_data<'py>(
    py: Python<'py>,
    container: &mut Option<Py<PyAny>>,
    key: &Bound<'py, PyString>,
    value: Py<PyAny>,
    opts: &ParseOpts,
) -> PyResult<()> {
    if container.is_none() {
        *container = Some(PyDict::new(py).into_any().unbind());
    }
    let owner = container.as_ref().unwrap();
    let d = owner
        .bind(py)
        .cast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("internal error: container is not a dict"))?;
    dict_push(py, d, key, value, opts)
}
