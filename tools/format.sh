#!/usr/bin/env bash
# Frontal Code — monorepo formatter.
#
# Runs Biome format --write (TS/JS/JSON/YAML) and cargo fmt. Failed commands
# propagate their exit status; missing tooling is a hard error. Accepts a target
# path relative to the repo root (default: repo root).
set -euo pipefail
REPO_ROOT="${BUILD_WORKSPACE_DIRECTORY:-}"
if [[ -z "$REPO_ROOT" ]]; then
  if [[ -n "${TEST_SRCDIR:-}" && -n "${TEST_WORKSPACE:-}" && -d "$TEST_SRCDIR/$TEST_WORKSPACE" ]]; then
    REPO_ROOT="$TEST_SRCDIR/$TEST_WORKSPACE"
  else
    REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  fi
fi
TARGET="${1:-.}"
cd "$REPO_ROOT"

echo "==> formatting $TARGET"

if command -v bun >/dev/null 2>&1; then
  bunx @biomejs/biome format --write "$TARGET"
elif command -v npx >/dev/null 2>&1; then
  npx -y @biomejs/biome format --write "$TARGET"
else
  echo "ERROR: neither bun nor npx found on PATH; cannot run Biome format" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "ERROR: cargo not found on PATH; cannot run cargo fmt" >&2
  exit 1
fi
cargo fmt --all
