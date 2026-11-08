#!/usr/bin/env bash
# End-to-end check for hosted worker lanes.
#
# Builds the worker image, starts code-server with the local-docker transport,
# submits one task, and polls it until it reaches a terminal state. The worker
# container runs `frontal-code hosted task run`, calls a real model, and reports
# back to the server.
#
# Requirements: a running Docker daemon, and a provider key in the environment
# (for example OPENAI_API_KEY plus OPENAI_BASE_URL for OpenRouter).
#
# Usage:
#   OPENAI_API_KEY=... OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
#     scripts/worker-e2e.sh [model] [provider]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FRONTAL_CODE_SERVER_DOCKER_IMAGE:-frontal-code-worker:local}"
PORT="${FRONTAL_CODE_E2E_PORT:-8790}"
MODEL="${1:-nvidia/nemotron-3-ultra-550b-a55b:free}"
PROVIDER="${2:-openai}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/frontal-worker-e2e.XXXXXX")"
SERVER_LOG="$WORK/server.log"
API_KEY="e2e-$(date +%s)"

if ! docker info >/dev/null 2>&1; then
  echo "error: Docker daemon is not reachable" >&2
  exit 1
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1 || [ "${REBUILD:-0}" = "1" ]; then
  echo "building $IMAGE ..."
  docker build -f "$ROOT/docker/worker.Dockerfile" -t "$IMAGE" "$ROOT"
fi

cargo build -q -p code-server --manifest-path "$ROOT/Cargo.toml"
SERVER_BIN="$ROOT/target/debug/code-server"

cleanup() {
  if [ -n "${SERVER_PID:-}" ]; then kill "$SERVER_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT

FRONTAL_CODE_SERVER_HOST=0.0.0.0 \
FRONTAL_CODE_SERVER_PORT="$PORT" \
FRONTAL_CODE_SERVER_API_KEY="$API_KEY" \
FRONTAL_CODE_SERVER_LANE_TRANSPORT=local-docker \
FRONTAL_CODE_SERVER_DOCKER_IMAGE="$IMAGE" \
FRONTAL_CODE_SERVER_CALLBACK_URL="http://host.docker.internal:$PORT" \
FRONTAL_CODE_SERVER_WORKSPACE_ROOT="$WORK/workspaces" \
FRONTAL_CODE_SERVER_STATE_FILE="$WORK/state.json" \
  "$SERVER_BIN" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 30); do
  curl -sf -H "authorization: Bearer $API_KEY" "http://127.0.0.1:$PORT/v1/version" >/dev/null && break
  sleep 1
done
curl -sf -H "authorization: Bearer $API_KEY" "http://127.0.0.1:$PORT/v1/version" >/dev/null || { cat "$SERVER_LOG"; exit 1; }

BODY=$(python3 -c "import json,sys; print(json.dumps({'prompt': 'Reply with the single word: pong', 'provider': sys.argv[1], 'model': sys.argv[2], 'permission_mode': 'read-only', 'source': 'api'}))" "$PROVIDER" "$MODEL")
RESP=$(curl -sf -X POST "http://127.0.0.1:$PORT/v1/tasks" -H "authorization: Bearer $API_KEY" -H 'content-type: application/json' -d "$BODY")
TASK_ID=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['task_id'])" "$RESP")
echo "submitted $TASK_ID"

STATUS=""
for _ in $(seq 1 120); do
  DETAIL=$(curl -sf "http://127.0.0.1:$PORT/v1/tasks/$TASK_ID" -H "authorization: Bearer $API_KEY")
  STATUS=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['status'])" "$DETAIL")
  case "$STATUS" in completed|failed|cancelled) break ;; esac
  sleep 3
done

echo "status: $STATUS"
python3 -c "import json,sys; d=json.loads(sys.argv[1]); print('result:', d.get('result')); print('error:', d.get('error'))" "$DETAIL"
[ "$STATUS" = "completed" ]
