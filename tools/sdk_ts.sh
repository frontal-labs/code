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

if [[ ! -d node_modules ]]; then
  if command -v npm >/dev/null 2>&1 && [[ -f package-lock.json ]]; then
    npm ci --ignore-scripts --no-audit --no-fund
  elif command -v bun >/dev/null 2>&1; then
    bun install --ignore-scripts
  else
    echo "ERROR: cannot install TypeScript SDK dependencies" >&2
    exit 1
  fi
fi

case "$ACTION" in
  test) run run test ;;
  build) run run build ;;
  typecheck) run run typecheck ;;
  *) echo "unknown action: $ACTION" >&2; exit 2 ;;
esac
