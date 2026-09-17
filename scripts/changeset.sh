#!/usr/bin/env bash
# Run Changesets from the repository root.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec npx changeset "$@"