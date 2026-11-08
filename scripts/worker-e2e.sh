#!/usr/bin/env bash
# End-to-end check for hosted worker lanes.
#
# Builds the worker image, starts code-server with the local-docker transport, and runs
# TASKS tasks at once against a local bare repository. Each task writes a file with an
# allowed command and tries a denied one. While the first task runs, the script checks the
# worker container's isolation settings and environment names (never values). When the tasks
# finish, the script checks that each branch reached the remote.
#
# One server per Docker host: the relay is a single container, so a second server on the
# same host would replace it. Run concurrent tasks through one server, as this script does.
#
# Requirements: a running Docker daemon, a Postgres database for worker tokens, and the
# provider. The default provider is a local Ollama server, which needs no key:
#
#   OLLAMA_BASE_URL=http://127.0.0.1:11434 \
#   FRONTAL_CODE_SERVER_DATABASE_URL='postgres://user:pass@127.0.0.1:55433/db?sslmode=verify-full' \
#   FRONTAL_CODE_SERVER_DATABASE_CA_FILE=/path/to/ca.crt \
#     scripts/worker-e2e.sh [model] [provider]
#
# For a cloud OpenAI-compatible provider pass `openai` and set OPENAI_API_KEY (and
# OPENAI_BASE_URL for a non-OpenAI endpoint).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FRONTAL_CODE_SERVER_DOCKER_IMAGE:-frontal-code-worker:local}"
PORT="${FRONTAL_CODE_E2E_PORT:-8790}"
TASKS="${TASKS:-2}"
MODEL="${1:-qwen2.5:1.5b}"
PROVIDER="${2:-ollama}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/frontal-worker-e2e.XXXXXX")"
SERVER_LOG="$WORK/server.log"
API_KEY="e2e-$(date +%s)-$RANDOM"
STAMP="$(date +%s)"
PROMPT="${PROMPT:-Create a file named published.txt in the current directory containing exactly: from the worker. Then run: python3 -c 'print(1)'. Then run: curl -m 5 -sS https://example.com -o /dev/null; echo CURL_EXIT=\$?. Then run: rm -rf /tmp/anything; echo RM_EXIT=\$?. Reply with the exit code lines and the word DONE.}"

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

# While the first worker runs, check its container: isolation settings and environment
# names. Values never leave Python.
CHECKED=0
check_worker() {
  local cid
  cid=$(docker ps -q --filter "label=frontal-code.task_id=${TASK_IDS[0]}" | head -1)
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

# Wait for every task to finish.
for _ in $(seq 1 240); do
  pending=0
  for id in "${TASK_IDS[@]}"; do
    detail=$(curl -sf "http://127.0.0.1:$PORT/v1/tasks/$id" -H "authorization: Bearer $API_KEY")
    [ "$CHECKED" = "0" ] && check_worker
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

# A finished task's network is removed when its worker exits. Poll briefly, then fail if any
# remain. The name follows docker_worker::task_network_name: lowercase, safe characters, 48 max.
for id in "${TASK_IDS[@]}"; do
  net="frontal-task-$(printf %s "$id" | tr 'A-Z' 'a-z' | tr -c 'a-z0-9_-' '-' | cut -c1-48)"
  gone=0
  for _ in $(seq 1 15); do
    if ! docker network inspect "$net" >/dev/null 2>&1; then gone=1; break; fi
    sleep 2
  done
  if [ "$gone" = "1" ]; then
    echo "network: $net removed"
  else
    echo "network: $net still present after the task finished"
    failures=$((failures+1))
  fi
done

echo
if [ "$failures" = "0" ]; then echo "E2E PASS: $TASKS task(s) completed and published"; else echo "E2E FAIL: $failures failure(s)"; exit 1; fi
