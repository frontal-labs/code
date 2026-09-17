#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="${BUILD_WORKSPACE_DIRECTORY:-}"
if [[ -z "$REPO_ROOT" ]]; then
  if [[ -n "${TEST_SRCDIR:-}" && -n "${TEST_WORKSPACE:-}" && -d "$TEST_SRCDIR/$TEST_WORKSPACE" ]]; then
    REPO_ROOT="$TEST_SRCDIR/$TEST_WORKSPACE"
  else
    REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  fi
fi
cd "$REPO_ROOT"
ACTION="${1:-test}"
cd cli
if [[ ! -f package.json ]]; then
  echo "ERROR: cli/package.json not found (checkout incomplete)" >&2
  exit 1
fi

case "$ACTION" in
  test)
    echo "==> Running cli test suite"
    if ! command -v node >/dev/null 2>&1; then
      echo "ERROR: node not found on PATH" >&2
      exit 1
    fi
    node --test tests/*.test.mjs
    ;;
  lint)
    if command -v bunx >/dev/null 2>&1; then
      bunx @biomejs/biome check .
    elif command -v npx >/dev/null 2>&1; then
      npx -y @biomejs/biome check .
    else
      echo "ERROR: neither bunx nor npx found on PATH" >&2
      exit 1
    fi
    ;;
  build)
    echo "==> Building cli"
    if ! command -v cargo >/dev/null 2>&1; then
      echo "ERROR: cargo not found on PATH. Install Rust to build the native binary." >&2
      exit 1
    fi
    ./scripts/build.sh
    ;;
  *) echo "unknown action: $ACTION" >&2; exit 2 ;;
esac
