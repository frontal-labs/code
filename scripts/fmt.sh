#!/usr/bin/env bash
# Format the monorepo through the Bazel formatter target.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
exec bazel run //:fmt "$@"
