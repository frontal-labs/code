#!/usr/bin/env bash
set -euo pipefail

rustup show
cargo build --workspace --locked --release
