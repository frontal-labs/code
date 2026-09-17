#!/usr/bin/env bash
# Delegate to the canonical Bazel benchmark target.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec bazel run //tools/benchmark:benchmark -- "$@"
