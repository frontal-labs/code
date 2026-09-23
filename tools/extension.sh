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

# Bazel runfiles are symlinks back to the source tree, which is outside the
# sandbox. Copy the extension into a writable temporary directory so Bun and
# Vitest can resolve and generate files normally.
if [[ -n "${TEST_SRCDIR:-}" && -z "${BUILD_WORKSPACE_DIRECTORY:-}" ]]; then
  WORK_DIR="$(mktemp -d "${TEST_TMPDIR:-/tmp}/extension.XXXXXX")"
  cp -RL "$EXT_DIR"/. "$WORK_DIR"/
  for config_file in biome.json ultracite.json .gitignore; do
    if [[ -f "$REPO_ROOT/$config_file" ]]; then
      cp -L "$REPO_ROOT/$config_file" "$WORK_DIR/$config_file"
    fi
  done
  EXT_DIR="$WORK_DIR"
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

has_script() {
  local script="$1"
  bun -e 'const pkg = await Bun.file("package.json").json(); process.exit(pkg.scripts?.[process.argv[1]] ? 0 : 1)' "$script" >/dev/null 2>&1
}

ensure_dependencies() {
  if [[ -d node_modules ]]; then
    return
  fi
  if [[ -f bun.lock ]]; then
    bun install --frozen-lockfile --ignore-scripts
  fi
}

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
    if [[ ! -f package.json ]] || ! has_script build; then
      echo "ERROR: no build script in $EXTENSION/package.json" >&2
      exit 1
    fi
    ensure_dependencies
    bun run build
    ;;
  test)
    if ! has_script test; then
      echo "ERROR: no test script in $EXTENSION/package.json" >&2
      exit 1
    fi
    ensure_dependencies
    bun run test
    ;;
  typecheck)
    if ! has_script typecheck; then
      echo "ERROR: no typecheck script in $EXTENSION/package.json" >&2
      exit 1
    fi
    ensure_dependencies
    bun run typecheck
    ;;
  *) echo "unknown action: $ACTION" >&2; exit 2 ;;
esac
