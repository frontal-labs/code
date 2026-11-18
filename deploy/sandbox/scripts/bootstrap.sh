#!/usr/bin/env bash
#
# bootstrap.sh — install the Frontal Sandbox server on the target host.
#
# Runs on the sandbox host. Idempotent: safe to re-run after a rebuild.
#
# Required environment:
#   SANDBOX_BINARY_URL   direct URL to the sandbox binary (no .tar.gz), or a
#                        "v" release tarball of the form
#                        sandbox-<version>-<arch>.tar.gz (extracted to
#                        /usr/local/bin/sandbox)
#
# Optional environment:
#   SANDBOX_API_KEY_HASH   SHA-256 (hex) of the API key code-server presents
#                          (printf '%s' "$KEY" | shasum -a 256). When set, it
#                          is injected into the systemd unit as
#                          SANDBOX_AUTH_API_KEY_HASH.
#   SANDBOX_CONFIG_SRC     local path to the TOML config to install
#                          (default: <script dir>/../config/production.toml)
#   SANDBOX_CAS_S3_BUCKET  S3 bucket name for the CAS tier (tofu output)
#   SANDBOX_CAS_S3_ENDPOINT S3 endpoint URL (from region)
#   SANDBOX_DATA_MOUNT     device to format+mount at /var/lib/sandbox
#                          (default: /dev/sdf, matching the EBS volume in
#                          compute.tf; unset to skip the mount)
#
# Steps:
#   1. Install prerequisites (util-linux, curl, ca-certificates).
#   2. Warn if /dev/kvm is absent (required for Tier 2 VM sessions).
#   3. Create the data directory and mount the EBS data volume.
#   4. Download and install the sandbox binary.
#   5. Install the TOML config.
#   6. Install and enable the systemd unit.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "${SANDBOX_BINARY_URL:-}" ]]; then
  printf '[bootstrap] ERROR: SANDBOX_BINARY_URL is required\n' >&2
  exit 1
fi

SANDBOX_API_KEY_HASH="${SANDBOX_API_KEY_HASH:-}"
SANDBOX_CONFIG_SRC="${SANDBOX_CONFIG_SRC:-${SCRIPT_DIR}/../config/production.toml}"
SANDBOX_CAS_S3_BUCKET="${SANDBOX_CAS_S3_BUCKET:-}"
SANDBOX_CAS_S3_ENDPOINT="${SANDBOX_CAS_S3_ENDPOINT:-}"
SANDBOX_DATA_MOUNT="${SANDBOX_DATA_MOUNT:-/dev/sdf}"

readonly SERVICE_NAME="sandbox-server"

log() { printf '\033[1;34m[bootstrap]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[bootstrap][warn]\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31m[bootstrap][error]\033[0m %s\n' "$*" >&2; exit 1; }

if [[ "$(id -u)" -ne 0 ]]; then
  die "This script must run as root (use: sudo $0)"
fi

# --- 1. Prerequisites -------------------------------------------------------

log "installing prerequisites"
apt-get update -qq
apt-get install -y -qq util-linux curl ca-certificates >/dev/null

# --- 2. KVM check ------------------------------------------------------------

if [[ ! -c /dev/kvm ]]; then
  warn "/dev/kvm not present — Tier 2 (VM) sessions will not work on this host."
  warn "Use a KVM-capable instance type (e.g. m5.metal) and retry, or rely on Tier 1 only."
fi

# --- 3. Data directory -------------------------------------------------------

readonly DATA_DIR=/var/lib/sandbox
log "preparing data directory at ${DATA_DIR}"
mkdir -p "${DATA_DIR}"

if [[ -n "${SANDBOX_DATA_MOUNT}" && -b "${SANDBOX_DATA_MOUNT}" ]]; then
  # Mount only if unformatted; never wipe an existing filesystem.
  if ! blkid "${SANDBOX_DATA_MOUNT}" >/dev/null 2>&1; then
    log "formatting ${SANDBOX_DATA_MOUNT} as ext4"
    mkfs.ext4 -q "${SANDBOX_DATA_MOUNT}"
  fi

  MOUNTED="$(findmnt -n -o SOURCE "${DATA_DIR}" 2>/dev/null || true)"
  if [[ "${MOUNTED}" != "${SANDBOX_DATA_MOUNT}" ]]; then
    log "mounting ${SANDBOX_DATA_MOUNT} at ${DATA_DIR}"
    mount "${SANDBOX_DATA_MOUNT}" "${DATA_DIR}"

    # Persist across reboots.
    if ! grep -q "${SANDBOX_DATA_MOUNT}" /etc/fstab 2>/dev/null; then
      UUID="$(blkid -s UUID -o value "${SANDBOX_DATA_MOUNT}")"
      echo "UUID=${UUID} ${DATA_DIR} ext4 defaults,nofail 0 2" >> /etc/fstab
    fi
  fi
fi

# --- 4. Binary ---------------------------------------------------------------

install -d -m 0755 /usr/local/bin
TMP_DOWNLOAD="$(mktemp)"
trap 'rm -f "${TMP_DOWNLOAD}"' EXIT

log "downloading sandbox binary from ${SANDBOX_BINARY_URL}"
curl -fSL --retry 3 -o "${TMP_DOWNLOAD}" "${SANDBOX_BINARY_URL}"

case "${SANDBOX_BINARY_URL}" in
  *.tar.gz)
    log "extracting tarball to /usr/local/bin/sandbox"
    tar -xzf "${TMP_DOWNLOAD}" -C /usr/local/bin --strip-components=1 sandbox
    ;;
  *)
    log "installing /usr/local/bin/sandbox"
    install -m 0755 "${TMP_DOWNLOAD}" /usr/local/bin/sandbox
    ;;
esac

# --- 5. Config ---------------------------------------------------------------

log "installing config from ${SANDBOX_CONFIG_SRC}"
install -d -m 0755 /etc/sandbox
install -m 0644 "${SANDBOX_CONFIG_SRC}" /etc/sandbox/config.toml

# --- 6. systemd unit ----------------------------------------------------------

SYSTEMD_UNIT="/etc/systemd/system/${SERVICE_NAME}.service"
log "installing ${SYSTEMD_UNIT}"

EXTRA_ENV=""
if [[ -n "${SANDBOX_API_KEY_HASH}" ]]; then
  EXTRA_ENV+="Environment=SANDBOX_AUTH_API_KEY_HASH=${SANDBOX_API_KEY_HASH}"$'\n'
fi
if [[ -n "${SANDBOX_CAS_S3_BUCKET}" ]]; then
  EXTRA_ENV+="Environment=SANDBOX_CAS_S3_BUCKET=${SANDBOX_CAS_S3_BUCKET}"$'\n'
fi
if [[ -n "${SANDBOX_CAS_S3_ENDPOINT}" ]]; then
  EXTRA_ENV+="Environment=SANDBOX_CAS_S3_ENDPOINT=${SANDBOX_CAS_S3_ENDPOINT}"$'\n'
fi

cat > "${SYSTEMD_UNIT}" <<EOF
[Unit]
Description=Frontal Sandbox server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/sandbox --config /etc/sandbox/config.toml
${EXTRA_ENV}Restart=on-failure
RestartSec=5s
LimitNOFILE=65536
# /dev/kvm is required for Tier 2 (VM) sessions.
SupplementaryGroups=kvm

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable "${SERVICE_NAME}.service"
systemctl restart "${SERVICE_NAME}.service"

log "sandbox server started"
systemctl --no-pager status "${SERVICE_NAME}.service" || true
