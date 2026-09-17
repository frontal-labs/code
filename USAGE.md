# Frontal Code Usage

This guide covers the current Rust workspace at the repository root and the `frontal-code` CLI binary. If you are brand new, make the doctor health check your first run: start `frontal-code`, then run `/doctor`.

## Quick-start health check

Run this before prompts, sessions, or automation:

```bash
brew install --HEAD ./homebrew/frontal-code.rb
frontal-code
# first command inside the REPL
/doctor
```

`/doctor` is the built-in setup and preflight diagnostic. Once you have a saved session, you can rerun it with `frontal-code --resume latest /doctor`.

## Prerequisites

- Homebrew for CLI installation, or a Rust toolchain with `cargo` for source builds
- One of:
  - `FCODE_API_KEY` for direct API access
  - `OPENAI_API_KEY` for OpenAI
  - `XAI_API_KEY` for xAI
  - `FRONTAL_API_KEY` for Frontal's OpenAI-compatible gateway
  - `BEDROCK_API_KEY` for Bedrock-compatible gateways
  - `AZURE_OPENAI_API_KEY` for Azure OpenAI-compatible gateways
  - or local `OLLAMA_BASE_URL` (defaults to `http://localhost:11434`)
- Optional: `FCODE_BASE_URL` when targeting a proxy or local service
- Optional: `FRONTAL_BASE_URL` when targeting a custom Frontal gateway URL

## Configuration

Frontal Code uses a centralized configuration system that allows you to customize behavior without modifying code.

### Configuration File

The main configuration file is `config/project.json`. It contains:

- **Project settings**: name, version, description
- **Runtime configuration**: default AI provider, timeouts, concurrency limits
- **Feature flags**: telemetry, plugins, caching, metrics, tracing
- **UI settings**: theme, colors, progress bars
- **Service configuration**: database, Redis, memory settings
- **Sandbox settings**: Docker configuration, execution limits

### Configuration File Locations

The system looks for `project.json` in this order:

1. `$FCODE_CONFIG_HOME/project.json` - Custom config directory
2. `$FCODE_HOME/project.json` - Frontal Code home directory  
3. `~/.frontal-code/project.json` - User's home directory
4. `config/project.json` - Project-local configuration

### Common Configuration Options

```json
{
  "runtime": {
    "default_provider": "frontal",
    "max_concurrent_requests": 10,
    "request_timeout_seconds": 30,
    "permission_mode": "permissive",
    "log_level": "info"
  },
  "features": {
    "enable_telemetry": true,
    "enable_plugins": true,
    "enable_caching": true,
    "enable_metrics": true,
    "enable_tracing": false
  },
  "ui": {
    "theme": "default",
    "enable_colors": true,
    "show_progress_bars": true
  }
}
```

### Checking Your Configuration

Run the doctor command to see your current configuration:

```bash
frontal-code
/doctor
```

The doctor report now includes a "Core Configuration" section showing:
- Default provider and settings
- Feature flags status
- UI preferences
- Service configurations

### Environment Variables

Environment variables take precedence over configuration file settings:

```bash
# Override default provider
export FCODE_DEFAULT_PROVIDER="openai"

# Override log level
export FCODE_LOG_LEVEL="debug"

# Override permission mode
export FCODE_PERMISSION_MODE="restricted"
```

## Install / build the workspace

```bash
# Install the CLI with Homebrew
brew install --HEAD ./homebrew/frontal-code.rb

# Or build from source
cargo build --workspace
```

The installed CLI is available as `frontal-code`. If you build from source instead, the debug binary is available at `target/debug/frontal-code`. Make the doctor check above your first post-build step.

## Quick start

### First-run doctor check

```bash
frontal-code
/doctor
```

### Interactive REPL

```bash
frontal-code
```

### One-shot prompt

```bash
frontal-code prompt "summarize this repository"
```

### Shorthand prompt mode

```bash
frontal-code "explain crates/runtime/src/lib.rs"
```

### JSON output for scripting

```bash
frontal-code --output-format json prompt "status"
```

## Model and permission controls

```bash
frontal-code --model sonnet prompt "review this diff"
frontal-code --permission-mode read-only prompt "summarize Cargo.toml"
frontal-code --permission-mode workspace-write prompt "update README.md"
frontal-code --allowedTools read,glob "inspect the runtime crate"
```

Supported permission modes:

- `read-only`
- `workspace-write`
- `danger-full-access`

Model aliases currently supported by the CLI:

- `opus` → `claude-opus-5`
- `sonnet` → `claude-sonnet-4-6`
- `haiku` → `claude-haiku-4-5`

## Provider Selection

Use the `--provider` flag to force a specific AI provider:

```bash
# Force Anthropic provider
frontal-code --provider anthropic prompt "your question"

# Force OpenAI provider
frontal-code --provider openai prompt "your question"

# Force xAI provider
frontal-code --provider xai prompt "your question"

# Combine with model aliases
frontal-code --provider anthropic --model opus prompt "complex task"
frontal-code --provider openai --model gpt-4 prompt "your question"
```

Supported providers:
- `anthropic` - Claude models via Anthropic API
- `openai` - GPT models via OpenAI API
- `xai` - Grok models via xAI API

## Authentication

### API key

```bash
export FCODE_API_KEY="sk-ant-..."
# or
export FRONTAL_API_KEY="frontal-..."
```

## Common operational commands

```bash
frontal-code status
frontal-code sandbox
frontal-code agents
frontal-code mcp
frontal-code skills
frontal-code system-prompt --cwd .. --date 2026-04-04
```

## Session management

REPL turns are persisted under `.frontal-code/sessions/` in the current workspace.

```bash
frontal-code --resume latest
frontal-code --resume latest /status /diff
```

Useful interactive commands include `/help`, `/status`, `/cost`, `/config`, `/session`, `/model`, `/permissions`, and `/export`.

## Config file resolution order

Runtime config is loaded in this order, with later entries overriding earlier ones:

1. `~/.frontal-code/settings.json`
2. `~/.config/frontal-code/settings.json`
3. `<repo>/.frontal-code/settings.json`
4. `<repo>/.frontal-code/settings.json`
5. `<repo>/.frontal-code/settings.local.json`

## Mock parity harness

The workspace includes a deterministic Anthropic-compatible mock service and parity harness.

```bash
./scripts/run_mock_parity_harness.sh
```

Manual mock service startup:

```bash
cargo run -p frontal-code-mock-gateway -- --bind 127.0.0.1:0
```

## Verification

```bash
cargo test --workspace
```

## Workspace overview

Current Rust crates:

- `api`
- `providers`
- `commands`
- `compat-harness`
- `frontal-code-mock-gateway`
- `plugins`
- `runtime`
- `cli` (package in `crates/cli/`)
- `telemetry`
- `tools`
