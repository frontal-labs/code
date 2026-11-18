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
cd "$REPO_ROOT"
ACTION="${1:-test}"
cd npm/code-cli
if [[ ! -f package.json ]]; then
  echo "ERROR: npm/code-cli/package.json not found (checkout incomplete)" >&2
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
      bunx @biomejs/biome@2.5.14 ci .
    elif command -v npx >/dev/null 2>&1; then
      npx -y @biomejs/biome@2.5.14 ci .
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
