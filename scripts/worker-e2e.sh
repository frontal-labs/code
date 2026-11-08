#!/usr/bin/env bash
# End-to-end check for hosted worker lanes.
#
# Builds the worker image, starts code-server with the local-docker transport, and runs
# one task against a local bare repository. The task writes a file and tries to reach the
# internet. While it runs, the script checks the worker container's isolation settings
# and environment names (never values). When it finishes, the script checks that the
# server published the branch to the remote.
#
# Requirements: a running Docker daemon, a Postgres database for worker tokens, and a
# provider key in the environment.
#
#   OPENAI_API_KEY=... OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
#   FRONTAL_CODE_SERVER_DATABASE_URL=postgres://user:pass@127.0.0.1:5432/db \
#     scripts/worker-e2e.sh [model] [provider]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FRONTAL_CODE_SERVER_DOCKER_IMAGE:-frontal-code-worker:local}"
PORT="${FRONTAL_CODE_E2E_PORT:-8790}"
MODEL="${1:-nvidia/nemotron-3-ultra-550b-a55b:free}"
PROVIDER="${2:-openai}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/frontal-worker-e2e.XXXXXX")"
SERVER_LOG="$WORK/server.log"
API_KEY="e2e-$(date +%s)-$RANDOM"
BRANCH="frontal-code/e2e-$(date +%s)"
PROMPT="${PROMPT:-Create a file named published.txt in the current directory containing exactly: from the worker. Then run: curl -m 5 -sS https://example.com -o /dev/null; echo CURL_EXIT=\$?  Reply with the exit code line and the word DONE.}"

: "${FRONTAL_CODE_SERVER_DATABASE_URL:?set FRONTAL_CODE_SERVER_DATABASE_URL to a Postgres database}"

if ! docker info >/dev/null 2>&1; then
  echo "error: Docker daemon is not reachable" >&2
  exit 1
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1 || [ "${REBUILD:-0}" = "1" ]; then
  echo "building $IMAGE ..."
  docker build -f "$ROOT/docker/worker.Dockerfile" -t "$IMAGE" "$ROOT"
fi

# A bare repository stands in for GitHub. The server publishes to it.
git init -q --bare "$WORK/remote.git"
git init -q -b main "$WORK/seed"
echo "hello" > "$WORK/seed/README.md"
git -C "$WORK/seed" add .
git -C "$WORK/seed" -c user.name=seed -c user.email=seed@local commit -q -m init
git -C "$WORK/seed" push -q "$WORK/remote.git" main

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

BODY=$(python3 - "$PROVIDER" "$MODEL" "$PROMPT" "$WORK/remote.git" "$BRANCH" "${PERMISSION_MODE-}" <<'PY'
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
RESP=$(curl -sf -X POST "http://127.0.0.1:$PORT/v1/tasks" -H "authorization: Bearer $API_KEY" -H 'content-type: application/json' -d "$BODY")
TASK_ID=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['task_id'])" "$RESP")
echo "submitted $TASK_ID on $BRANCH"

# While the worker runs, check its container: isolation settings and environment names.
# Values never leave Python.
CHECKED=0
check_worker() {
  local cid
  cid=$(docker ps -q --filter "label=frontal-code.task_id=$TASK_ID" | head -1)
  [ -n "$cid" ] || return 0
  docker inspect "$cid" | python3 -c '
import json, sys
c = json.load(sys.stdin)[0]
hc = c["HostConfig"]
env = dict(item.split("=", 1) for item in c["Config"]["Env"] if "=" in item)
print("worker isolation:")
print("  cap_drop:          ", hc.get("CapDrop"))
print("  read_only_rootfs:  ", hc.get("ReadonlyRootfs"))
print("  security_opt:      ", hc.get("SecurityOpt"))
print("  network:           ", hc.get("NetworkMode"))
print("  memory_bytes:      ", hc.get("Memory"))
print("  privileged:        ", hc.get("Privileged"))
print("  mounts:            ", [m["Destination"] for m in c.get("Mounts", [])])
print("worker env names:  ", sorted(n for n in env if n.endswith(("_KEY", "_TOKEN", "_BASE_URL")) or n.startswith(("FRONTAL_", "OLLAMA"))))
bad = [n for n in env if n in ("GITHUB_TOKEN", "FRONTAL_CODE_GITHUB_API_BASE")]
bad += [n for n in env if n.endswith("_API_KEY") and n != "FRONTAL_CODE_SERVER_API_KEY" and env[n] != env.get("FRONTAL_CODE_SERVER_API_KEY")]
print("RESULT: no GitHub or upstream provider credential in worker" if not bad else "RESULT: CREDENTIAL PRESENT: " + ", ".join(bad))
'
  CHECKED=1
}

STATUS=""
DETAIL=""
for _ in $(seq 1 180); do
  DETAIL=$(curl -sf "http://127.0.0.1:$PORT/v1/tasks/$TASK_ID" -H "authorization: Bearer $API_KEY")
  [ "$CHECKED" = "0" ] && check_worker
  STATUS=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['status'])" "$DETAIL")
  case "$STATUS" in completed|failed|cancelled) break ;; esac
  sleep 2
done

echo "status: $STATUS"
python3 -c "import json,sys; d=json.loads(sys.argv[1]); print('result:', d.get('result')); print('error:', d.get('error'))" "$DETAIL"

# The branch must exist on the remote, with the worker's file in it.
if git -C "$WORK/remote.git" rev-parse --verify --quiet "refs/heads/$BRANCH" >/dev/null; then
  echo "published: $BRANCH at $(git -C "$WORK/remote.git" rev-parse --short "refs/heads/$BRANCH")"
  echo "published.txt: $(git -C "$WORK/remote.git" show "refs/heads/$BRANCH:published.txt" 2>/dev/null || echo MISSING)"
else
  echo "published: NO (branch $BRANCH is not on the remote)"
  exit 1
fi
[ "$STATUS" = "completed" ]
