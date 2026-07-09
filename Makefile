.DEFAULT_GOAL := help

.PHONY: help setup install build typecheck fmt test clippy check smoke clean

help:
	@echo ""
	@echo "Fiber DevKit - available make targets"
	@echo ""
	@echo "  make setup      Install support deps and local fiber CLI"
	@echo "  make install    Install or refresh local fiber CLI only"
	@echo "  make check      Run CI-equivalent local verification"
	@echo "  make smoke      Run unfunded local network smoke test"
	@echo "  make build      Build debug binary"
	@echo "  make typecheck  Run TypeScript type checking"
	@echo "  make fmt        Check Rust formatting"
	@echo "  make test       Run Rust tests"
	@echo "  make clippy     Run Clippy with warnings as errors"
	@echo "  make clean      Remove Rust build artifacts with cargo clean"
	@echo ""
	@echo "Run make setup on a fresh clone."
	@echo "Funded payment scenarios require testnet CKB and are intentionally not automated here."
	@echo "See README.md and SCENARIO_FORMAT.md."
	@echo ""

setup:
	pnpm install
	cargo install --path . --locked --force

install:
	cargo install --path . --locked --force

build:
	cargo build --locked

typecheck:
	pnpm typecheck

fmt:
	cargo fmt --check

test:
	cargo test --locked

clippy:
	cargo clippy --locked -- -D warnings

check: build typecheck fmt test clippy

smoke: build
	@trap 'echo "==> Cleaning up containers..."; target/debug/fiber down' EXIT; \
	echo "==> Resetting local DevKit network..." && \
	target/debug/fiber reset && \
	echo "==> Starting local Fiber network..." && \
	(target/debug/fiber up || (echo "==> First startup attempt failed; retrying once..." && target/debug/fiber down && target/debug/fiber up)) && \
	echo "==> Running unfunded network smoke scenario..." && \
	target/debug/fiber run scenarios/network-smoke.yaml --report > /tmp/fiber-devkit-smoke.jsonl && \
	echo "==> Structured scenario output written to /tmp/fiber-devkit-smoke.jsonl" && \
	target/debug/fiber report --format md && \
	echo "==> Inspecting local network..." && \
	target/debug/fiber inspect && \
	target/debug/fiber inspect node-1 --channels && \
	echo "==> Smoke complete. Reports written to .fiber/output/"

clean:
	cargo clean
