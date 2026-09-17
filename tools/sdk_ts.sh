#!/usr/bin/env bash
# Run the TypeScript SDK's Node toolchain (Bun) actions inside its directory.
# Usage: sdk_ts.sh <relative-dir> <test|build|typecheck>
set -euo pipefail
REPO_ROOT="${BUILD_WORKSPACE_DIRECTORY:-}"
if [[ -z "$REPO_ROOT" ]]; then
  if [[ -n "${TEST_SRCDIR:-}" && -n "${TEST_WORKSPACE:-}" && -d "$TEST_SRCDIR/$TEST_WORKSPACE" ]]; then
    REPO_ROOT="$TEST_SRCDIR/$TEST_WORKSPACE"
  else
    REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  fi
fi
DIR="$1"
ACTION="${2:-test}"
PKG_DIR="$REPO_ROOT/$DIR"
if [[ ! -d "$PKG_DIR" ]]; then
  echo "ERROR: package dir not found: $PKG_DIR" >&2
  exit 1
fi
cd "$PKG_DIR"
if [[ ! -f package.json ]]; then
  echo "ERROR: package.json missing in $DIR" >&2
  exit 1
fi

run() {
  if command -v bun >/dev/null 2>&1; then
    bun "$@"
  elif command -v npm >/dev/null 2>&1; then
    npm "$@"
  else
    echo "ERROR: neither bun nor npm found" >&2
    exit 1
  fi
}

case "$ACTION" in
  test) run run test ;;
  build) run run build ;;
  typecheck) run run typecheck ;;
  *) echo "unknown action: $ACTION" >&2; exit 2 ;;
esac
