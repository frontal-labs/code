#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FORMULA="$SCRIPT_DIR/Formula/code.rb"

if [[ ! -f "$FORMULA" ]]; then
  echo "Formula not found: $FORMULA" >&2
  exit 1
fi

brew install --HEAD "$FORMULA"
