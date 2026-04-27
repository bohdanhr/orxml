"""Parse parity: orxml.parse should equal xmltodict.parse for supported opts."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import orxml
import pytest
import xmltodict

# A matrix of option combinations to exercise on the corpus. Keep these
# side-effect-free (no Python callbacks, no streaming).
OPTION_SETS: list[dict[str, Any]] = [
    {},
    {"attr_prefix": "_"},
    {"cdata_key": "__text__"},
    {"force_cdata": True},
    {"force_list": True},
    {"strip_whitespace": False},
    {"cdata_separator": " | "},
    {"xml_attribs": False},
    {"process_comments": True},
]


@pytest.mark.parametrize("opts", OPTION_SETS, ids=lambda o: repr(sorted(o.items())))
def test_parse_matches_xmltodict_corpus(corpus_bytes: bytes, opts: dict[str, Any]) -> None:
    assert orxml.parse(corpus_bytes, **opts) == xmltodict.parse(corpus_bytes, **opts)


def test_parse_accepts_str_and_bytes() -> None:
    xml = "<a prop='x'><b>hello</b></a>"
    from_str = orxml.parse(xml)
    from_bytes = orxml.parse(xml.encode("utf-8"))
    assert from_str == from_bytes == {"a": {"@prop": "x", "b": "hello"}}


def test_parse_repeated_elements_become_list() -> None:
    doc = orxml.parse("<a><b>1</b><b>2</b><b>3</b></a>")
    assert doc == {"a": {"b": ["1", "2", "3"]}}


def test_parse_empty_element_is_none() -> None:
    doc = orxml.parse("<a><b/></a>")
    assert doc == {"a": {"b": None}}


def test_parse_cdata_joined_with_separator() -> None:
    xml = "<a>foo<b/>bar</a>"
    orx = orxml.parse(xml, cdata_separator=" | ")
    ref = xmltodict.parse(xml, cdata_separator=" | ")
    assert orx == ref


def test_parse_force_list_keys() -> None:
    xml = "<a><b>1</b></a>"
    doc = orxml.parse(xml, force_list=("b",))
    assert doc == {"a": {"b": ["1"]}}


def test_parse_force_cdata_wraps_text_in_text_key() -> None:
    doc = orxml.parse("<a>hello</a>", force_cdata=True)
    assert doc == {"a": {"#text": "hello"}}


@pytest.mark.parametrize(
    "xml,opts",
    [
        (
            '<root xmlns="http://default" xmlns:a="http://a"><b/><a:c/></root>',
            {"process_namespaces": True},
        ),
        (
            '<root xmlns:a="http://a"><a:b a:x="1">hi</a:b></root>',
            {"process_namespaces": True, "namespaces": {"http://a": "a"}},
        ),
        (
            '<root xmlns="http://default"><child/></root>',
            {"process_namespaces": True, "namespaces": {"http://default": ""}},
        ),
    ],
)
def test_parse_namespaces_matches_xmltodict(xml: str, opts: dict[str, Any]) -> None:
    assert orxml.parse(xml, **opts) == xmltodict.parse(xml, **opts)


def test_parse_process_comments() -> None:
    xml = "<a><!--hello--><b>1</b></a>"
    assert orxml.parse(xml, process_comments=True) == xmltodict.parse(xml, process_comments=True)


def test_parse_disable_entities_raises_on_entity_decl() -> None:
    xml = '<?xml version="1.0"?><!DOCTYPE a [<!ENTITY e "val">]><a>&e;</a>'
    with pytest.raises((orxml.ParseError, ValueError)):
        orxml.parse(xml)


def test_parse_result_type_is_plain_dict() -> None:
    result = orxml.parse("<a><b>1</b></a>")
    assert isinstance(result, dict)


@pytest.mark.parametrize(
    "inp",
    [
        b"",
        "",
        "   ",
        '<?xml version="1.0"?>',
        "<!-- just a comment -->",
    ],
)
def test_parse_raises_when_no_root_element(inp: bytes | str) -> None:
    # Matches xmltodict's ExpatError("no element found") behavior and keeps
    # the `dict[str, Any]` return type on the Python side honest.
    with pytest.raises((orxml.ParseError, ValueError)):
        orxml.parse(inp)


def test_parse_preserves_element_order() -> None:
    xml = "<r><c/><a/><b/></r>"
    doc = orxml.parse(xml)
    assert list(doc["r"].keys()) == ["c", "a", "b"]


def test_parse_corpus_files_exist() -> None:
    corpus_dir = Path(__file__).resolve().parent.parent / "bench" / "corpus"
    assert corpus_dir.is_dir()
    files = list(corpus_dir.glob("*.xml"))
    assert files, "expected bench/corpus/ to contain XML fixtures"
