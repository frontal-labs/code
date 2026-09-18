#!/usr/bin/env bash
# scripts/test.sh — run the JS test suite and a binary smoke test.
set -euo pipefail
PKG_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PKG_ROOT"

echo "==> Running node:test suite"
node --test tests/*.test.mjs

# Smoke test: only if a binary can be resolved (vendored or local cargo build).
if node -e 'import("./lib/resolve-binary.mjs").then(m=>process.exit(m.resolveBinary()?0:1))'; then
  echo "==> Smoke test: frontal-code --version"
  node ./bin/index.js --version
  node ./bin/index.js --version | grep -qi "Frontal Code" && echo "OK: version output contains 'Frontal Code'"
else
  echo "WARN: no native binary resolved; skipping smoke test."
fi
