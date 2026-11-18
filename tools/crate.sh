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
# Redirect toolchain caches into TEST_TMPDIR so Bazel sandboxes can
# download and resolve dependencies (see tools/user_env.sh).
if [[ -n "${TEST_TMPDIR:-}" ]]; then
  # user_env.sh is a source file next to this wrapper. The wrapper may be
  # staged into the test's runfiles as a symlink to the source tree (which
  # the sandbox can read), so resolve it from the wrapper's symlinked
  # directory first; fall back to the runfiles tree and the REPO_ROOT copy
  # for layouts where the wrapper's directory does not contain it.
  USER_ENV="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/user_env.sh"
  if [[ ! -f "$USER_ENV" && -n "${TEST_SRCDIR:-}" ]]; then
    USER_ENV="$TEST_SRCDIR/$TEST_WORKSPACE/tools/user_env.sh"
  fi
  if [[ ! -f "$USER_ENV" ]]; then
    USER_ENV="$REPO_ROOT/tools/user_env.sh"
  fi
  if [[ ! -f "$USER_ENV" ]]; then
    echo "ERROR: user_env.sh not found (tried $USER_ENV)" >&2
    exit 1
  fi
  source "$USER_ENV"
fi
if [[ -n "${TEST_TMPDIR:-}" ]]; then
  WORKSPACE_DIR="$TEST_TMPDIR/cargo-workspace"
  rm -rf "$WORKSPACE_DIR"
  mkdir -p "$WORKSPACE_DIR"
  SOURCE_ROOT="$REPO_ROOT"
  if [[ ! -f "$SOURCE_ROOT/Cargo.toml" ]]; then
    LOCKFILE="$(find "$REPO_ROOT" -type f -name Cargo.lock -print -quit)"
    if [[ -n "$LOCKFILE" ]]; then
      SOURCE_ROOT="$(dirname "$LOCKFILE")"
    elif [[ -d "$REPO_ROOT/cargo_workspace" ]]; then
      SOURCE_ROOT="$REPO_ROOT/cargo_workspace"
    fi
  fi
  (
    cd "$SOURCE_ROOT"
    find . \( -type f -o -type l \) \
      ! -path "./bazel-*" \
      ! -path "./target/*" \
      ! -path "*/node_modules/*" \
      ! -path "*/dist/*" \
      ! -path "*/coverage/*" \
      ! -path "*/.git/*" \
    -print0 | while IFS= read -r -d '' f; do
      [[ -f "$f" ]] || continue
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
  test) cargo test -p "$CRATE" "$@" ;;
  fmt-check) cargo fmt -p "$CRATE" -- --check ;;
  clippy) cargo clippy -p "$CRATE" --all-targets -- -D warnings ;;
  run) cargo run -q -p "$CRATE" "$@" ;;
  build) cargo build -p "$CRATE" "$@" ;;
  *) echo "unknown action: $ACTION" >&2; exit 2 ;;
esac
