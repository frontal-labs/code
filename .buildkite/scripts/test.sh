#!/usr/bin/env bash
set -euo pipefail

rustup show
cargo test --workspace --locked --no-fail-fast
