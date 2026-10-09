.PHONY: ci fmt lint test coverage architecture build smoke-local smoke-backpressure helm helm-package smoke-helm

ci: architecture fmt lint coverage helm

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

smoke-backpressure:
	cargo test --test live_envoy -- --ignored --nocapture

helm:
	python3 scripts/test-helm.py

helm-package: helm
	mkdir -p artifacts/helm
	helm package charts/kube-bug-agent --destination artifacts/helm

smoke-helm:
	HELM_ENVOY_SMOKE=1 python3 scripts/test-helm.py HelmChartTests.test_envoy_bootstrap_is_validated_by_real_envoy
