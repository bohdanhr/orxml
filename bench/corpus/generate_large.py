"""Generate bench/corpus/large_synthetic.xml.

Produces a deterministic synthetic XML doc with deeply nested children,
mixed attribute and text content, and repeated sibling elements — the kind
of shape that exercises push_data and namespace handling at scale.

Usage::

    python bench/corpus/generate_large.py [N_RECORDS]

`N_RECORDS` defaults to 5000, which yields roughly 10 MB.
"""

from __future__ import annotations

import sys
from pathlib import Path


def build(n_records: int) -> str:
    parts: list[str] = []
    parts.append('<?xml version="1.0" encoding="utf-8"?>')
    parts.append('<records version="1.0" xmlns:x="http://example.com/x">')
    for i in range(n_records):
        parts.append(f'  <record id="rec-{i:06d}" active="{((i % 2 == 0) and "true") or "false"}">')
        parts.append(f"    <name>Record number {i}</name>")
        parts.append(f"    <index>{i}</index>")
        parts.append("    <meta>")
        parts.append(f'      <created by="system">2026-04-{(i % 28) + 1:02d}</created>')
        parts.append(
            f'      <modified by="user-{i % 50}">2026-04-{(i % 28) + 1:02d}T12:00:00Z</modified>'
        )
        parts.append("    </meta>")
        parts.append("    <tags>")
        for j in range(3 + (i % 5)):
            parts.append(f"      <tag>tag-{i}-{j}</tag>")
        parts.append("    </tags>")
        parts.append("    <body>")
        parts.append(
            "      Lorem ipsum dolor sit amet, consectetur adipiscing elit. "
            "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."
        )
        parts.append("    </body>")
        parts.append(f'    <x:extended note="synthetic">extended payload {i}</x:extended>')
        parts.append("  </record>")
    parts.append("</records>")
    return "\n".join(parts) + "\n"


def main() -> None:
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 5000
    xml = build(n)
    out = Path(__file__).parent / "large_synthetic.xml"
    out.write_text(xml, encoding="utf-8")
    print(f"Wrote {out} ({out.stat().st_size / 1024 / 1024:.2f} MiB, {n} records)")


if __name__ == "__main__":
    main()
