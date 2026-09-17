#!/usr/bin/env bash
# Run the Bazel lint suite.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
exec bazel test //:lint "$@"
