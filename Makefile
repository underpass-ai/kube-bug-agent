.PHONY: ci fmt lint test coverage architecture build smoke-local

ci: architecture fmt lint coverage

architecture:
	bash scripts/check-architecture.sh

fmt:
	cargo fmt --all --check

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test --all-targets

coverage:
	mkdir -p artifacts
	cargo llvm-cov --all-targets --fail-under-lines 80 --json --output-path artifacts/coverage.json

build:
	cargo build --release

smoke-local:
	cargo test --test live_local -- --ignored --nocapture
