#!/usr/bin/env bash
# Frontal Code — monorepo linter.
#
# Runs Biome (TS/JS/JSON/YAML) and Rust clippy. Any failed check fails the run;
# missing tooling is a hard error. Accepts a target path relative to the repo
# root (default: repo root), e.g. `tools/lint.sh sdks/typescript`.
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

echo "==> linting $TARGET"

if command -v bun >/dev/null 2>&1; then
  bunx @biomejs/biome check "$TARGET"
elif command -v npx >/dev/null 2>&1; then
  npx -y @biomejs/biome check "$TARGET"
else
  echo "ERROR: neither bun nor npx found on PATH; cannot run Biome check" >&2
  exit 1
fi

case "$TARGET" in
  cli|sdks/typescript|extensions/*)
    exit 0
    ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
  echo "ERROR: cargo not found on PATH; cannot run clippy" >&2
  exit 1
fi
cargo clippy --workspace --all-targets -- -D warnings
