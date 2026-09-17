#!/usr/bin/env bash
# Delegate to the canonical Bazel doctor target.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec bazel run //tools/doctor:doctor -- "$@"
