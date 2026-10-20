# Homebrew snapshot

This directory contains a point-in-time snapshot of the Homebrew installation
on this machine (macOS 26.6.2 arm64, prefix `/opt/homebrew`, Homebrew 7.0.9).

## Files

| File | Contents |
|---|---|
| `brew-config.txt` | `brew config` output |
| `brew-env.txt` | `brew --env` output |
| `taps.txt` | `brew tap` list |
| `formulae-versions.txt` | `brew list --formula --versions` |
| `services.txt` | `brew services list` |
| `analytics-developer.txt` | `brew analytics` + `brew developer` state |
| `casks-versions.txt` | Excluded (see note below) |

## Known issue

`stainless-api/tap` is untrusted, so its cask (`stl`) fails to load and
`snapshot/casks-versions.txt` is excluded. Trust it via:

    brew trust --tap stainless-api/tap

Then regenerate the snapshot with:

    homebrew/bin/brew-sync.sh dump

## Regenerating

    homebrew/bin/brew-sync.sh dump   # regenerate Brewfile + snapshot
    homebrew/bin/brew-sync.sh check  # diff install vs Brewfile
    homebrew/bin/brew-sync.sh apply  # install Brewfile after confirmation

Use `HOMEBREW_BUNDLE_FILE=homebrew/Brewfile brew bundle <cmd>` to target this
folder directly for a single command.

## Secrets

None committed; tokens live in keychain/CI secret store.

## Notable

- Brewfile includes `coder/coder/coder-desktop` with `trusted: true`.
- Mise (activated via `~/.zshrc`) is not managed here.
