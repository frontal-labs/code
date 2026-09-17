#!/usr/bin/env bash
# Delegate to the canonical Bazel generators target.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec bazel run //tools/generators:generators -- "$@"
