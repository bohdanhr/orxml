"""Shared fixtures for orxml benchmarks."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest
import xmltodict

CORPUS_DIR = Path(__file__).resolve().parent / "corpus"


def _available_fixtures() -> list[Path]:
    return sorted(p for p in CORPUS_DIR.glob("*.xml"))


@pytest.fixture(params=_available_fixtures(), ids=lambda p: p.name)
def corpus_path(request: pytest.FixtureRequest) -> Path:
    return request.param


@pytest.fixture
def corpus_bytes(corpus_path: Path) -> bytes:
    return corpus_path.read_bytes()


@pytest.fixture
def corpus_str(corpus_path: Path) -> str:
    return corpus_path.read_text(encoding="utf-8")


@pytest.fixture
def parsed_dict(corpus_bytes: bytes) -> dict[str, Any]:
    return xmltodict.parse(corpus_bytes)
