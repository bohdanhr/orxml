"""Parse benchmarks — orxml vs xmltodict on each corpus fixture.

Run with::

    uv run pytest bench/bench_parse.py --benchmark-only

Add ``--benchmark-compare`` (or ``--benchmark-columns=mean,median,stddev``)
for more detail. Results are grouped per fixture so you can see both
implementations side-by-side.
"""

from __future__ import annotations

from pathlib import Path

import orxml
import xmltodict


def _group_name(corpus_path: Path) -> str:
    return f"parse:{corpus_path.name}"


def test_bench_orxml_parse(benchmark, corpus_bytes: bytes, corpus_path: Path) -> None:
    benchmark.group = _group_name(corpus_path)
    benchmark(orxml.parse, corpus_bytes)


def test_bench_xmltodict_parse(benchmark, corpus_bytes: bytes, corpus_path: Path) -> None:
    benchmark.group = _group_name(corpus_path)
    benchmark(xmltodict.parse, corpus_bytes)
