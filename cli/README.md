# @frontal-labs/frontal-code

Frontal Code CLI is a coding agent from Frontal that runs locally on your computer.
This npm package is a **thin launcher** that downloads and runs the native
Rust `frontal-code` binary; the real agent logic lives in the
[`crates/cli`](https://github.com/frontal-labs/frontal-code/tree/main/crates/cli)
Rust crate.

## Install

```bash
npm install -g @frontal-labs/frontal-code
# or: pnpm add -g @frontal-labs/frontal-code
# or: bunx @frontal-labs/frontal-code
# or one-off: npx @frontal-labs/frontal-code
```

On install, a `postinstall` script downloads the prebuilt native binary for
your platform from the
[GitHub releases](https://github.com/frontal-labs/frontal-code/releases) into the
package's `vendor/` directory. (`vendor/` is git- and npm-ignored; it is
populated at install time, not committed.)

## What it does

```
npm install  ──▶  postinstall  ──▶  detect platform
                                            │
                                            ▼
                              download frontal-code-<platform>.tar.gz (+ sha256 verify)
                                            │
                                            ▼
                              extract  ──▶  vendor/<target>/bin/frontal-code
```

At run time, the `frontal-code` command resolves the binary in this order:

1. **Vendored** download: `vendor/<target>/bin/frontal-code`
2. **Local cargo build** (dev/monorepo): walk up from the package to find
   `target/release/frontal-code`
3. If neither exists, an actionable error is printed (see Troubleshooting).

## Quick start

```bash
# Interactive REPL
frontal-code

# Print version / build info
frontal-code --version

# One-shot prompt
frontal-code prompt "explain this codebase"

# With a specific provider
frontal-code --provider anthropic prompt "your question"
frontal-code --provider openai   prompt "your question"

# Machine-readable output for automation
frontal-code --output-format json prompt "summarize crates/cli/src/main.rs"
```

For the full CLI reference (slash commands, permission modes, flags), see
[`crates/cli/README.md`](https://github.com/frontal-labs/frontal-code/tree/main/crates/cli).

## Configuration

Environment variables used by the native binary:

```bash
export FRONTAL_API_KEY="sk-ant-..."     # Anthropic
export OPENAI_API_KEY="sk-..."
export XAI_API_KEY="xai-..."
export FRONTAL_API_KEY="frontal-..."
export FRONTAL_BASE_URL="https://ai.frontal.dev/v1"
export FRONTAL_AUTH_TOKEN="sk-..."      # bearer token w/ custom base URL
export FRONTAL_BASE_URL="https://api.deepseek.com/anthropic"
```

Configuration files (read by the native binary):

- `~/.frontal-code/settings.json` — global user configuration
- `~/.config/frontal-code/settings.json` — system configuration
- `.frontal-code/settings.json` — workspace configuration
- `.frontal-code/settings.local.json` — local workspace overrides

## Launcher environment flags

These control the **npm launcher only** (not the agent):

| Variable | Effect |
| --- | --- |
| `FCODE_SKIP_DOWNLOAD=1` | Skip the postinstall binary download entirely. |
| `FCODE_FORCE_DOWNLOAD=1` | Force (re)download even when a binary already exists or CI skips are set (used by `npm run download`). |
| `npm_config_offline=true` | Offline install → download is skipped automatically. |

## Supported platforms

The downloader targets one of:

| Platform | Asset |
| --- | --- |
| macOS arm64 | `frontal-code-macos-arm64.tar.gz` |
| macOS x64 | `frontal-code-macos-x64.tar.gz` |
| Linux x64 | `frontal-code-linux-x64.tar.gz` |
| Windows x64 | `frontal-code-windows-x64.exe` |

These asset names are produced by
[`.github/workflows/release.yml`](https://github.com/frontal-labs/frontal-code/blob/main/.github/workflows/release.yml)
and consumed by the Homebrew formula.

## Troubleshooting

**Offline / CI install** — the postinstall is resilient: if it cannot download
(offline, no network, or a dev version like `0.0.0-dev`), it prints a warning
and still exits `0` so `npm install` never breaks. The `frontal-code` command will
report a missing binary only at run time.

**"native `frontal-code` binary not found"** — either:

- Re-run the download: `cd cli && npm run download` (or
  `npm rebuild @frontal-labs/frontal-code`).
- Build locally: `cargo build --release -p frontal-code-cli`, then the launcher will
  find `target/release/frontal-code` automatically.

**Corporate proxy** — set the standard `HTTPS_PROXY`/`HTTP_PROXY` env vars;
Node's `fetch` respects them.

**SHA-256 mismatch** — the download is aborted and the partial file removed.
Re-run `npm run download` or `npm rebuild`.

## Development

This package is dependency-free (Node stdlib only) so a globally installed CLI
has no dependency tree.

```bash
cd cli

# Build the native binary and place it in vendor/ (needs Rust/cargo)
npm run build

# Force (re)download the prebuilt binary for this platform
npm run download

# Run the JS test suite (no network required)
npm test

# Dry-run `npm pack` and inspect the published file list
npm run pack:dry

# Pre-publish guard: version check + tests
npm run prepublishOnly

# Full CI/presubmit checks
./scripts/verify.sh
```

### Repository layout

```
cli/
 ├── bin/index.js             # the `frontal-code` launcher (entry point)
├── lib/
│   ├── platform.mjs        # platform → release-asset mapping
│   ├── resolve-binary.mjs  # find vendored / local cargo binary
│   └── download.mjs        # GitHub release download + sha256 verify + extract
├── scripts/
│   ├── postinstall.sh     # install-time binary downloader
│   ├── build.sh            # cargo build → vendor/
│   ├── download.sh         # force download
│   ├── test.sh             # node --test + smoke test
│   ├── pack.sh             # npm pack dry-run
│   ├── clean.sh            # remove vendor/ + *.tgz
│   ├── prepublish.sh       # publish guard
│   └── verify.sh           # CI entrypoint
└── tests/                   # node:test suite (no network)
```

### Wiring into the monorepo

- Registered as a workspace package in the root `package.json` `packages`.
- Launcher tests: `bazel test //cli:test` (also reached by `bazel run //:cli`
  through [`scripts/cli.sh`](https://github.com/frontal-labs/frontal-code/blob/main/scripts/cli.sh)).
- Launcher lint and native vendoring build: `bazel test //cli:lint` and
  `bazel run //cli:build`. These wrappers invoke Node/Biome and Cargo respectively.
- CI: `.github/workflows/npm-cli.yml` runs on changes under `cli/**`.

## License

Apache-2.0
