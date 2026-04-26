# orxml

Rust-backed XML ↔ Python dict conversion, API-compatible with the common surface of `[xmltodict](https://github.com/martinblech/xmltodict)`. 3–8× faster on a representative corpus.

Uses the [BadgerFish-lite convention](https://www.xml.com/pub/a/2006/05/31/converting-between-xml-and-json.html):
`@` prefix for attributes, `#text` for text content, lists for repeated sibling elements. Because the shape is JSON-compatible, any parsed dict feeds straight into `json.dumps`.

## Install (from source)

```bash
git clone https://github.com/bohdanhr/orxml
cd orxml
uv sync
uv run maturin develop --release
```

Requires a stable Rust toolchain and Python 3.11+.

## Usage

```python
import orxml

doc = orxml.parse("<a prop='x'>hello</a>")
# {'a': {'@prop': 'x', '#text': 'hello'}}

xml = orxml.unparse(doc)
```

### Supported options

`parse`: `attr_prefix`, `cdata_key`, `force_cdata` (bool / tuple / list), `cdata_separator`, `strip_whitespace`, `namespace_separator`, `process_namespaces`, `namespaces`, `process_comments`, `comment_key`, `force_list` (bool / tuple / list), `disable_entities`, `xml_attribs`.

`unparse`: `attr_prefix`, `cdata_key`, `comment_key`, `namespaces`, `namespace_separator`, `pretty`, `newl`, `indent` (str or int), `full_document`, `short_empty_elements`, `encoding`.

Out-of-scope options (`postprocessor`, `item_callback` / `item_depth` streaming, callable `force_list` / `force_cdata`, `dict_constructor`, `preprocessor`, `expand_iter`, file-like / generator inputs) raise `NotImplementedError` if passed. Unknown options raise `TypeError`.

### Parity

Behavior is matched on the observable surface: for every supported option combination, `orxml.parse(xml) == xmltodict.parse(xml)` and `xmltodict.parse(orxml.unparse(d)) == d`. Intentional divergences:

- Exception classes: `orxml.ParseError` (a `ValueError`), not `xml.parsers.expat.ExpatError`.
- Error messages and byte-level unparse output (e.g. attribute ordering) are not guaranteed identical.
- Behavior on malformed XML is not defined.

## Benchmarks

Run on an Apple Silicon laptop comparing orxml against `xmltodict` 0.14:


| Operation | Fixture             | orxml (mean) | xmltodict (mean) | Speedup |
| --------- | ------------------- | ------------ | ---------------- | ------- |
| parse     | tiny.xml            | 3.55 µs      | 14.43 µs         | 4.07×   |
| parse     | small.xml           | 33.56 µs     | 119.78 µs        | 3.57×   |
| parse     | rss.xml             | 36.94 µs     | 131.46 µs        | 3.56×   |
| parse     | soap.xml            | 30.11 µs     | 134.53 µs        | 4.47×   |
| parse     | large_synthetic.xml | 51.20 ms     | 162.10 ms        | 3.17×   |
| unparse   | tiny.xml            | 2.56 µs      | 11.53 µs         | 4.51×   |
| unparse   | small.xml           | 25.28 µs     | 108.40 µs        | 4.29×   |
| unparse   | rss.xml             | 27.67 µs     | 112.74 µs        | 4.07×   |
| unparse   | soap.xml            | 18.52 µs     | 88.06 µs         | 4.75×   |
| unparse   | large_synthetic.xml | 33.17 ms     | 257.12 ms        | 7.75×   |


Reproduce locally with `make bench`.

## Development

```bash
make sync        # uv sync
make build       # maturin develop --release
make test        # pytest
make bench       # pytest-benchmark
make lint        # ruff check
make fmt         # ruff format
make typecheck   # ty check
make all         # sync + build + lint + typecheck + test
```

### Corpus

`bench/corpus/` holds fixtures used by both tests and benchmarks: `tiny.xml`, `small.xml`, `rss.xml`, `soap.xml`, and `large_synthetic.xml`. The last one is generated — regenerate it (or resize it) with:

```bash
uv run python bench/corpus/generate_large.py 5000   # ~3 MB
```

## Releasing

Release artifacts are built and published to PyPI automatically by the `Release` workflow in `.github/workflows/release.yml`. The workflow builds wheels for Linux (glibc + musl) x86_64 / aarch64, macOS x86_64 / arm64, Windows x64, and a source distribution, then publishes them to PyPI using [trusted publishing](https://docs.pypi.org/trusted-publishers/) (no API token stored in the repo).

### One-time PyPI setup

1. Create the `orxml` project on PyPI (happens automatically on first upload, or can be pre-reserved via a pending publisher).
2. On PyPI → Account settings → Publishing → **Add pending publisher** with:
   - PyPI project name: `orxml`
   - Owner: `bohdanhr`
   - Repository: `orxml`
   - Workflow: `release.yml`
   - Environment: `pypi`
3. (Optional) Create a **GitHub environment** named `pypi` under Settings → Environments. Add protection rules (e.g. required reviewers) if desired.

### Cutting a release

```bash
# 1. Bump version in Cargo.toml and pyproject.toml (keep them in sync).
$EDITOR Cargo.toml pyproject.toml

# 2. Commit, tag, and push.
git commit -am "Release v0.1.1"
git push

# 3. Create a GitHub Release pointing at a new tag.
#    The `Release` workflow runs on release.published and uploads to PyPI.
gh release create v0.1.1 --generate-notes
```

To dry-run wheel builds without publishing, trigger the workflow manually (`Actions → Release → Run workflow`); the publish step is skipped for `workflow_dispatch` runs.

## License

MIT