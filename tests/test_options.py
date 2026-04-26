"""Option handling: NotImplementedError for out-of-scope options, TypeError for unknown."""

from __future__ import annotations

import orxml
import pytest

# ---------- parse() out-of-scope options ----------


@pytest.mark.parametrize(
    "kwargs",
    [
        {"postprocessor": lambda *args: args},
        {"item_callback": lambda *args: True},
        {"item_depth": 1},
        {"dict_constructor": dict},
        {"preprocessor": lambda *args: args},
        {"expand_iter": "foo"},
        {"expat": None},
    ],
)
def test_parse_unsupported_options_raise_not_implemented(
    kwargs: dict[str, object],
) -> None:
    with pytest.raises(NotImplementedError):
        orxml.parse("<a>hi</a>", **kwargs)


def test_parse_force_list_callable_not_implemented() -> None:
    with pytest.raises(NotImplementedError):
        orxml.parse("<a><b>1</b></a>", force_list=lambda *a: True)


def test_parse_force_cdata_callable_not_implemented() -> None:
    with pytest.raises(NotImplementedError):
        orxml.parse("<a>hi</a>", force_cdata=lambda *a: True)


def test_parse_unknown_option_is_type_error() -> None:
    with pytest.raises(TypeError):
        orxml.parse("<a>hi</a>", totally_bogus=True)


# ---------- unparse() out-of-scope options ----------


@pytest.mark.parametrize(
    "kwargs",
    [
        {"postprocessor": lambda *args: args},
        {"preprocessor": lambda *args: args},
        {"expand_iter": "foo"},
        {"dict_constructor": dict},
    ],
)
def test_unparse_unsupported_options_raise_not_implemented(
    kwargs: dict[str, object],
) -> None:
    with pytest.raises(NotImplementedError):
        orxml.unparse({"a": "x"}, **kwargs)


def test_unparse_unknown_option_is_type_error() -> None:
    with pytest.raises(TypeError):
        orxml.unparse({"a": "x"}, not_a_real_option=True)


# ---------- parse() supported options — smoke tests ----------


def test_parse_attr_prefix_custom() -> None:
    assert orxml.parse("<a x='1'/>", attr_prefix="_") == {"a": {"_x": "1"}}


def test_parse_cdata_key_custom() -> None:
    assert orxml.parse("<a x='1'>hi</a>", cdata_key="TEXT") == {"a": {"@x": "1", "TEXT": "hi"}}


def test_parse_comment_key_custom() -> None:
    doc = orxml.parse("<a><!--hi--><b>1</b></a>", process_comments=True, comment_key="CMT")
    assert "CMT" in doc["a"]


def test_parse_force_list_tuple() -> None:
    assert orxml.parse("<a><b>1</b></a>", force_list=("b",)) == {"a": {"b": ["1"]}}


def test_parse_force_list_list() -> None:
    assert orxml.parse("<a><b>1</b></a>", force_list=["b"]) == {"a": {"b": ["1"]}}


def test_parse_force_list_bool_true() -> None:
    doc = orxml.parse("<a><b>1</b><c>2</c></a>", force_list=True)
    # With force_list=True the root is also wrapped in a list (matches xmltodict).
    assert doc == {"a": [{"b": ["1"], "c": ["2"]}]}


def test_parse_force_cdata_tuple() -> None:
    doc = orxml.parse("<a><b>hi</b><c>bye</c></a>", force_cdata=("b",))
    assert doc == {"a": {"b": {"#text": "hi"}, "c": "bye"}}


def test_parse_strip_whitespace_false() -> None:
    doc = orxml.parse("<a>  hi  </a>", strip_whitespace=False)
    assert doc == {"a": "  hi  "}


def test_parse_xml_attribs_false() -> None:
    doc = orxml.parse("<a x='1'>hi</a>", xml_attribs=False)
    assert doc == {"a": "hi"}


# ---------- unparse() supported options — smoke tests ----------


def test_unparse_full_document_false_has_no_xml_decl() -> None:
    xml = orxml.unparse({"a": "x"}, full_document=False)
    assert not xml.startswith("<?xml")


def test_unparse_encoding_reflected_in_declaration() -> None:
    xml = orxml.unparse({"a": "x"}, encoding="utf-16")
    assert 'encoding="utf-16"' in xml


def test_unparse_attr_prefix_custom() -> None:
    xml = orxml.unparse({"a": {"_x": "1"}}, attr_prefix="_")
    assert 'x="1"' in xml
