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
if [[ -n "${TEST_TMPDIR:-}" ]]; then
  WORKSPACE_DIR="$TEST_TMPDIR/cargo-workspace"
  rm -rf "$WORKSPACE_DIR"
  mkdir -p "$WORKSPACE_DIR"
  (
    cd "$REPO_ROOT"
    find . -type f \
      ! -path "./bazel-*" \
      ! -path "./target/*" \
      ! -path "*/node_modules/*" \
      ! -path "*/dist/*" \
      ! -path "*/coverage/*" \
      ! -path "*/.git/*" \
      -print0 | while IFS= read -r -d '' f; do
      rel="${f#./}"
      mkdir -p "$WORKSPACE_DIR/$(dirname "$rel")"
      cp -L "$f" "$WORKSPACE_DIR/$rel"
    done
  )
  chmod -R u+rwX "$WORKSPACE_DIR"
  REPO_ROOT="$WORKSPACE_DIR"
fi
cd "$REPO_ROOT"
export CARGO_TARGET_DIR="${TEST_TMPDIR:-$REPO_ROOT/target}/bazel"
if ! command -v cargo >/dev/null 2>&1; then
  echo "ERROR: cargo not found on PATH" >&2
  exit 1
fi
CRATE="$1"
ACTION="${2:-test}"
shift 2

case "$ACTION" in
  test) cargo test --offline -p "$CRATE" "$@" ;;
  fmt-check) cargo fmt -p "$CRATE" -- --check ;;
  clippy) cargo clippy --offline -p "$CRATE" --all-targets -- -D warnings ;;
  run) cargo run --offline -q -p "$CRATE" -- "$@" ;;
  build) cargo build --offline -p "$CRATE" "$@" ;;
  *) echo "unknown action: $ACTION" >&2; exit 2 ;;
esac
