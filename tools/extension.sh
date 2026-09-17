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
EXTENSION="$1"
ACTION="${2:-lint}"
shift 2
EXT_DIR="$REPO_ROOT/$EXTENSION"
if [[ ! -d "$EXT_DIR" ]]; then
  echo "ERROR: extension dir not found: $EXT_DIR" >&2
  exit 1
fi
cd "$EXT_DIR"
if [[ ! -f package.json ]]; then
  echo "ERROR: package.json missing in $EXTENSION" >&2
  exit 1
fi

if ! command -v bun >/dev/null 2>&1; then
  echo "ERROR: bun not found on PATH" >&2
  exit 1
fi

case "$ACTION" in
  lint)
    if [[ -d src ]]; then
      bunx @biomejs/biome check src/
    else
      bunx @biomejs/biome check .
    fi
    ;;
  format)
    if [[ -d src ]]; then
      bunx @biomejs/biome format --write src/
    else
      bunx @biomejs/biome format --write .
    fi
    ;;
  build)
    if [[ ! -f package.json ]] || ! bun pm has-script build >/dev/null 2>&1; then
      echo "ERROR: no build script in $EXTENSION/package.json" >&2
      exit 1
    fi
    bun run build
    ;;
  test)
    if ! bun pm has-script test >/dev/null 2>&1; then
      echo "ERROR: no test script in $EXTENSION/package.json" >&2
      exit 1
    fi
    bun run test
    ;;
  typecheck)
    if ! bun pm has-script typecheck >/dev/null 2>&1; then
      echo "ERROR: no typecheck script in $EXTENSION/package.json" >&2
      exit 1
    fi
    bun run typecheck
    ;;
  *) echo "unknown action: $ACTION" >&2; exit 2 ;;
esac
