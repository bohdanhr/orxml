"""Shared fixtures for orxml tests."""

from __future__ import annotations

from pathlib import Path

import pytest

CORPUS_DIR = Path(__file__).resolve().parent.parent / "bench" / "corpus"


def _available_fixtures() -> list[Path]:
    if not CORPUS_DIR.is_dir():
        return []
    return sorted(p for p in CORPUS_DIR.glob("*.xml"))


@pytest.fixture(params=_available_fixtures(), ids=lambda p: p.name)
def corpus_path(request: pytest.FixtureRequest) -> Path:
    return request.param


@pytest.fixture
def corpus_bytes(corpus_path: Path) -> bytes:
    return corpus_path.read_bytes()


@pytest.fixture
def corpus_text(corpus_path: Path) -> str:
    return corpus_path.read_text(encoding="utf-8")
