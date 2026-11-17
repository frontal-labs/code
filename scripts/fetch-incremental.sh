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
REPO="$(python3 -c "import json; print(json.load(open('$MANIFEST'))['repo'])")"
# `repo` is the GitHub owner/name; the clone URL is derived from it. A bare
# "owner/name" is not a URL and `git clone` would fail on the clone path.
REPO_URL="https://github.com/${REPO}.git"

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
# The digest is taken over the source bytes in a fixed entry order, not over
# a tar archive: an ordinary `tar` embeds filesystem metadata (mtimes, uid,
# entry order) so two checkouts with identical bytes would hash differently.
# Sorting the paths and hashing each file's bytes in that order makes the
# digest reproducible across clones. `sha256_rust_src` in the manifest is
# computed with this same procedure.
RUST_SRC_PATHS="bindings/rust/Cargo.lock
bindings/rust/Cargo.toml
bindings/rust/src/lib.rs"

COMPUTED_HASH="$(cd "$SOURCE_DIR" && python3 -c "
import hashlib, os, sys
h = hashlib.sha256()
paths = '''${RUST_SRC_PATHS}'''.splitlines()
paths.sort()
for p in paths:
    with open(p, 'rb') as f:
        h.update(p.encode('utf-8'))
        h.update(f.read())
print(h.hexdigest())
")"

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

# The upstream Cargo.toml does not declare a license. The vendored copy
# carries `license = "Apache-2.0"` so that downstream `cargo audit` /
# license checks can attribute it. Re-apply that line after copying so a
# re-vendor does not silently drop it.
LICENSE="$(python3 -c "import json; print(json.load(open('$MANIFEST')).get('license',''))")"
if [[ -n "$LICENSE" ]]; then
  if ! grep -q '^license = ' "$DEST/rust/Cargo.toml"; then
    # Insert after the [package] header line (first `license =` slot).
    awk -v lic="$LICENSE" '
      { print }
      /^[[:space:]]*description[[:space:]]*=/ { print "license = \"" lic "\"" }
    ' "$DEST/rust/Cargo.toml" > "$DEST/rust/Cargo.toml.tmp" && mv "$DEST/rust/Cargo.toml.tmp" "$DEST/rust/Cargo.toml"
  fi
fi

# --- copy engine binary (if present in source) ---
BIN_SRC="$SOURCE_DIR/core/dist/incremental"
if [[ -f "$BIN_SRC" ]]; then
  # Detect the binary's platform triple. The OS component is normalized to
  # the directory names used under engine-binaries/: Darwin reports as
  # `darwin` from uname, so no translation is needed, but be explicit.
  OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
  ARCH="$(uname -m)"
  case "$ARCH" in
    x86_64)  ARCH="x64" ;;
    aarch64) ARCH="arm64" ;;
    arm64)   ARCH="arm64" ;;
  esac
  TRIPLE="$OS-$ARCH"

  # Resolve the expected hash from the manifest before touching the
  # destination, so a mismatch fails without leaving a bad binary in place.
  EXPECTED_BIN_HASH="$(python3 -c "
import json
m = json.load(open('$MANIFEST'))
h = m.get('sha256_bins', {}).get('$TRIPLE')
print('NOT_PINNED' if h is None else h)
")"
  ACTUAL_BIN_HASH="$(shasum -a 256 "$BIN_SRC" | awk '{print $1}')"

  if [[ "$ACTUAL_BIN_HASH" != "$EXPECTED_BIN_HASH" ]]; then
    if [[ "$EXPECTED_BIN_HASH" == "NOT_PINNED" ]]; then
      echo "error: engine binary for $TRIPLE is not pinned in the manifest." >&2
      echo "  computed: $ACTUAL_BIN_HASH" >&2
      echo "  Add it to third_party/manifests/incremental.json sha256_bins before vendoring." >&2
    else
      echo "error: engine binary hash mismatch for $TRIPLE" >&2
      echo "  expected: $EXPECTED_BIN_HASH" >&2
      echo "  computed: $ACTUAL_BIN_HASH" >&2
    fi
    exit 1
  fi
  echo "==> Engine binary hash verified ($TRIPLE)"

  echo "==> Copying engine binary ($TRIPLE) ..."
  mkdir -p "$DEST/engine-binaries/$TRIPLE"
  cp "$BIN_SRC" "$DEST/engine-binaries/$TRIPLE/incremental"
  chmod +x "$DEST/engine-binaries/$TRIPLE/incremental"
else
  echo "==> No engine binary found in source (core/dist/incremental). Skipping." >&2
  echo "==> Build it with: cd incremental && make build" >&2
fi

echo "==> Done. Incremental vendored at $COMMIT"
