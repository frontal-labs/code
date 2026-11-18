# Shared environment for toolchain wrappers (cargo, bun, npm) running inside
# Bazel test sandboxes.
#
# Bazel sandboxes block writes to paths outside the sandbox, but cargo, bun and
# npm need writable state: cargo's registry/git checkout under ~/.cargo, bun's
# install cache under ~/.bun, and npm's cacache under ~/.npm. Without this
# every target that downloads a dependency fails with
# "Operation not permitted (os error 1)" or
# "bun is unable to write files to tempdir: PermissionDenied".
#
# bun's `bunx` additionally writes its tempdir under $HOME (not $TMPDIR),
# so HOME itself must also be redirected into the sandbox.
#
# This script redirects all three caches (and HOME for bun) into $TEST_TMPDIR
# for the duration of the test. It is a no-op outside Bazel tests (no
# TEST_TMPDIR), and any value the user or CI already set (via .bazelrc.project,
# --action_env, or the environment) always wins.
#
# Usage:
#   source tools/user_env.sh
set -uo pipefail

if [[ -z "${TEST_TMPDIR:-}" ]]; then
  return 0 2>/dev/null || true
fi

# --- HOME (bun writes its tempdir under $HOME, not $TMPDIR) ------------------
# bunx resolves its tempdir relative to $HOME. The real $HOME lives outside
# the sandbox and writes there are denied, which surfaces as
# "bun is unable to write files to tempdir: PermissionDenied". Redirect HOME
# into the sandbox for the duration of the test. This also isolates any
# home-relative install state (~/.bun, ~/.npm, etc.) per test.
#
# We only set HOME if it was not already redirected into the sandbox by a
# higher-level wrapper, so a user who intentionally set HOME is respected.
if [[ -z "${HOME:-}" || "$HOME" != "$TEST_TMPDIR/"* ]]; then
  # Keep the real home for seeding the cargo cache below and for resolving
  # RUSTUP_HOME so the rustup toolchain stays reachable after HOME moves.
  REAL_HOME="${HOME:-}"
  if [[ -n "$REAL_HOME" && -z "${RUSTUP_HOME:-}" && -d "$REAL_HOME/.rustup" ]]; then
    export RUSTUP_HOME="$REAL_HOME/.rustup"
  fi
  export HOME="$TEST_TMPDIR/home"
  mkdir -p "$HOME"
fi

# --- TMPDIR (bun writes its tempdir here) -----------------------------------
export TMPDIR="${TEST_TMPDIR}"

# --- cargo -------------------------------------------------------------------
# A fresh, per-test CARGO_HOME. Writes (registry index, git checkouts) stay
# inside the sandbox; reads of the global toolchain (~/.cargo/bin on PATH) are
# unaffected because the toolchain itself is not under CARGO_HOME in a rustup
# layout.
if [[ -z "${CARGO_HOME:-}" ]]; then
  export CARGO_HOME="$TEST_TMPDIR/cargo-home"
  mkdir -p "$CARGO_HOME"
  # git-fetch-with-cli lets cargo shell out to the system git instead of using
  # libgit2, which is more robust under the sandbox's filesystem restrictions.
  printf '[net]\ngit-fetch-with-cli = true\n' > "$CARGO_HOME/config.toml"

  # Seed the per-test registry cache from the global one when present. Reading
  # the global cache is allowed; copying .crate files into the sandbox cache
  # means `cargo test` resolves from cache instead of re-downloading the whole
  # world on every run.
  GLOBAL_CARGO_HOME="${REAL_HOME:-${HOME:-}}/.cargo"
  if [[ -d "$GLOBAL_CARGO_HOME/registry/cache" ]]; then
    mkdir -p "$CARGO_HOME/registry"
    for shard in "$GLOBAL_CARGO_HOME/registry/cache"/*/; do
      [[ -d "$shard" ]] || continue
      shard_name="$(basename "$shard")"
      mkdir -p "$CARGO_HOME/registry/cache/$shard_name"
      cp -n "$shard"*.crate "$CARGO_HOME/registry/cache/$shard_name"/ 2>/dev/null || true
    done
    # Copy the index if present (read-only usage is fine).
    if [[ -d "$GLOBAL_CARGO_HOME/registry/index" ]]; then
      cp -Rn "$GLOBAL_CARGO_HOME/registry/index" "$CARGO_HOME/registry/index" 2>/dev/null || true
    fi
  fi
fi

# Keep build artifacts inside the sandbox too (crate.sh already does this;
# belt and braces for any other wrapper that invokes cargo).
if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="$TEST_TMPDIR/cargo-target"
fi

# --- bun ---------------------------------------------------------------------
# bun's install cache (bunx binaries, bun install packages, tempdir writes).
if [[ -z "${BUN_CONFIG_CACHE_DIR:-}" ]]; then
  export BUN_CONFIG_CACHE_DIR="$TEST_TMPDIR/bun-cache"
  mkdir -p "$BUN_CONFIG_CACHE_DIR"
fi

# --- npm ---------------------------------------------------------------------
if [[ -z "${npm_config_cache:-}" ]]; then
  export npm_config_cache="$TEST_TMPDIR/npm-cache"
  mkdir -p "$npm_config_cache"
fi
