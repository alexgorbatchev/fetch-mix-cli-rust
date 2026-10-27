default:
    @just --list

build:
    cargo build --release --bin fetch-mix
    mkdir -p bin
    cp target/release/fetch-mix bin/fetch-mix

test:
    cargo test --all-targets

lint:
    cargo clippy --all-targets -- -D warnings

fmt:
    cargo fmt --package fetch-mix-cli

fmt-check:
    cargo fmt --package fetch-mix-cli -- --check

coverage:
    cargo llvm-cov --all-targets --fail-under-lines 90

check: fmt-check lint test coverage

run *args:
    cargo run --bin fetch-mix -- {{args}}

run-ai *args:
    AGENT=1 cargo run --bin fetch-mix -- {{args}}
