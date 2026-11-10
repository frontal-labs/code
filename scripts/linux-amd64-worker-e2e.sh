#!/usr/bin/env bash
# Runs code-server inside a linux/amd64 container (Rosetta on Apple silicon), with the host Docker
# daemon driving the worker containers through the mounted socket. Then it submits two Ollama
# tasks and checks that both finish, publish their branches, and leave no task networks behind.
#
# Needs: Docker, the dev services (scripts/dev-services.sh), a local Ollama with the model below,
# and the worker image (docker build -f docker/worker.Dockerfile -t frontal-code-worker:local .).
#
#   scripts/linux-amd64-worker-e2e.sh [model]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="${1:-qwen2.5:1.5b}"
PORT="${FRONTAL_CODE_E2E_PORT:-8790}"
TLS_DIR="${TLS_DIR:-$HOME/.frontal-code-dev/tls}"
PG_PASSWORD="${PG_PASSWORD:-dev-local-only}"
OLLAMA_URL="${OLLAMA_BASE_URL:-http://127.0.0.1:11434}"
OLLAMA_PORT="${OLLAMA_URL##*:}"
OLLAMA_PORT="${OLLAMA_PORT%%/*}"
CONTAINER="frontal-linux-server"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/frontal-linux-e2e.XXXXXX")"
KEY="linux-e2e-$RANDOM$RANDOM"

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


if ! curl -sf -m 5 "http://127.0.0.1:$OLLAMA_PORT/api/tags" >/dev/null; then
  echo "error: Ollama is not answering on port $OLLAMA_PORT" >&2
  exit 1
fi

"$ROOT/scripts/dev-services.sh" up >/dev/null
SRC="$(snapshot_source)"

git init -q --bare "$WORK/remote.git"
git init -q -b main "$WORK/seed"
echo hello > "$WORK/seed/README.md"
git -C "$WORK/seed" add . && git -C "$WORK/seed" -c user.name=seed -c user.email=seed@local commit -q -m init
git -C "$WORK/seed" push -q "$WORK/remote.git" main

docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker rm -f "frontal-code-relay-$PORT" >/dev/null 2>&1 || true
docker run -d --name "$CONTAINER" --platform linux/amd64 -p "127.0.0.1:$PORT:$PORT" \
  --add-host host.docker.internal:host-gateway \
  -v "$SRC":/src -w /src \
  -v frontal-cargo-registry:/usr/local/cargo/registry -v frontal-target-amd64:/src/target-linux \
  -v "$WORK":"$WORK" -v /var/run/docker.sock:/var/run/docker.sock -v "$TLS_DIR":/certs:ro \
  -e FRONTAL_CODE_SERVER_HOST=0.0.0.0 -e FRONTAL_CODE_SERVER_PORT="$PORT" \
  -e FRONTAL_CODE_SERVER_API_KEY="$KEY" -e FRONTAL_CODE_SERVER_LANE_TRANSPORT=local-docker \
  -e FRONTAL_CODE_SERVER_DOCKER_IMAGE=frontal-code-worker:local \
  -e FRONTAL_CODE_SERVER_CALLBACK_URL="http://host.docker.internal:$PORT" \
  -e FRONTAL_CODE_SERVER_WORKSPACE_ROOT="$WORK/workspaces" \
  -e FRONTAL_CODE_SERVER_STATE_FILE="$WORK/state.json" \
  -e FRONTAL_CODE_SERVER_DATABASE_URL="postgres://frontal:$PG_PASSWORD@host.docker.internal:55433/frontal?sslmode=verify-full" \
  -e FRONTAL_CODE_SERVER_DATABASE_CA_FILE=/certs/ca.crt \
  -e OLLAMA_BASE_URL="http://host.docker.internal:$OLLAMA_PORT" \
  rust:1.95-bookworm bash -c 'apt-get update -qq && apt-get install -y -qq docker.io >/dev/null 2>&1; CARGO_TARGET_DIR=/src/target-linux cargo build -q -p code-server && exec /src/target-linux/debug/code-server' >/dev/null

cleanup() { docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; }
trap cleanup EXIT

for _ in $(seq 1 150); do
  curl -sf -H "authorization: Bearer $KEY" "http://127.0.0.1:$PORT/v1/version" >/dev/null 2>&1 && break
  sleep 4
done
curl -sf -H "authorization: Bearer $KEY" "http://127.0.0.1:$PORT/v1/version" >/dev/null \
  || { echo "error: server did not start"; docker logs "$CONTAINER" 2>&1 | tail -30; exit 1; }

PROMPT="Create a file named published.txt in the current directory (use the relative path published.txt, never an absolute path) containing exactly: from the linux worker. Reply DONE."
IDS=()
for n in 1 2; do
  BRANCH="frontal-code/linux-e2e-$n"
  BODY=$(python3 -c 'import json,sys; print(json.dumps({"prompt":sys.argv[1],"provider":"ollama","model":sys.argv[2],"source":"api","repository":"local/linux-e2e","repo_url":sys.argv[3],"base_ref":"main","branch":sys.argv[4]}))' "$PROMPT" "$MODEL" "$WORK/remote.git" "$BRANCH")
  ID=$(curl -sf -X POST "http://127.0.0.1:$PORT/v1/tasks" -H "authorization: Bearer $KEY" -H 'content-type: application/json' -d "$BODY" | python3 -c "import json,sys; print(json.load(sys.stdin)['task_id'])")
  echo "submitted $ID on $BRANCH"
  IDS+=("$ID")
done

failures=0
for ID in "${IDS[@]}"; do
  ST=""
  for _ in $(seq 1 200); do
    ST=$(curl -sf -H "authorization: Bearer $KEY" "http://127.0.0.1:$PORT/v1/tasks/$ID" | python3 -c "import json,sys; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
    case "$ST" in completed|failed|cancelled) break ;; esac
    sleep 3
  done
  echo "$ID: $ST"
  [ "$ST" = "completed" ] || failures=$((failures+1))
done

for n in 1 2; do
  BRANCH="frontal-code/linux-e2e-$n"
  if git -C "$WORK/remote.git" rev-parse --verify --quiet "refs/heads/$BRANCH" >/dev/null; then
    echo "published: $BRANCH: $(git -C "$WORK/remote.git" show "$BRANCH:published.txt" 2>/dev/null | head -1)"
  else
    echo "published: NO ($BRANCH)"
    failures=$((failures+1))
  fi
done

sleep 10
LEFT="$(docker network ls --filter name=frontal-task --format '{{.Name}}')"
if [ -n "$LEFT" ]; then
  echo "task networks still present: $LEFT"
  failures=$((failures+1))
else
  echo "task networks: none left"
fi

if [ "$failures" = "0" ]; then echo "LINUX E2E PASS"; else echo "LINUX E2E FAIL: $failures failure(s)"; docker logs "$CONTAINER" 2>&1 | tail -20; exit 1; fi
