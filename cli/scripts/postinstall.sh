#!/usr/bin/env bash
# postinstall.sh — download the native `frontal-code` binary for this platform.
#
# Resilient by design: failures warn and exit 0 so that `npm install` in
# unrelated/CI/offline contexts never breaks. The launcher itself is the
# authority that reports a missing binary to the user at run time.
#
# Skip conditions (exit 0 quietly):
#   - FCODE_SKIP_DOWNLOAD=1
#   - npm_config_offline (npm/pnpm/yarn offline)
#   - npm_config_ignore_scripts (already handled by npm, but be safe)
#   - dev version (0.0.0-dev / *-dev) -> expect a local cargo build
#   - a valid vendored or local binary already exists

set -euo pipefail

PKG_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PKG_ROOT"

# Check skip conditions.
if [ "${FCODE_SKIP_DOWNLOAD:-}" = "1" ]; then
  echo "[frontal-code:postinstall] skipping download (FCODE_SKIP_DOWNLOAD=1)."
  exit 0
fi

if [ "${npm_config_offline:-}" = "true" ]; then
  echo "[frontal-code:postinstall] skipping download (offline install)."
  exit 0
fi

if [ "${npm_config_ignore_scripts:-}" = "true" ]; then
  echo "[frontal-code:postinstall] skipping download (ignore-scripts)."
  exit 0
fi

# Read version from package.json.
VERSION="$(node -e "console.log(require('./package.json').version ?? '0.0.0-dev')")"

# Dev version: do not hit the network; expect local cargo build.
if [[ "$VERSION" == *-dev* || "$VERSION" == "0.0.0" ]]; then
  if node -e "import('./lib/resolve-binary.mjs').then(m => process.exit(m.resolveBinary() ? 0 : 1))"; then
    echo "[frontal-code:postinstall] dev version detected; using existing local build."
  else
    echo "[frontal-code:postinstall] dev version detected ($VERSION); skipping download."
    echo "Build locally with: cargo build --release -p cli"
  fi
  exit 0
fi

# Already downloaded (or locally built)?
if node -e "import('./lib/resolve-binary.mjs').then(m => process.exit(m.resolveBinary() ? 0 : 1))"; then
  echo "[frontal-code:postinstall] native binary already present; nothing to do."
  exit 0
fi

echo "[frontal-code:postinstall] installing native binary for $VERSION..."
node -e "
import('./lib/download.mjs').then(async (m) => {
  const { resolve } = require('node:path');
  const { readFileSync } = require('node:fs');
  const ROOT = process.env.POSTINSTALL_ROOT;
  const version = JSON.parse(readFileSync(resolve(ROOT, 'package.json'), 'utf8')).version ?? '0.0.0-dev';
  try {
    const bin = await m.downloadRelease({ version, destDir: ROOT });
    console.log('[frontal-code:postinstall] installed native binary to', bin);
  } catch (e) {
    console.warn('[frontal-code:postinstall] warning: failed to download native binary:', e.message);
    console.warn(
      'The frontal-code command will not work until a binary is available.\n' +
        'Build locally with \`cargo build --release -p frontal-code-cli\` or run\n' +
        '  (cd cli && ./scripts/download.sh) after tagging a release.',
    );
    process.exit(0);
  }
});
" POSTINSTALL_ROOT="$PKG_ROOT"
