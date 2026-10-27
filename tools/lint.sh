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

if [[ -n "${TEST_SRCDIR:-}" && -z "${BUILD_WORKSPACE_DIRECTORY:-}" ]]; then
  # Under Bazel the runfiles are symlinks into the source tree, which Biome cannot
  # walk. Lint a writable mirror: the root configs plus a copy of the target at the
  # same relative path, so path-based overrides in biome.json still apply.
  MIRROR="$(mktemp -d "${TEST_TMPDIR:-/tmp}/lint-mirror.XXXXXX")"
  export TMPDIR="${TEST_TMPDIR:-/tmp}"
  for config_file in biome.json ultracite.json .gitignore; do
    if [[ -f "$config_file" ]]; then
      cp -L "$config_file" "$MIRROR/$config_file"
    fi
  done
  mkdir -p "$MIRROR/$TARGET"
  cp -RL "$TARGET"/. "$MIRROR/$TARGET"/
  cd "$MIRROR"
fi

if command -v bun >/dev/null 2>&1; then
  bunx @biomejs/biome@2.5.14 ci "$TARGET"
elif command -v npx >/dev/null 2>&1; then
  npx -y @biomejs/biome@2.5.14 ci "$TARGET"
else
  echo "ERROR: neither bun nor npx found on PATH; cannot run Biome check" >&2
  exit 1
fi
cd "$REPO_ROOT"

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
