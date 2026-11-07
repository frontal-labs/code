#!/usr/bin/env bash
set -euo pipefail

echo "Deploying to staging"
# Use OIDC/cloud credentials fetched via buildkite-agent secret get
