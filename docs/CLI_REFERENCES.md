# CLI Reference

This comprehensive reference covers all Frontal Code CLI commands, options, and usage patterns.

> Audited on 2026-11-08 against `cli/src/main.rs`. Commands and flags that are not listed in the parser are marked *not implemented*. The `session`, `tools`, and singular `plugin` sections describe commands the parser does not accept as top-level subcommands. Use the slash commands in the REPL (`/session`, `/tools`) or the plural `plugins`.

## Table of Contents

- [Global Options](#global-options)
- [Commands](#commands)
- [Slash Commands](#slash-commands)
- [Model Aliases](#model-aliases)
- [Permission Modes](#permission-modes)
- [Output Formats](#output-formats)
- [Exit Codes](#exit-codes)

## Global Options

These options are parsed before the subcommand, and most of them can also be placed after it:

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--model MODEL` | | Model name or alias (`opus`, `sonnet`, `haiku`, `fable`) | `claude-opus-5-5` |
| `--provider NAME` | | Provider: `anthropic`, `openai`, `xai`, `frontal`, `bedrock`, `azure`, `ollama` | configured default (`frontal`) |
| `--output-format FORMAT` | | `text` or `json` | `text` |
| `--permission-mode MODE` | | `read-only`, `workspace-write`, `prompt`, or `danger-full-access` | `prompt` |
| `--dangerously-skip-permissions` | | Same as `--permission-mode danger-full-access` | |
| `--allowed-tools LIST` / `--allowedTools` | | Comma-separated tool allowlist | all tools |
| `--resume SESSION` | | Resume a session by ID, `latest`, or path, followed by slash commands | |
| `--print` | | Force text output (SDK compatibility) | |
| `-p TEXT` | | One-shot prompt (same as `prompt -p`) | |
| `--version` | `-V` | Show version | |
| `--help` | `-h` | Show help | |

**Not implemented:** `--config FILE`, `--verbose`, `--quiet`, `--no-color`. Use `--output-format json` for machine-readable output.

### Model Selection

```bash
# Full model names
frontal-code --model claude-opus-5
frontal-code --model claude-sonnet-4-6
frontal-code --model claude-haiku-4-5

# Model aliases
frontal-code --model opus      # claude-opus-5
frontal-code --model sonnet     # claude-sonnet-4-6
frontal-code --model haiku      # claude-haiku-4-5

# Provider-specific models
frontal-code --provider openai --model gpt-4
frontal-code --provider xai --model grok-3
```

### Provider Selection

```bash
# Anthropic (default)
frontal-code --provider anthropic

# OpenAI
frontal-code --provider openai --model gpt-4-turbo

# xAI
frontal-code --provider xai --model grok-3

# Frontal (API gateway)
frontal-code --provider frontal
```

## Commands

### prompt

Send one prompt to the model and print the result.

```bash
frontal-code prompt [-p TEXT | TEXT...] [--stream] [--resume SESSION] [--config KEY=VALUE]
```

Also accepted as shorthand: `frontal-code "explain crates/code-runtime/src/lib.rs"`, or `frontal-code -p "..."`.

Free text is sent to the model, so unrecognized `--flags` are rejected with `unknown option` instead of being sent as text.

**Options** (`prompt` subcommand)
- `-p TEXT`: prompt text. Words run until the next `--flag`.
- `--stream`: print JSONL turn events. Requires `--output-format json`. See [JSONL events](#jsonl-events).
- `--resume SESSION`: continue a stored session by ID or `latest`. The transcript is kept, and the model and provider from this run apply to the next turn.
- `--config KEY=VALUE`: run-scoped override. Supported keys: `frontal_base_url` (overrides `FRONTAL_BASE_URL`) and `skip_git_repo_check` (accepted; the CLI does not check for a git repository). Other keys are rejected.

**Not implemented**: `--image`, `--file`, `--template`, `--context`, `--temperature`, `--max-tokens`. The output token limit is set with `FRONTAL_CODE_MAX_OUTPUT_TOKENS` (see [Environment Variables](#environment-variables)).

**Examples**
```bash
frontal-code prompt "What files are in the current directory?"
frontal-code --provider openai --model google/gemma-4-26b-a4b-it --permission-mode read-only prompt "Summarize Cargo.toml"
frontal-code --output-format json prompt "status"
frontal-code prompt -p "Continue the refactor" --resume latest --output-format json --stream
```

### JSONL events

`prompt --output-format json --stream` prints one JSON object per line. The Rust and TypeScript SDKs parse this stream.

```
{"type":"turn.started"}
{"type":"item.completed","item":{"type":"tool_use","name":"read_file","input":"{\"path\":\"Cargo.toml\"}"}}
{"type":"item.completed","item":{"type":"tool_result","content":"..."}}
{"type":"item.completed","item":{"type":"text","text":"0.1.1"}}
{"type":"turn.completed","finalResponse":"0.1.1","sessionId":"session-...","usage":{...}}
```

On failure the stream ends with `{"type":"turn.failed","error":"..."}` and the process exits non-zero. Events are emitted when the turn finishes, not token by token.

### repl

Start the interactive REPL. `frontal-code` with no arguments does the same.

```bash
frontal-code repl
```

The REPL builds its provider client at startup, so it needs credentials for the default provider. Without them it exits before any slash command runs. Use `frontal-code doctor` first when setting up. It works without credentials.

### status

Show runtime status: model, provider, sandbox, and session.

```bash
frontal-code status
frontal-code --output-format json status
```

`status --json` and `status --detailed` are not implemented. Use the global `--output-format json`.

### hosted

```bash
frontal-code hosted tasks list [--status STATUS[,STATUS...]] [--source SOURCE] [--repository REPO] [--channel-id ID] [--thread-ts TS] [--needs-followup] [--limit N]
frontal-code hosted task approval <TASK_ID> [retry|cancel|ack] [--kind orphaned_hosted_agent|github_review_followup] [--resolved-by NAME] [--reason TEXT]
```

**Examples:**

```bash
# List active Slack-created tasks
frontal-code hosted tasks list --status pending,running --source slack --limit 10

# List tasks that have GitHub review follow-up pending
frontal-code hosted tasks list --needs-followup

# Clear GitHub review follow-up and rerun the lane
frontal-code hosted task approval task_123 retry --kind github_review_followup --resolved-by reviewer
```

### config

Manage configuration.

```bash
frontal-code config [SUBCOMMAND] [OPTIONS]
```

**Subcommands:**
- `show`: Show current configuration
- `get <KEY>`: Get configuration value
- `set <KEY> <VALUE>`: Set configuration value
- `unset <KEY>`: Remove configuration value
- `reset`: Reset to defaults
- `validate`: Validate configuration
- `init`: Initialize configuration

**Examples:**
```bash
# Show all config
frontal-code config show

# Get specific value
frontal-code config get runtime.default_model

# Set value
frontal-code config set runtime.default_model "claude-sonnet-4-6"
frontal-code config set permission-mode safe-mode

# Unset value
frontal-code config unset api.timeout

# Reset configuration
frontal-code config reset

# Validate configuration
frontal-code config validate
```

### session

Manage sessions.

```bash
frontal-code session [SUBCOMMAND] [OPTIONS]
```

**Subcommands:**
- `list`: List available sessions
- `show <ID>`: Show session details
- `export <ID> --output <PATH>`: Export session
- `import <PATH>`: Import session
- `delete <ID>`: Delete session
- `cleanup`: Clean up old sessions

**Examples:**
```bash
# List sessions
frontal-code session list

# Show session details
frontal-code session show session-123

# Export session
frontal-code session export session-123 --output session.json

# Import session
frontal-code session import session.json

# Delete session
frontal-code session delete session-123

# Clean up old sessions
frontal-code session cleanup --older-than 7d
```

### plugin

Manage plugins.

```bash
frontal-code plugin [SUBCOMMAND] [OPTIONS]
```

**Subcommands:**
- `list`: List installed plugins
- `install <PATH|URL|NAME>`: Install plugin
- `uninstall <NAME>`: Uninstall plugin
- `enable <NAME>`: Enable plugin
- `disable <NAME>`: Disable plugin
- `update [NAME]`: Update plugin(s)
- `show <NAME>`: Show plugin details
- `validate <NAME>`: Validate plugin

**Examples:**
```bash
# List plugins
frontal-code plugin list

# Install plugin
frontal-code plugin install ./my-plugin
frontal-code plugin install https://github.com/user/plugin.git
frontal-code plugin install plugin-name

# Uninstall plugin
frontal-code plugin uninstall my-plugin

# Enable/disable plugin
frontal-code plugin enable my-plugin
frontal-code plugin disable my-plugin

# Update plugin
frontal-code plugin update my-plugin
frontal-code plugin update  # Update all

# Show plugin details
frontal-code plugin show my-plugin
```

### mcp

Manage MCP (Model Context Protocol) servers.

```bash
frontal-code mcp [SUBCOMMAND] [OPTIONS]
```

**Subcommands:**
- `list`: List MCP servers
- `start <NAME>`: Start MCP server
- `stop <NAME>`: Stop MCP server
- `restart <NAME>`: Restart MCP server
- `status [NAME]`: Show server status
- `config <NAME>`: Configure server
- `tools [NAME]`: List available tools
- `logs <NAME>`: Show server logs

**Examples:**
```bash
# List MCP servers
frontal-code mcp list

# Start server
frontal-code mcp start filesystem

# Stop server
frontal-code mcp stop filesystem

# Show status
frontal-code mcp status filesystem

# List tools
frontal-code mcp tools filesystem

# Configure server
frontal-code mcp config filesystem --timeout 60
```

### tools

Manage built-in tools.

```bash
frontal-code tools [SUBCOMMAND] [OPTIONS]
```

**Subcommands:**
- `list`: List available tools
- `show <NAME>`: Show tool details
- `test <NAME>`: Test tool
- `enable <NAME>`: Enable tool
- `disable <NAME>`: Disable tool

**Examples:**
```bash
# List tools
frontal-code tools list

# Show tool details
frontal-code tools show read
frontal-code tools show bash

# Test tool
frontal-code tools test read --arg path="/tmp/test"

# Enable/disable tool
frontal-code tools enable bash
frontal-code tools disable bash
```

### login / logout / auth

Sign in to a provider with OAuth instead of an API key. Login needs an OAuth
client in settings (see [OAuth Login](../CONFIGURATION.md#oauth-login)).

```bash
frontal-code login [anthropic|openai]    # default: anthropic
frontal-code logout [anthropic|openai]
frontal-code auth status                 # how each provider is authenticated
```

`login` opens your browser, waits for the redirect on the configured callback
port, and stores the tokens in `~/.frontal-code/credentials.json` (mode `0600`).
`auth status` lists env keys and saved logins per provider and never prints token
values. Environment API keys take precedence over a saved login.

### doctor

Run system diagnostics.

```bash
frontal-code doctor [OPTIONS]
```

**Options:**
- `--component <NAME>`: Check specific component
- `--fix`: Attempt to fix issues automatically
- `--detailed`: Show detailed diagnostic information

**Examples:**
```bash
# Run full diagnostics
frontal-code doctor

# Check specific component
frontal-code doctor --component api
frontal-code doctor --component config

# Auto-fix issues
frontal-code doctor --fix

# Detailed diagnostics
frontal-code doctor --detailed
```

### version

Show version information.

```bash
frontal-code version [OPTIONS]
```

**Options:**
- `--detailed`: Show detailed version information
- `--json`: Output in JSON format

**Examples:**
```bash
# Basic version
frontal-code version

# Detailed version
frontal-code version --detailed

# JSON output
frontal-code version --json
```

### help

Show help information.

```bash
frontal-code help [COMMAND]
```

**Examples:**
```bash
# General help
frontal-code help

# Command-specific help
frontal-code help prompt
frontal-code help repl
frontal-code help config
```

## Slash Commands

Slash commands are available in the REPL and provide quick access to common operations.

### System Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/help` | Show help information | `/help` |
| `/version` | Show version | `/version` |
| `/status` | Show system status | `/status` |
| `/doctor` | Run diagnostics | `/doctor` |
| `/exit` | Exit REPL | `/exit` |
| `/quit` | Exit REPL | `/quit` |

### Session Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/session` | Show session info | `/session` |
| `/resume <ID>` | Resume session | `/resume latest` |
| `/export` | Export session | `/export --output session.json` |
| `/clear` | Clear screen | `/clear` |
| `/history` | Show command history | `/history` |

### Configuration Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/config` | Show configuration | `/config` |
| `/config get <KEY>` | Get config value | `/config get runtime.default_model` |
| `/config set <KEY> <VALUE>` | Set config value | `/config set permission-mode safe-mode` |
| `/model <MODEL>` | Change model | `/model sonnet` |
| `/provider <PROVIDER>` | Change provider | `/provider openai` |

### Tool Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/tools` | List available tools | `/tools` |
| `/tools <NAME>` | Show tool details | `/tools read` |
| `/enable <TOOL>` | Enable tool | `/enable bash` |
| `/disable <TOOL>` | Disable tool | `/disable bash` |
| `/permissions` | Show permission mode | `/permissions` |

### Plugin Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/plugins` | List plugins | `/plugins` |
| `/plugin install <PATH>` | Install plugin | `/plugin install ./my-plugin` |
| `/plugin enable <NAME>` | Enable plugin | `/plugin enable my-plugin` |
| `/plugin disable <NAME>` | Disable plugin | `/plugin disable my-plugin` |

### MCP Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/mcp` | List MCP servers | `/mcp` |
| `/mcp start <NAME>` | Start server | `/mcp start filesystem` |
| `/mcp stop <NAME>` | Stop server | `/mcp stop filesystem` |
| `/mcp tools <NAME>` | List tools | `/mcp tools filesystem` |

### File System Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/pwd` | Show current directory | `/pwd` |
| `/cd <PATH>` | Change directory | `/cd /tmp` |
| `/ls [PATH]` | List directory | `/ls` |
| `/cat <FILE>` | Show file contents | `/cat README.md` |

### Git Commands

| Command | Description | Example |
|---------|-------------|---------|
| `/git status` | Git status | `/git status` |
| `/git diff` | Git diff | `/git diff` |
| `/git log` | Git log | `/git log` |
| `/git add <PATH>` | Git add | `/git add .` |
| `/git commit <MESSAGE>` | Git commit | `/git commit "Update docs"` |

## Model Aliases

Model aliases provide convenient shortcuts for full model names:

| Alias | Full Name | Description |
|-------|------------|-------------|
| `opus` | `claude-opus-5` | Most capable model, best for complex tasks |
| `sonnet` | `claude-sonnet-4-6` | Balanced model, good for most tasks |
| `haiku` | `claude-haiku-4-5` | Fastest model, good for simple tasks |

### Usage Examples

```bash
# Using aliases
frontal-code --model opus prompt "Complex analysis task"
frontal-code --model sonnet prompt "Moderate complexity task"
frontal-code --model haiku prompt "Simple task"

# In REPL
/model opus
/model sonnet
/model haiku
```

## Permission Modes

| Mode | Reads | Writes | Shell |
|------|-------|--------|-------|
| `read-only` | allowed | denied | denied |
| `workspace-write` | allowed | allowed inside the workspace | asks |
| `prompt` (default) | allowed | asks | asks |
| `danger-full-access` | allowed | allowed | allowed, no prompts |

Set it with `--permission-mode`, `RUSTY_CLAUDE_PERMISSION_MODE`, or `permissionMode` in `~/.frontal-code/settings.json`. Repository settings only apply after `frontal-code trust` (see [USAGE.md](../USAGE.md#trusting-a-repository)).

## Output Formats

### Text Output (Default)

Human-readable text output with formatting and colors.

```bash
frontal-code --output-format text prompt "What files are in this directory?"
```

### JSON Output

Machine-readable JSON output for automation and scripting.

```bash
frontal-code --output-format json prompt "Analyze this code"
```

**JSON Structure:**
```json
{
  "success": true,
  "response": "AI response text",
  "model": "claude-sonnet-4-6",
  "provider": "anthropic",
  "tokens_used": 150,
  "tools_used": ["read", "grep"],
  "timestamp": "2024-01-01T12:00:00Z",
  "session_id": "session-123",
  "request_id": "req-456"
}
```

### Streaming Output

Real-time streaming of AI responses.

```bash
frontal-code prompt --stream "Generate a long story"
```

## Exit Codes

| Code | Meaning | Description |
|------|---------|-------------|
| 0 | Success | Command completed successfully |
| 1 | General Error | Generic error occurred |
| 2 | Configuration Error | Invalid configuration |
| 3 | Authentication Error | API key or authentication failed |
| 4 | Network Error | Network connectivity issue |
| 5 | Permission Error | Permission denied |
| 6 | Tool Error | Tool execution failed |
| 7 | Session Error | Session-related error |
| 8 | Plugin Error | Plugin-related error |
| 9 | MCP Error | MCP server error |
| 10 | Resource Error | Insufficient resources |
| 11 | Timeout Error | Operation timed out |
| 12 | Interrupted | Operation was interrupted |
| 13 | Invalid Argument | Invalid command arguments |
| 14 | File Not Found | Requested file not found |
| 15 | Invalid Input | Invalid user input |

## Environment Variables

Each variable below was checked against the source (`cli/src/main.rs`, `crates/code-providers`, `crates/code-core`).

### Credentials and endpoints

| Variable | Description |
|----------|-------------|
| `FRONTAL_API_KEY` | Key for the default `frontal` provider, and for `--provider anthropic`. |
| `FRONTAL_BASE_URL` | Base URL for the Frontal gateway or an Anthropic-compatible proxy. |
| `OPENAI_API_KEY` | Key for `--provider openai`. Also works with OpenRouter (see [USAGE.md](../USAGE.md#openrouter)). |
| `OPENAI_BASE_URL` | Base URL for the OpenAI-compatible endpoint, for example `https://openrouter.ai/api/v1`. |
| `XAI_API_KEY`, `XAI_BASE_URL` | Key and base URL for `--provider xai`. |
| `OLLAMA_BASE_URL` | Ollama endpoint, default `http://localhost:11434`. |

### Behavior

| Variable | Description |
|----------|-------------|
| `FRONTAL_CODE_MAX_OUTPUT_TOKENS` | Caps output tokens per request. Overrides the per-model default. Use it when a gateway or prepaid balance cannot cover the model's output ceiling (HTTP 402). |
| `RUSTY_CLAUDE_PERMISSION_MODE` | Default permission mode. Legacy name, still read. |
| `FRONTAL_CODE_CONFIG_HOME` | Directory searched first for `project.json`. |
| `FRONTAL_CODE_HOME` | Second directory searched for `project.json`. |
| `FRONTAL_CODE_CONFIG_DIR` | Configuration directory. |
| `FRONTAL_CODE_TELEMETRY_PATH` | Telemetry JSONL output path. |
| `FRONTAL_CODE_CLI_BIN` | CLI binary used by `code-api` to run prompts. |

**Not implemented** (documented before, but not read by the code): `FRONTAL_CODE_DEFAULT_PROVIDER`, `FRONTAL_CODE_LOG_LEVEL`, `FRONTAL_CODE_DATA_DIR`, `FRONTAL_CODE_SESSION_DIR`, `RUST_LOG`, `FRONTAL_CODE_PERMISSION_MODE`. The default provider is set in `config/project.json` under `runtime.default_provider`.

## Configuration Files

### User Configuration

Location: `~/.frontal-code/config.json`

```json
{
  "runtime": {
    "default_provider": "frontal",
    "default_model": "claude-sonnet-4-6",
    "permission_mode": "safe-mode"
  },
  "ui": {
    "output_format": "text",
    "color_output": true
  }
}
```

### Project Configuration

Location: `.frontal-code/settings.json` (project root)

```json
{
  "runtime": {
    "default_model": "claude-opus-5",
    "permission_mode": "danger-full-access"
  },
  "tools": {
    "allowed_tools": ["read", "write", "grep"]
  }
}
```

## Advanced Usage

### Command Chaining

```bash
# Chain commands with &&
frontal-code prompt "Analyze code" && frontal-code tools list

# Use output of one command as input
frontal-code prompt "$(frontal-code config get runtime.default_model) model capabilities"
```

### Batch Operations

```bash
# Process multiple files
for file in *.md; do
  frontal-code prompt "Summarize $file" --context "$file"
done

# Batch with xargs
find . -name "*.rs" | xargs -I {} frontal-code prompt "Analyze {}" --context "{}"
```

### Automation Scripts

```bash
#!/bin/bash
# frontal-code-analyze.sh

set -e

echo "Starting Frontal Code analysis..."
frontal-code --output-format json prompt "Analyze codebase" > analysis.json

echo "Extracting results..."
jq '.response' analysis.json > response.txt

echo "Analysis complete!"
```

### Integration with Other Tools

```bash
# Use with jq for JSON processing
frontal-code --output-format json prompt "List files" | jq '.tools_used'

# Use with grep for filtering
frontal-code prompt "Generate report" | grep -E "(ERROR|WARNING)"

# Use with sed for transformation
frontal-code prompt "Generate config" | sed 's/development/production/'
```

This CLI reference provides comprehensive coverage of all Frontal Code CLI commands and options for effective usage.
