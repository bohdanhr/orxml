"""Type stubs for the Rust extension module ``orxml._core``."""

from __future__ import annotations

from typing import Any

class ParseError(ValueError):
    """Raised when XML parsing fails."""

def parse(
    xml_input: str | bytes,
    *,
    attr_prefix: str = "@",
    cdata_key: str = "#text",
    cdata_separator: str = "",
    strip_whitespace: bool = True,
    namespace_separator: str = ":",
    process_namespaces: bool = False,
    namespaces: dict[str, str] | None = None,
    process_comments: bool = False,
    comment_key: str = "#comment",
    force_list: bool | tuple[str, ...] | list[str] | None = None,
    force_cdata: bool | tuple[str, ...] | list[str] | None = None,
    disable_entities: bool = True,
    xml_attribs: bool = True,
    encoding: str | None = None,
) -> Any:
    """Parse XML into a Python dict (xmltodict-compatible)."""

def unparse(
    input_dict: dict[str, Any],
    *,
    attr_prefix: str = "@",
    cdata_key: str = "#text",
    comment_key: str = "#comment",
    namespaces: dict[str, str] | None = None,
    namespace_separator: str = ":",
    pretty: bool = False,
    newl: str = "\n",
    indent: str | int = "\t",
    full_document: bool = True,
    short_empty_elements: bool = False,
    encoding: str = "utf-8",
) -> str:
    """Emit XML from a Python dict (xmltodict-compatible)."""
