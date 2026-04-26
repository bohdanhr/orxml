"""orxml: Rust-backed xmltodict-compatible XML <-> dict conversion.

Typical usage::

    import orxml

    data = orxml.parse(xml_bytes_or_str)
    xml_out = orxml.unparse(data)

The surface mirrors ``xmltodict``'s ``parse`` and ``unparse`` on a medium
API surface; see README for supported options.
"""

from __future__ import annotations

from orxml._core import ParseError, parse, unparse

__all__ = ["ParseError", "parse", "unparse"]
__version__ = "0.1.0"
