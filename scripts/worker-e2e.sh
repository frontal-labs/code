#!/usr/bin/env bash
# End-to-end check for hosted worker lanes on the sandbox transport.
#
# Starts code-server with the sandbox transport and runs TASKS tasks at once
# against a local bare repository. Each task writes a file with an allowed
# command and tries a denied one. When the tasks finish, the script checks
# that each branch reached the remote.
#
# Requirements: a running sandbox server (FRONTAL_SANDBOX_BASE_URL +
# FRONTAL_SANDBOX_API_KEY), a Postgres database for worker tokens, and the
# provider. The default provider is a local Ollama server, which needs no key:
#
#   FRONTAL_SANDBOX_BASE_URL=http://localhost:8080 \
#   FRONTAL_SANDBOX_API_KEY=sbx_… \
#   FRONTAL_CODE_SERVER_URL=http://127.0.0.1:8790 \
#   OLLAMA_BASE_URL=http://127.0.0.1:11434 \
#   FRONTAL_CODE_SERVER_DATABASE_URL='postgres://user:pass@127.0.0.1:55433/db?sslmode=verify-full' \
#   FRONTAL_CODE_SERVER_DATABASE_CA_FILE=/path/to/ca.crt \
#     scripts/worker-e2e.sh [model] [provider]
#
# For a cloud OpenAI-compatible provider pass `openai` and set OPENAI_API_KEY (and
# OPENAI_BASE_URL for a non-OpenAI endpoint).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${FRONTAL_CODE_E2E_PORT:-8790}"
TASKS="${TASKS:-2}"
MODEL="${1:-qwen2.5:1.5b}"
PROVIDER="${2:-ollama}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/frontal-worker-e2e.XXXXXX")"
SERVER_LOG="$WORK/server.log"
API_KEY="e2e-$(date +%s)-$RANDOM"
STAMP="$(date +%s)"
PROMPT="${PROMPT:-Create a file named published.txt in the current directory (use the relative path published.txt, never an absolute path) containing exactly: from the worker. Then run: python3 -c 'print(1)'. Then run: curl -m 5 -sS https://example.com -o /dev/null; echo CURL_EXIT=\$?. Then run: rm -rf /tmp/anything; echo RM_EXIT=\$?. Reply with the exit code lines and the word DONE.}"

: "${FRONTAL_CODE_SERVER_DATABASE_URL:?set FRONTAL_CODE_SERVER_DATABASE_URL to a Postgres database}"
: "${FRONTAL_SANDBOX_BASE_URL:?set FRONTAL_SANDBOX_BASE_URL to a running sandbox server}"
: "${FRONTAL_SANDBOX_API_KEY:?set FRONTAL_SANDBOX_API_KEY to the sandbox server Bearer key}"

# The sandbox server must be reachable and accept the key.
if ! curl -sf -H "authorization: Bearer $FRONTAL_SANDBOX_API_KEY" "$FRONTAL_SANDBOX_BASE_URL/v1/health" >/dev/null 2>&1; then
  echo "error: sandbox server at $FRONTAL_SANDBOX_BASE_URL is not reachable or refused the key" >&2
  exit 1
fi

# A bare repository stands in for GitHub. The server publishes to it.
git init -q --bare "$WORK/remote.git"
git init -q -b main "$WORK/seed"
echo "hello" > "$WORK/seed/README.md"
git -C "$WORK/seed" add .
git -C "$WORK/seed" -c user.name=seed -c user.email=seed@local commit -q -m init
git -C "$WORK/seed" push -q "$WORK/remote.git" main

cargo build -q -p code-server --manifest-path "$ROOT/Cargo.toml"
SERVER_BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/code-server"

