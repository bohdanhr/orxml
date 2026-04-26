#!/usr/bin/env bash
# Rewrite the package version in Cargo.toml and pyproject.toml.
#
# Usage:
#   scripts/set-version.sh 0.1.2
#   scripts/set-version.sh v0.1.2            # leading 'v' is stripped
#   scripts/set-version.sh "$GITHUB_REF_NAME"
#
# The script is idempotent and only touches the [package] section of
# Cargo.toml and the [project] section of pyproject.toml.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi

raw="$1"
# Accept "v0.1.0" or "refs/tags/v0.1.0".
version="${raw#refs/tags/}"
version="${version#v}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.+][A-Za-z0-9.-]+)?$ ]]; then
  echo "error: version '$version' doesn't look like a PEP 440 / SemVer version" >&2
  exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"

# Cargo.toml — scoped to [package] section.
python3 - "$root/Cargo.toml" "$version" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
version = sys.argv[2]
text = path.read_text()

def replace_in_section(section: str, text: str) -> str:
    pattern = re.compile(
        r"(^\[" + re.escape(section) + r"\][^\[]*?^version\s*=\s*\")[^\"]+(\")",
        re.MULTILINE | re.DOTALL,
    )
    new, n = pattern.subn(r"\g<1>" + version + r"\g<2>", text, count=1)
    if n != 1:
        raise SystemExit(f"error: could not rewrite [{section}].version in {path}")
    return new

text = replace_in_section("package", text)
path.write_text(text)
print(f"updated {path} -> {version}")
PY

python3 - "$root/pyproject.toml" "$version" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
version = sys.argv[2]
text = path.read_text()

pattern = re.compile(
    r"(^\[project\][^\[]*?^version\s*=\s*\")[^\"]+(\")",
    re.MULTILINE | re.DOTALL,
)
new, n = pattern.subn(r"\g<1>" + version + r"\g<2>", text, count=1)
if n != 1:
    raise SystemExit(f"error: could not rewrite [project].version in {path}")
path.write_text(new)
print(f"updated {path} -> {version}")
PY
