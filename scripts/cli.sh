#!/usr/bin/env bash
# Delegate CLI package verification to Bazel.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec bazel test //cli:test "$@"