cleanup() {
  if [ -n "${SERVER_PID:-}" ]; then kill "$SERVER_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT

FRONTAL_CODE_SERVER_HOST=0.0.0.0 \
FRONTAL_CODE_SERVER_PORT="$PORT" \
FRONTAL_CODE_SERVER_API_KEY="$API_KEY" \
FRONTAL_CODE_SERVER_LANE_TRANSPORT=sandbox \
FRONTAL_CODE_SERVER_ALLOW_LOCAL_REPOS=1 \
FRONTAL_CODE_SERVER_URL="http://127.0.0.1:$PORT" \
FRONTAL_CODE_SERVER_WORKSPACE_ROOT="$WORK/workspaces" \
FRONTAL_CODE_SERVER_STATE_FILE="$WORK/state.json" \
FRONTAL_SANDBOX_BASE_URL="$FRONTAL_SANDBOX_BASE_URL" \
FRONTAL_SANDBOX_API_KEY="$FRONTAL_SANDBOX_API_KEY" \
  "$SERVER_BIN" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 30); do
  curl -sf -H "authorization: Bearer $API_KEY" "http://127.0.0.1:$PORT/v1/version" >/dev/null && break
  sleep 1
done
curl -sf -H "authorization: Bearer $API_KEY" "http://127.0.0.1:$PORT/v1/version" >/dev/null || { cat "$SERVER_LOG"; exit 1; }

submit() {
  local branch="$1"
  local body
  body=$(python3 - "$PROVIDER" "$MODEL" "$PROMPT" "$WORK/remote.git" "$branch" "${PERMISSION_MODE-}" <<'PY'
import json, sys
provider, model, prompt, repo, branch, permission = sys.argv[1:7]
body = {
    "prompt": prompt,
    "provider": provider,
    "model": model,
    "source": "api",
    "repository": "local/e2e",
    "repo_url": repo,
    "base_ref": "main",
    "branch": branch,
}
if permission:
    body["permission_mode"] = permission
print(json.dumps(body))
PY
)
  curl -sf -X POST "http://127.0.0.1:$PORT/v1/tasks" -H "authorization: Bearer $API_KEY" -H 'content-type: application/json' -d "$body" \
    | python3 -c "import json,sys; print(json.loads(sys.stdin.read())['task_id'])"
}

TASK_IDS=()
BRANCHES=()
for n in $(seq 1 "$TASKS"); do
  branch="frontal-code/e2e-$STAMP-$n"
  id="$(submit "$branch")"
  TASK_IDS+=("$id")
  BRANCHES+=("$branch")
  echo "submitted $id on $branch"
done

# Wait for every task to finish.
for _ in $(seq 1 240); do
  pending=0
  for id in "${TASK_IDS[@]}"; do
    detail=$(curl -sf "http://127.0.0.1:$PORT/v1/tasks/$id" -H "authorization: Bearer $API_KEY")
    status=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['status'])" "$detail")
    case "$status" in completed|failed|cancelled) ;; *) pending=1 ;; esac
  done
  [ "$pending" = "0" ] && break
  sleep 2
done

failures=0
for n in "${!TASK_IDS[@]}"; do
  id="${TASK_IDS[$n]}"
  branch="${BRANCHES[$n]}"
  detail=$(curl -sf "http://127.0.0.1:$PORT/v1/tasks/$id" -H "authorization: Bearer $API_KEY")
  status=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['status'])" "$detail")
  echo "--- $id: $status"
  python3 -c "import json,sys; d=json.loads(sys.argv[1]); print(d.get('result') or ''); print('error:', d.get('error'))" "$detail"
  if [ "$status" != "completed" ]; then failures=$((failures+1)); continue; fi
  if git -C "$WORK/remote.git" rev-parse --verify --quiet "refs/heads/$branch" >/dev/null; then
    echo "published: $branch at $(git -C "$WORK/remote.git" rev-parse --short "refs/heads/$branch")"
    echo "published.txt: $(git -C "$WORK/remote.git" show "refs/heads/$branch:published.txt" 2>/dev/null || echo MISSING)"
  else
    echo "published: NO ($branch is not on the remote)"
    failures=$((failures+1))
  fi
done

echo
if [ "$failures" = "0" ]; then echo "E2E PASS: $TASKS task(s) completed and published"; else echo "E2E FAIL: $failures failure(s)"; exit 1; fi
