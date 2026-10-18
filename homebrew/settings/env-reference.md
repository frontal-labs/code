# HOMEBREW_* environment variable reference

This document lists the `HOMEBREW_*` environment variables referenced in
`homebrew/settings/homebrew.env.example`. Names only — no secrets or values
are committed here.

## General behaviour

| Variable | Purpose |
|---|---|
| `HOMEBREW_NO_AUTO_UPDATE` | Skip `brew update` before install/upgrade commands. |
| `HOMEBREW_NO_INSTALL_CLEANUP` | Skip cleanup of old versions after installing. |
| `HOMEBREW_NO_ENV_HINTS` | Suppress environment hint messages. |
| `HOMEBREW_NO_ANALYTICS` | Disable Homebrew analytics reporting. |
| `HOMEBREW_DEVELOPER` | Enable developer mode (updates from `main` branch, developer commands). |

## Toolchain paths

| Variable | Purpose |
|---|---|
| `HOMEBREW_GIT` | Path to the git binary used by Homebrew. |
| `HOMEBREW_EDITOR` | Editor used by `brew edit`, `brew cat`, etc. |
| `HOMEBREW_BROWSER` | Browser used to open project homepages. |

## Prefix & paths

| Variable | Purpose |
|---|---|
| `HOMEBREW_PREFIX` | Installation prefix (default `/opt/homebrew` on Apple Silicon). |
| `HOMEBREW_CELLAR` | Where installed formulae are stored. |
| `HOMEBREW_REPOSITORY` | Homebrew git repository location. |
| `HOMEBREW_LIBRARY_PATH` | Homebrew library path. |
| `HOMEBREW_CACHE` | Download cache directory. |
| `HOMEBREW_LOGS` | Log directory. |
| `HOMEBREW_TEMP` | Temporary build directory. |

## Bundle

| Variable | Purpose |
|---|---|
| `HOMEBREW_BUNDLE_FILE` | Path to the Brewfile used by `brew bundle` (set to `homebrew/Brewfile`). |
| `HOMEBREW_NO_INSTALL_FROM_API` | Install from local taps instead of the Homebrew API. |
| `HOMEBREW_TRUSTED_CASKS` | Comma-separated cask tokens marked trusted (e.g. `coder/coder/coder-desktop`). |

## Usage

```bash
# Source the example (after creating homebrew.env with your values):
set -a; . homebrew/settings/homebrew.env; set +a

# Or target the Brewfile directly for a single command:
HOMEBREW_BUNDLE_FILE=homebrew/Brewfile brew bundle check
```

## Secrets

No tokens or credentials are stored in this repo. Sensitive values live in the
macOS keychain or the CI secret store and are injected at runtime.
