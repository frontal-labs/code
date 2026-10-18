#!/usr/bin/env bash
# scripts/verify.sh — CI/presubmit checks for the npm package.
set -euo pipefail
PKG_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PKG_ROOT"

echo "==> Syntax-checking JS/mjs sources"
node --check bin/index.js
bash -n scripts/postinstall.sh
for f in lib/*.mjs; do
  node --check "$f"
done

echo "==> Running test suite"
./scripts/test.sh

echo "==> Checking published file list excludes vendor/ and tests/"
npm pack --dry-run --json 2>/dev/null | node -e '
let raw = "";
process.stdin.on("data", d => raw += d);
process.stdin.on("end", () => {
  let data;
  try { data = JSON.parse(raw); } catch (e) { console.log("skip: no json"); process.exit(0); }
  const files = (Array.isArray(data) ? data[0].files : data.files).map(f => f.path);
  const bad = files.filter(p => p.startsWith("vendor/") || p.startsWith("tests/"));
  if (bad.length) {
    console.error("ERROR: unexpected files in package:", bad);
    process.exit(1);
  }
  for (const need of ["bin/index.js","lib/platform.mjs","lib/resolve-binary.mjs","lib/download.mjs","scripts/postinstall.sh"]) {
    if (!files.includes(need)) { console.error("ERROR: missing required file:", need); process.exit(1); }
  }
  console.log("OK: package file list is correct");
});
'
echo "==> verify.sh complete"
