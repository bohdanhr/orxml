"""Unparse round-trip: orxml.unparse output re-parses to the same dict.

We do NOT assert byte equality with xmltodict.unparse (attribute ordering,
whitespace in pretty mode, etc. are intentionally non-parity).
"""

from __future__ import annotations

from typing import Any

import orxml
import pytest
import xmltodict


def _canonical(d: Any) -> Any:
    """Normalize dict for comparison (rebuild without orxml in the loop)."""
    return d


@pytest.fixture
def parsed_corpus(corpus_bytes: bytes) -> dict[str, Any]:
    return xmltodict.parse(corpus_bytes)


def test_unparse_roundtrips_through_xmltodict(parsed_corpus: dict[str, Any]) -> None:
    xml = orxml.unparse(parsed_corpus)
    assert xmltodict.parse(xml) == parsed_corpus


def test_unparse_roundtrips_through_orxml(parsed_corpus: dict[str, Any]) -> None:
    xml = orxml.unparse(parsed_corpus)
    assert orxml.parse(xml) == parsed_corpus


def test_unparse_simple_dict() -> None:
    d = {"a": {"@prop": "x", "b": "hello"}}
    xml = orxml.unparse(d)
    assert xmltodict.parse(xml) == d


def test_unparse_repeated_list() -> None:
    d = {"a": {"b": ["1", "2", "3"]}}
    xml = orxml.unparse(d)
    assert xmltodict.parse(xml) == d


def test_unparse_empty_list_omits_element() -> None:
    d = {"a": {"b": []}}
    xml = orxml.unparse(d)
    # After round-trip the empty list is gone because no element was emitted.
    assert xmltodict.parse(xml) == {"a": None}


def test_unparse_multiple_roots_rejected_when_full_document() -> None:
    d = {"a": "1", "b": "2"}
    with pytest.raises(ValueError):
        orxml.unparse(d)


def test_unparse_multiple_roots_allowed_without_full_document() -> None:
    d = {"a": "1", "b": "2"}
    xml = orxml.unparse(d, full_document=False)
    assert "<a>1</a>" in xml
    assert "<b>2</b>" in xml


def test_unparse_pretty_formats_correctly() -> None:
    d = {"a": {"b": "1", "c": "2"}}
    xml = orxml.unparse(d, pretty=True)
    # Should be re-parseable.
    assert xmltodict.parse(xml) == d
    # And contain indentation / newlines.
    assert "\n" in xml
    assert "\t" in xml or "  " in xml


def test_unparse_pretty_custom_indent_str() -> None:
    d = {"a": {"b": "1"}}
    xml = orxml.unparse(d, pretty=True, indent="  ")
    assert "  <b>" in xml
    assert xmltodict.parse(xml) == d


def test_unparse_pretty_custom_indent_int() -> None:
    d = {"a": {"b": "1"}}
    xml = orxml.unparse(d, pretty=True, indent=2)
    assert "  <b>" in xml


def test_unparse_short_empty_elements() -> None:
    d = {"a": {"b": None}}
    xml = orxml.unparse(d, short_empty_elements=True)
    assert "<b/>" in xml


def test_unparse_rejects_invalid_element_name() -> None:
    d = {"a<b": "x"}
    with pytest.raises(ValueError):
        orxml.unparse(d, full_document=False)


def test_unparse_rejects_invalid_attribute_name() -> None:
    d = {"a": {"@b c": "x"}}
    with pytest.raises(ValueError):
        orxml.unparse(d)


def test_unparse_comment_at_element_level() -> None:
    d = {"a": {"#comment": "hello", "b": "1"}}
    xml = orxml.unparse(d)
    assert "<!--hello-->" in xml


def test_unparse_comment_list() -> None:
    d = {"a": {"#comment": ["one", "two"], "b": "1"}}
    xml = orxml.unparse(d)
    assert "<!--one-->" in xml
    assert "<!--two-->" in xml


def test_unparse_comment_with_dashes_rejected() -> None:
    d = {"a": {"#comment": "has -- dashes"}}
    with pytest.raises(ValueError):
        orxml.unparse(d)


def test_unparse_bool_becomes_lowercase() -> None:
    d = {"a": True}
    xml = orxml.unparse(d)
    assert "<a>true</a>" in xml


def test_unparse_none_becomes_empty_element() -> None:
    d = {"a": None}
    xml = orxml.unparse(d)
    # None -> no text, no attrs
    assert "<a></a>" in xml or "<a/>" in xml


def test_unparse_escapes_text_specials() -> None:
    d = {"a": "<b>&c"}
    xml = orxml.unparse(d)
    assert "&lt;" in xml
    assert "&amp;" in xml
    assert xmltodict.parse(xml) == d


def test_unparse_escapes_attr_specials() -> None:
    d = {"a": {"@x": 'he said "hi"', "#text": "ok"}}
    xml = orxml.unparse(d)
    # The escaped quote should appear as &quot; inside the attribute value.
    assert "&quot;" in xml
    assert xmltodict.parse(xml) == d
