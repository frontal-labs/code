#!/usr/bin/env bash
set -euo pipefail

rustup show
cargo fmt --all --check
cargo clippy --workspace --locked -- -D warnings
cargo deny check
