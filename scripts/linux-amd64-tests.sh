#!/usr/bin/env bash
# Runs the whole workspace test suite, including ignored tests, inside a linux/amd64 container.
# On Apple silicon the container runs under Rosetta. Use it to check Linux behavior before a
# change lands on a Linux CI runner.
#
# The services come from scripts/dev-services.sh. The ClickHouse container's network namespace is
# shared with the test container, so the audit tests reach it over loopback, which the server
# requires for cleartext HTTP.
#
#   scripts/linux-amd64-tests.sh            # full run
#   TEST_FILTER=worker_ scripts/linux-amd64-tests.sh   # only tests whose names match
#
# The run is bounded by TEST_TIMEOUT seconds (default 5400). A hung test binary otherwise blocks
# the run indefinitely, which happened under Rosetta; see docs/TROUBLESHOOTING.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TLS_DIR="${TLS_DIR:-$HOME/.frontal-code-dev/tls}"
CH_NAME="${CH_NAME:-frontal-dev-clickhouse}"
CH_PASSWORD="${CH_PASSWORD:-dev-local-only}"
PG_PASSWORD="${PG_PASSWORD:-dev-local-only}"
IMAGE="${RUST_IMAGE:-rust:1.95-bookworm}"
# Where Postgres is reachable from the container. On a native Linux host, where host.docker.internal
# does not resolve, use the bridge gateway instead, usually 172.17.0.1.
PG_HOST="${PG_HOST:-host.docker.internal}"

# Copy the source into a snapshot, and run the containers against that copy. A test that writes or
# deletes under its working directory then cannot touch the working tree. Build output and VCS
# metadata stay out of the copy.
snapshot_source() {
  local dest="$HOME/.frontal-code-dev/src-snapshot"
  mkdir -p "$dest"
  rsync -a --delete \
    --exclude '.git' --exclude 'target' --exclude 'target-linux' --exclude 'node_modules' \
    --exclude 'bazel-*' --exclude '.frontal-code' --exclude 'extensions' \
    "$ROOT/" "$dest/"
  echo "$dest"
}


"$ROOT/scripts/dev-services.sh" up >/dev/null
SRC="$(snapshot_source)"

docker run --rm --platform linux/amd64 --network "container:$CH_NAME" \
  -v "$SRC":/src -w /src \
  -v frontal-cargo-registry:/usr/local/cargo/registry \
  -v frontal-target-amd64:/src/target-linux \
  -v "$TLS_DIR":/certs:ro \
  -e FRONTAL_TEST_POSTGRES_URL="postgres://frontal:$PG_PASSWORD@$PG_HOST:55433/frontal?sslmode=verify-full" \
  -e FRONTAL_TEST_POSTGRES_CA_FILE=/certs/ca.crt \
  -e FRONTAL_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
  -e FRONTAL_TEST_CLICKHOUSE_USER=frontal \
  -e FRONTAL_TEST_CLICKHOUSE_PASSWORD="$CH_PASSWORD" \
  "$IMAGE" bash -c "apt-get update -qq && apt-get install -y -qq pkg-config libssl-dev libpq-dev git python3 >/dev/null 2>&1; \
    CARGO_TARGET_DIR=/src/target-linux timeout -k 30 ${TEST_TIMEOUT:-5400} cargo test --workspace --no-fail-fast -- --include-ignored ${TEST_FILTER:-}"
