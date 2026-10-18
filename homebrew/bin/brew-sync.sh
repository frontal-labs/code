#!/usr/bin/env bash
set -euo pipefail

# brew-sync.sh — Homebrew setup sync for this machine.
#
# Modes:
#   dump   regenerate Brewfile + snapshot from the current installation
#   check  diff installed packages vs Brewfile (brew bundle check)
#   apply  install the Brewfile after confirmation (brew bundle install)
#
# All brew bundle commands target homebrew/Brewfile via
# HOMEBREW_BUNDLE_FILE=homebrew/Brewfile.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOMEBREW_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$HOMEBREW_DIR/.." && pwd)"
BREWFILE="$HOMEBREW_DIR/Brewfile"
SNAPSHOT_DIR="$HOMEBREW_DIR/snapshot"

export HOMEBREW_BUNDLE_FILE="$BREWFILE"

usage() {
  echo "Usage: $0 {dump|check|apply}" >&2
  exit 1
}

require_brew() {
  if ! command -v brew >/dev/null 2>&1; then
    echo "Error: brew not found in PATH" >&2
    exit 1
  fi
}

cmd_dump() {
  require_brew
  echo "==> Dumping Brewfile to $BREWFILE"
  brew bundle dump --force --file="$BREWFILE"

  echo "==> Regenerating snapshot in $SNAPSHOT_DIR"
  mkdir -p "$SNAPSHOT_DIR"

  brew config > "$SNAPSHOT_DIR/brew-config.txt"
  brew --env > "$SNAPSHOT_DIR/brew-env.txt"
  brew tap > "$SNAPSHOT_DIR/taps.txt"
  brew list --formula --versions > "$SNAPSHOT_DIR/formulae-versions.txt"
  brew services list > "$SNAPSHOT_DIR/services.txt"
  {
    brew analytics
    echo "---"
    brew developer
  } > "$SNAPSHOT_DIR/analytics-developer.txt"

  # Cask versions snapshot is excluded while stainless-api/tap is untrusted
  # (its cask "stl" fails to load). Trust the tap to re-enable:
  #   brew trust --tap stainless-api/tap
  cat > "$SNAPSHOT_DIR/casks-versions.txt" <<'EOF'
# Cask versions snapshot — EXCLUDED

This file is intentionally not generated.

Reason: the `stainless-api/tap` is untrusted, so its cask `stl` fails to
load during `brew bundle dump` / `brew list --cask --versions`. Until the tap
is trusted, cask version snapshotting is skipped to avoid incomplete/inaccurate
output.

To trust the tap and re-enable cask snapshotting:

    brew trust --tap stainless-api/tap

After trusting, regenerate with:

    homebrew/bin/brew-sync.sh dump
EOF

  echo "==> Done. Brewfile and snapshot regenerated."
}

cmd_check() {
  require_brew
  echo "==> Checking installed packages vs $BREWFILE"
  brew bundle check
}

cmd_apply() {
  require_brew
  echo "==> About to install from $BREWFILE"
  read -rp "Proceed with brew bundle install? [y/N] " -n 1 reply
  echo
  if [[ ! "$reply" =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 0
  fi
  brew bundle install
}

main() {
  if [[ $# -ne 1 ]]; then
    usage
  fi
  case "$1" in
    dump)  cmd_dump ;;
    check) cmd_check ;;
    apply) cmd_apply ;;
    *)     usage ;;
  esac
}

main "$@"
