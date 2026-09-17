#!/usr/bin/env bash
# Aggregate Bazel CI entrypoint: build, test, then lint.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
bazel build //...
bazel test //...
bazel test //:lint
