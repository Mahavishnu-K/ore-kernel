.PHONY: setup build check test lint format clean

# Default task
all: format check lint build test

setup:
	cargo fetch

build:
	cargo build --workspace

release:
	cargo build --release --workspace

check:
	cargo check --workspace

test:
	cargo test --workspace

lint:
	cargo clippy --workspace -- -D warnings

format:
	cargo fmt --all

clean:
	cargo clean
