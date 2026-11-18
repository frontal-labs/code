#!/usr/bin/env bash
#
# deploy.sh — (re)deploy the sandbox server on an already-bootstrapped host.
#
# v1 behavior:
#   1. Verify the sandbox binary is installed.
#   2. Restart the systemd service.
#   3. Verify the service is active.
#   4. Print the base URL.
#
# Run as root on the host:
#   sudo /tmp/sandbox-deploy/scripts/deploy.sh

set -euo pipefail

readonly SERVICE_NAME="sandbox-server"
readonly BINARY_PATH="/usr/local/bin/sandbox"

log() { printf '\033[1;34m[deploy]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[deploy][error]\033[0m %s\n' "$*" >&2; exit 1; }

if [[ "$(id -u)" -ne 0 ]]; then
  die "This script must run as root (use: sudo $0)"
fi

if [[ ! -x "${BINARY_PATH}" ]]; then
  die "Sandbox binary not found at ${BINARY_PATH}. Run bootstrap.sh first: sudo SANDBOX_BINARY_URL=<url> $(dirname "${BASH_SOURCE[0]}")/bootstrap.sh"
fi

log "Restarting ${SERVICE_NAME}..."
systemctl restart "${SERVICE_NAME}"

# Give the server a moment to bind its port before checking status.
sleep 2

if ! systemctl is-active --quiet "${SERVICE_NAME}"; then
  die "${SERVICE_NAME} is not active after restart. Check: journalctl -u ${SERVICE_NAME} -n 50"
fi

log "Service is running."
HOST_IP="$(hostname -I | awk '{print $1}')"
log "Base URL: http://${HOST_IP}:8080"
log "Configure code-server with:"
log "  FRONTAL_SANDBOX_BASE_URL=http://${HOST_IP}:8080"
log "  FRONTAL_SANDBOX_API_KEY=<your api key>"
log "  FRONTAL_CODE_SERVER_LANE_TRANSPORT=sandbox"
