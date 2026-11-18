#!/usr/bin/env bash
#
# smoke.sh — end-to-end smoke test for the Frontal Sandbox API.
#
# Confirms the server is reachable with a valid API key, creates a session,
# runs `echo hello` in it, reads the output, and destroys the session.
# Exits non-zero if any step fails.
#
# Required:
#   SANDBOX_BASE_URL   e.g. http://1.2.3.4:8080 (from `tofu output -raw sandbox_base_url`)
#   SANDBOX_API_KEY    the API key (raw, not hashed)
#
# Optional:
#   SANDBOX_SESSION_TTL  seconds before the test gives up waiting on exec
#                        (default: 60)
set -euo pipefail

BASE_URL="${SANDBOX_BASE_URL:?SANDBOX_BASE_URL is required (e.g. http://1.2.3.4:8080)}"
API_KEY="${SANDBOX_API_KEY:?SANDBOX_API_KEY is required}"
TTL="${SANDBOX_SESSION_TTL:-60}"

# Trim trailing slash.
BASE_URL="${BASE_URL%/}"

log() { printf '\033[1;32m[smoke]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[smoke][error]\033[0m %s\n' "$*" >&2; exit 1; }

api() {
  local method="$1" path="$2" body="${3:-}"
  if [[ -n "${body}" ]]; then
    curl -fsS -X "${method}" \
      -H "Authorization: Bearer ${API_KEY}" \
      -H 'Content-Type: application/json' \
      -d "${body}" \
      "${BASE_URL}${path}"
  else
    curl -fsS -X "${method}" \
      -H "Authorization: Bearer ${API_KEY}" \
      "${BASE_URL}${path}"
  fi
}

# --- 0. Sanity: server reachable and API key valid ---------------------------

log "fetching OpenAPI spec"
OPENAPI_RESPONSE="$(api GET /v1/openapi.json)" || die "cannot reach ${BASE_URL} — is the server up and the API key correct?"
printf '%s' "${OPENAPI_RESPONSE}" | python3 -c 'import sys, json; json.load(sys.stdin)' \
  || die "server responded but /v1/openapi.json is not valid JSON"
log "server reachable, API key accepted"

# --- 1. Create a session ----------------------------------------------------

log "creating session"
CREATE_RESPONSE="$(api POST /v1/sessions '{"image":"ubuntu:24.04"}')" \
  || die "failed to create a session (check FRONTAL_SANDBOX_API_KEY matches the server hash)"
SESSION_ID="$(printf '%s' "${CREATE_RESPONSE}" | python3 -c 'import sys, json; print(json.load(sys.stdin)["id"])')"
log "session created: ${SESSION_ID}"

cleanup() {
  log "destroying session ${SESSION_ID}"
  api DELETE "/v1/sessions/${SESSION_ID}" >/dev/null || true
}
trap cleanup EXIT

# --- 2. Run a command -------------------------------------------------------

log "running 'echo hello' in session"
EXEC_RESPONSE="$(api POST "/v1/sessions/${SESSION_ID}/exec" '{"command":"echo hello"}')"
EXEC_ID="$(printf '%s' "${EXEC_RESPONSE}" | python3 -c 'import sys, json; print(json.load(sys.stdin)["id"])')"

# --- 3. Read output ---------------------------------------------------------

log "waiting for exec output (up to ${TTL}s)"
DEADLINE=$(( $(date +%s) + TTL ))
OUTPUT=""
STATUS="running"
while [[ "$(date +%s)" -lt "${DEADLINE}" ]]; do
  EXEC_STATE="$(api GET "/v1/sessions/${SESSION_ID}/exec/${EXEC_ID}")"
  STATUS="$(printf '%s' "${EXEC_STATE}" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("status","running"))')"
  if [[ "${STATUS}" == "completed" || "${STATUS}" == "failed" ]]; then
    OUTPUT="$(printf '%s' "${EXEC_STATE}" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("stdout","").strip())')"
    break
  fi
  sleep 1
done

if [[ "${STATUS}" != "completed" ]]; then
  die "exec did not complete (status=${STATUS:-unknown})"
fi

log "output: ${OUTPUT}"
if [[ "${OUTPUT}" != "hello" ]]; then
  die "expected 'hello', got '${OUTPUT}'"
fi

log "PASS"
