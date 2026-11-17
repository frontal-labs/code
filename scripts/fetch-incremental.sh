#!/usr/bin/env bash
# Fetch and vendor the Incremental engine at a pinned commit.
#
# This script is the reproducibility anchor for the vendored Incremental
# artifacts. It reads the pin from third_party/manifests/incremental.json,
# clones at that SHA, and copies the Rust binding source and the prebuilt
# engine binaries into third_party/incremental/.
#
# Usage:
#   scripts/fetch-incremental.sh [--source DIR]
#
# --source DIR  Use a local checkout instead of cloning (for offline use).
#               DIR must be a checkout of frontal-labs/incremental at the
#               pinned commit.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MANIFEST="$REPO_ROOT/third_party/manifests/incremental.json"
DEST="$REPO_ROOT/third_party/incremental"

# --- read pin from manifest ---
if [[ ! -f "$MANIFEST" ]]; then
  echo "error: manifest not found: $MANIFEST" >&2
  exit 1
fi

COMMIT="$(python3 -c "import json; print(json.load(open('$MANIFEST'))['commit'])")"
SHA256_RUST="$(python3 -c "import json; print(json.load(open('$MANIFEST'))['sha256_rust_src'])")"
REPO_URL="$(python3 -c "import json; print(json.load(open('$MANIFEST'))['repo'])")"

echo "==> Pinned incremental commit: $COMMIT"

# --- resolve source ---
SOURCE_DIR=""
if [[ "${1:-}" == "--source" ]]; then
  SOURCE_DIR="${2:?usage: --source DIR}"
  if [[ ! -d "$SOURCE_DIR" ]]; then
    echo "error: source directory not found: $SOURCE_DIR" >&2
    exit 1
  fi
else
  # Clone at the pinned SHA
  SOURCE_DIR="$(mktemp -d)"
  trap 'rm -rf "$SOURCE_DIR"' EXIT
  echo "==> Cloning $REPO_URL at $COMMIT ..."
  git clone --quiet "$REPO_URL" "$SOURCE_DIR"
  (cd "$SOURCE_DIR" && git checkout --quiet "$COMMIT")
fi

ACTUAL_COMMIT="$(cd "$SOURCE_DIR" && git rev-parse HEAD)"
if [[ "$ACTUAL_COMMIT" != "$COMMIT" ]]; then
  echo "error: source commit $ACTUAL_COMMIT does not match pinned $COMMIT" >&2
  exit 1
fi

# --- verify Rust binding source hash ---
COMPUTED_HASH="$(cd "$SOURCE_DIR" && tar -cf - \
  bindings/rust/Cargo.toml \
  bindings/rust/Cargo.lock \
  bindings/rust/src/ \
  2>/dev/null | shasum -a 256 | awk '{print $1}')"

if [[ "$COMPUTED_HASH" != "$SHA256_RUST" ]]; then
  echo "error: Rust binding source hash mismatch" >&2
  echo "  expected: $SHA256_RUST" >&2
  echo "  computed: $COMPUTED_HASH" >&2
  exit 1
fi
echo "==> Rust binding source hash verified"

# --- copy Rust binding ---
echo "==> Copying Rust binding to $DEST/rust ..."
rm -rf "$DEST/rust"
mkdir -p "$DEST/rust/src"
cp "$SOURCE_DIR/bindings/rust/Cargo.toml" "$DEST/rust/Cargo.toml"
cp "$SOURCE_DIR/bindings/rust/Cargo.lock" "$DEST/rust/Cargo.lock"
cp "$SOURCE_DIR/bindings/rust/src/lib.rs" "$DEST/rust/src/lib.rs"

# --- copy engine binary (if present in source) ---
BIN_SRC="$SOURCE_DIR/core/dist/incremental"
if [[ -f "$BIN_SRC" ]]; then
  # Detect the binary's platform triple
  OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
  ARCH="$(uname -m)"
  case "$ARCH" in
    x86_64)  ARCH="x64" ;;
    aarch64) ARCH="arm64" ;;
    arm64)   ARCH="arm64" ;;
  esac
  TRIPLE="$OS-$ARCH"
  echo "==> Copying engine binary ($TRIPLE) ..."
  mkdir -p "$DEST/engine-binaries/$TRIPLE"
  cp "$BIN_SRC" "$DEST/engine-binaries/$TRIPLE/incremental"
  chmod +x "$DEST/engine-binaries/$TRIPLE/incremental"

  # Verify SHA-256 against the manifest
  EXPECTED_BIN_HASH="$(python3 -c "
import json, sys
m = json.load(open('$MANIFEST'))
bins = m.get('sha256_bins', {})
h = bins.get('$TRIPLE')
if h is None:
    print('NOT_PINNED')
else:
    print(h)
")"
  ACTUAL_BIN_HASH="$(shasum -a 256 "$DEST/engine-binaries/$TRIPLE/incremental" | awk '{print $1}')"
  if [[ "$EXPECTED_BIN_HASH" == "NOT_PINNED" ]]; then
    echo "==> WARNING: engine binary for $TRIPLE is not pinned in the manifest." >&2
    echo "==> Computed SHA-256: $ACTUAL_BIN_HASH" >&2
    echo "==> Add it to third_party/manifests/incremental.json sha256_bins." >&2
  elif [[ "$ACTUAL_BIN_HASH" != "$EXPECTED_BIN_HASH" ]]; then
    echo "error: engine binary hash mismatch for $TRIPLE" >&2
    echo "  expected: $EXPECTED_BIN_HASH" >&2
    echo "  computed: $ACTUAL_BIN_HASH" >&2
    exit 1
  else
    echo "==> Engine binary hash verified"
  fi
else
  echo "==> No engine binary found in source (core/dist/incremental). Skipping." >&2
  echo "==> Build it with: cd incremental && make build" >&2
fi

echo "==> Done. Incremental vendored at $COMMIT"
