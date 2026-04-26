.PHONY: help sync build test bench lint fmt typecheck clean all

UV ?= uv

help:
	@echo "Targets:"
	@echo "  sync       Install Python dev dependencies (via uv)"
	@echo "  build      Build the Rust extension in release mode"
	@echo "  test       Run the test suite"
	@echo "  bench      Run the benchmark suite"
	@echo "  lint       Run ruff on Python code"
	@echo "  fmt        Run ruff format on Python code"
	@echo "  typecheck  Run ty on the Python surface"
	@echo "  clean      Remove build artifacts"
	@echo "  all        sync + build + lint + typecheck + test"

sync:
	$(UV) sync

build:
	$(UV) run maturin develop --release

test:
	$(UV) run pytest

bench:
	$(UV) run pytest bench/bench_parse.py bench/bench_unparse.py \
		--benchmark-only \
		--benchmark-columns=min,mean,stddev,rounds \
		--benchmark-sort=mean \
		--override-ini="testpaths="

lint:
	$(UV) run ruff check python tests bench

fmt:
	$(UV) run ruff format python tests bench

typecheck:
	$(UV) run ty check python

clean:
	rm -rf target/ dist/ build/ .pytest_cache/ .benchmarks/ .ruff_cache/ .ty_cache/

all: sync build lint typecheck test
