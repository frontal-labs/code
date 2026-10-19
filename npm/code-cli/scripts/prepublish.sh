#!/usr/bin/env bash
# scripts/prepublish.sh — guard against publishing a broken/dev package.
set -euo pipefail
PKG_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PKG_ROOT"

VERSION="$(node -e 'console.log(require("./package.json").version)')"
if [[ "$VERSION" == *-dev* || "$VERSION" == "0.0.0" ]]; then
  echo "ERROR: refusing to publish dev version ($VERSION). Bump to a release version first." >&2
  exit 1
fi

# Launcher + support code must be present.
for f in bin/index.js lib/platform.mjs lib/resolve-binary.mjs lib/download.mjs scripts/postinstall.sh; do
  if [[ ! -f "$f" ]]; then
    echo "ERROR: required file missing: $f" >&2
    exit 1
  fi
done

echo "==> Pre-publish checks passed (v$VERSION). Running tests."
exec ./scripts/test.sh
