# Installation Guide

This guide covers installing both the Frontal Code CLI tool and the Frontal Code server.

## CLI Installation

### Homebrew (Recommended)

The easiest way to install the Frontal Code CLI is via Homebrew:

```bash
brew install --HEAD ./homebrew/frontal-code.rb
```

This will build and install the `frontal-code` binary from source.

### From Source

If you prefer to build from source:

```bash
# Clone the repository
git clone https://github.com/frontal-labs/frontal-code.git
cd frontal-code

# Build the workspace
cargo build --workspace

# Run the CLI
cargo run -p cli -- ...
```

## Server Installation

### Docker (Recommended)

The Frontal Code server is available as a Docker container:

```bash
# Build the server image
docker build -f infrastructure/docker/frontal-code-server.Dockerfile -t frontal-code-server .

# Run the server
docker run -p 8080:8080 frontal-code-server
```

### Docker Compose

For a complete setup with dependencies:

```bash
cd infrastructure/compose
cp .env.example .env
# Edit .env with your configuration
docker-compose up -d
```

### From Source

Build and run the server directly:

```bash
# Build the server
cargo build -p frontal-code-server

# Run the server
cargo run -p frontal-code-server
```

## Bazel Build Foundation (Advanced)

A hermetic, reproducible build foundation is also provided via **Bzlmod** (no
`WORKSPACE` file). All build infrastructure lives under `bazel/` and
`third_party/`; `MODULE.bazel` is the single source of truth.

### Option A — Dev container (recommended)

The dev container installs Bazel via the official apt repository (keyring
based) and layers language features (Git, Node 22, Rust 1.80, Python 3.11).

1. Open the repo in a dev container (`Dev Containers: Reopen in Container`).
2. Post-create runs `bazel run //:bootstrap`, which installs pre-commit hooks and runs
   `bazel mod tidy` (non-fatal).

### Option B — Local machine

1. Install [Bazelisk](https://github.com/bazelbuild/bazelisk). The pinned
   version comes from `.bazelversion` (`7.4.0`).
2. Bootstrap:

   ```bash
   bazel run //:bootstrap
   ```

### Common Bazel commands

| Command            | What it does                                  |
|--------------------|-----------------------------------------------|
| `bazel build //...`| Build all Bazel targets                          |
| `bazel test //...` | Run all Bazel tests                          |
| `bazel run //:buildifier -- -r .` | Format Starlark/BUILD files |
| `bazel mod tidy`   | Trim dependency information (non-fatal)        |
| `bazel run //:doctor` | Sanity-check the toolchain / environment      |
| `bazel clean`      | Clean Bazel outputs (`EXPUNGE=1` for full expunge) |
| `bazel coverage //...` → `coverage/lcov.info` | Generate coverage report |
| `bazel test //:ci`         | Build → test → lint (aggregate)                           |

### Local overrides

Never put machine-specific flags in `.bazelrc`. Add them to the gitignored
`.bazelrc.project`, for example:

```bash
common --output_user_root=~/.cache/bazel/frontal-code
```

#### Remote cache

When you have a Bazel remote cache endpoint (e.g. `bazel-remote`, GCS, S3),
uncomment the remote cache stanza in `.bazelrc.project` and fill in the URL:

```bash
build --remote_cache=grpcs://remote-cache.example.com
build --remote_upload_local_results=true
build --remote_timeout=10
```

For fully remote / CI builds you may also want to force a platform:

```bash
build --platforms=//bazel/platforms:linux_x86_64
```

## Configuration

After installation, configure your API credentials:

```bash
export FRONTAL_API_KEY="sk-ant-..."
# Or use Frontal's OpenAI-compatible API gateway
export FRONTAL_API_KEY="frontal-..."
export FRONTAL_BASE_URL="https://ai.frontal.dev/v1"
# Or use an Anthropic proxy
export FRONTAL_BASE_URL="https://your-proxy.com"
```

## Verification

Verify your installation:

```bash
# Check CLI version
frontal-code --version

# Test CLI functionality
frontal-code --help

# Test server (if running)
curl http://localhost:8080/health
```

## Quick Start

Once installed, you can start using Frontal Code:

```bash
# Interactive REPL
frontal-code --model claude-opus-5

# One-shot prompt
frontal-code prompt "explain this codebase"

# Check status
frontal-code status
```

## Troubleshooting

### Build Issues

If you encounter build issues:

1. Ensure you have Rust installed: `rustc --version`
2. Update Rust: `rustup update`
3. Clean build cache: `cargo clean`

### Permission Issues

If you get permission errors:

1. Check binary permissions: `ls -la $(which frontal-code)`
2. Reinstall with Homebrew: `brew reinstall --HEAD ./homebrew/frontal-code.rb`

### Server Issues

If the server won't start:

1. Check port availability: `lsof -i :8080`
2. Verify Docker is running: `docker version`
3. Check logs: `docker logs frontal-code-server`

For more detailed troubleshooting, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
