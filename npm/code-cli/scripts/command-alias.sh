#!/usr/bin/env bash
# Adds `code` beside `frontal-code` in the npm global bin directory, so both names work.
#
#   command-alias.sh install   create the alias, unless some other `code` is already on PATH
#   command-alias.sh remove    remove the alias, but only if this script created it
#
# VS Code also provides a `code` command. It is never replaced: if one is on PATH, this script
# leaves it alone and says so, and `frontal-code` still works.
set -euo pipefail

log() { echo "[frontal-code:alias] $*"; }

case "${1:-}" in
  install)
    fc="$(command -v frontal-code 2>/dev/null || true)"
    if [ -z "$fc" ]; then
      log "frontal-code is not on PATH yet. Add the npm global bin directory to PATH, then run: npm rebuild -g @frontal-labs/code-cli"
      exit 0
    fi
    dir="$(dirname "$fc")"
    if command -v code >/dev/null 2>&1; then
      log "a 'code' command already exists at $(command -v code); not replacing it. Use frontal-code."
      exit 0
    fi
    ln -s "$fc" "$dir/code"
    log "created $dir/code -> $fc"
    ;;
  remove)
    fc="$(command -v frontal-code 2>/dev/null || true)"
    [ -n "$fc" ] || exit 0
    dir="$(dirname "$fc")"
    # Only our own link: a symlink that points at this frontal-code.
    if [ -L "$dir/code" ] && [ "$(readlink "$dir/code")" = "$fc" ]; then
      rm -f "$dir/code"
      log "removed $dir/code"
    fi
    ;;
  *)
    echo "usage: command-alias.sh install|remove" >&2
    exit 2
    ;;
esac
