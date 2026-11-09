#!/usr/bin/env bash
# Checks that the Anthropic provider works through a gateway that speaks the Messages API.
#
# It runs two requests through the CLI's `anthropic` provider:
#   1. a plain prompt, which must return the expected word;
#   2. a tool call, read_file on a file it did not know about, which must return its content.
# The second check is the one that matters: it proves the tool-use blocks and tool results
# round-trip through the gateway.
#
# Ollama serves the Messages API at /v1/messages, so it can stand in for a Claude gateway:
#
#   scripts/verify-claude-gateway.sh                          # Ollama on localhost, qwen2.5:1.5b
#   GATEWAY_URL=https://gateway.example.com GATEWAY_KEY=... MODEL=claude-... \
#     scripts/verify-claude-gateway.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY_URL="${GATEWAY_URL:-http://127.0.0.1:11434}"
GATEWAY_KEY="${GATEWAY_KEY:-ollama}"
MODEL="${MODEL:-qwen2.5:1.5b}"
CODE_BIN="${CODE_BIN:-$ROOT/target/debug/code}"

if [ ! -x "$CODE_BIN" ]; then
  echo "building the CLI..."
  cargo build -q -p cli --manifest-path "$ROOT/Cargo.toml"
fi

# Reachability check that does not depend on the CLI.
probe_status=$(curl -s -o /dev/null -w '%{http_code}' -m 60 -X POST "$GATEWAY_URL/v1/messages" \
  -H "content-type: application/json" -H "x-api-key: $GATEWAY_KEY" -H "anthropic-version: 2023-06-01" \
  -d "{\"model\":\"$MODEL\",\"max_tokens\":8,\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}" || true)
if [ "$probe_status" != "200" ]; then
  echo "FAIL: $GATEWAY_URL/v1/messages returned HTTP $probe_status for model $MODEL" >&2
  exit 1
fi
echo "ok: gateway answers the Messages API"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/claude-gateway.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
printf 'gateway-check-version 7.7.7\n' > "$WORK/VERSION.txt"

export FRONTAL_BASE_URL="$GATEWAY_URL"
export FRONTAL_API_KEY="$GATEWAY_KEY"

plain=$(cd "$WORK" && "$CODE_BIN" --provider anthropic --model "$MODEL" prompt "Reply with the single word: pong" 2>&1 || true)
if ! grep -qi "pong" <<<"$plain"; then
  echo "FAIL: plain prompt did not return 'pong':" >&2
  echo "$plain" | tail -5 >&2
  exit 1
fi
echo "ok: plain prompt round-trips"

tool=$(cd "$WORK" && "$CODE_BIN" --provider anthropic --model "$MODEL" --permission-mode read-only prompt \
  "Read the file VERSION.txt with the read_file tool and tell me the version number." 2>&1 || true)
if ! grep -q "7.7.7" <<<"$tool"; then
  echo "FAIL: tool call did not return the file's version:" >&2
  echo "$tool" | tail -8 >&2
  exit 1
fi
echo "ok: tool call round-trips through the gateway"
echo "PASS: $MODEL via $GATEWAY_URL"
