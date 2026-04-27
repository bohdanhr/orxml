# orxml

Rust-backed XML ↔ Python dict conversion, API-compatible with the common surface of `[xmltodict](https://github.com/martinblech/xmltodict)`. ~4× faster on `parse` and ~10× faster on `unparse` across a representative corpus.

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

Run on an Apple Silicon laptop (Python 3.13, `xmltodict` 0.14, `quick-xmltodict` 0.2):

| Operation | Fixture             | orxml (mean) | xmltodict (mean) | Speedup vs xmltodict |
| --------- | ------------------- | ------------ | ---------------- | -------------------- |
| parse     | tiny.xml            | 3.14 µs      | 14.02 µs         | 4.47×                |
| parse     | small.xml           | 27.37 µs     | 117.28 µs        | 4.29×                |
| parse     | rss.xml             | 29.55 µs     | 124.79 µs        | 4.22×                |
| parse     | soap.xml            | 25.46 µs     | 97.19 µs         | 3.82×                |
| parse     | large_synthetic.xml | 39.06 ms     | 158.14 ms        | 4.05×                |
| unparse   | tiny.xml            | 1.07 µs      | 11.20 µs         | 10.47×               |
| unparse   | small.xml           | 10.36 µs     | 107.73 µs        | 10.40×               |
| unparse   | rss.xml             | 10.64 µs     | 110.94 µs        | 10.43×               |
| unparse   | soap.xml            | 8.86 µs      | 87.01 µs         | 9.82×                |
| unparse   | large_synthetic.xml | 12.94 ms     | 129.03 ms        | 9.97×                |

`orxml.parse` also outperforms `quick-xmltodict` (the other Rust-backed parser) by 1.07–1.50× across these fixtures — e.g. 39.06 ms vs 58.43 ms on `large_synthetic.xml`.

`large_synthetic.xml` is ~3 MB / 75k elements / 25k attributes / 40k leaves / 254k text chars. The other fixtures are tens of elements each and are dominated by fixed per-call overhead.

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

## License

MIT