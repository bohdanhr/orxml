"""Unparse benchmarks — orxml vs xmltodict on each corpus fixture.

Inputs are pre-parsed dicts (via xmltodict) so we measure emission alone.
Run with::

    uv run pytest bench/bench_unparse.py --benchmark-only
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import orxml
import xmltodict


def _group_name(corpus_path: Path) -> str:
    return f"unparse:{corpus_path.name}"


def test_bench_orxml_unparse(benchmark, parsed_dict: dict[str, Any], corpus_path: Path) -> None:
    benchmark.group = _group_name(corpus_path)
    benchmark(orxml.unparse, parsed_dict)


def test_bench_xmltodict_unparse(benchmark, parsed_dict: dict[str, Any], corpus_path: Path) -> None:
    benchmark.group = _group_name(corpus_path)
    benchmark(xmltodict.unparse, parsed_dict)
