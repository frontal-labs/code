#!/usr/bin/env bash
# scripts/build.sh — build the native `frontal-code` binary and place it where the
# launcher (bin/index.js) expects it in dev: vendor/<target>/bin/frontal-code.
set -euo pipefail
PKG_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PKG_ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "ERROR: cargo not found on PATH. Install Rust to build the native binary." >&2
  exit 1
fi

# Resolve the cargo workspace root from the package directory so the binary
# lands in the same target/ that cargo writes to.
REPO_ROOT="$(dirname "$(cargo locate-project --workspace --message-format plain)")"

echo "==> Building cli with cargo (release)"
( cd "$REPO_ROOT" && cargo build --release -p cli )

# Determine Rust target triple dir name (mirror lib/platform.mjs TARGETS).
OS="$(uname -s)"; ARCH="$(uname -m)"
case "$OS/$ARCH" in
  Darwin/arm64|Darwin/aarch64)  TARGET_DIR="aarch64-apple-darwin" ;;
  Darwin/x86_64)                TARGET_DIR="x86_64-apple-darwin" ;;
  Linux/x86_64|Linux/amd64)     TARGET_DIR="x86_64-unknown-linux-gnu" ;;
  Linux/aarch64|Linux/arm64)    TARGET_DIR="aarch64-unknown-linux-gnu" ;;
  *) echo "ERROR: unsupported build platform: $OS/$ARCH" >&2; exit 1 ;;
esac

# Respect CARGO_TARGET_DIR (set by tools/user_env.sh under Bazel sandboxes)
# so the binary is found where cargo actually wrote it. Resolve relative
# values against REPO_ROOT (cargo's CWD during the build) so the cp finds
# the same directory cargo wrote to.
if [[ -n "${CARGO_TARGET_DIR:-}" && "${CARGO_TARGET_DIR:-}" != /* ]]; then
  TARGET_DIR_CARGO="$REPO_ROOT/${CARGO_TARGET_DIR}"
else
  TARGET_DIR_CARGO="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
fi
SRC="$TARGET_DIR_CARGO/release/code"
DEST="$PKG_ROOT/vendor/$TARGET_DIR/bin/frontal-code"
mkdir -p "$(dirname "$DEST")"
cp "$SRC" "$DEST"
chmod +x "$DEST"
echo "==> Native binary placed at $DEST"
