# Configuration Guide

This guide covers all configuration options for the Frontal Code CLI, including environment variables, config files, and runtime settings.

## Configuration Precedence

Settings are applied in the following order (highest to lowest priority):

1. Command-line flags
2. Environment variables
3. Project config file (`.frontal-code/settings.json`)
4. User config file (`~/.frontal-code/config.json`)
5. Default values

## Environment Variables

### Required Variables

```bash
# Anthropic API (primary provider)
export FRONTAL_API_KEY="sk-ant-..."
export FRONTAL_BASE_URL="https://api.anthropic.com"  # optional

# OpenAI-compatible API
export OPENAI_API_KEY="sk-..."
export OPENAI_BASE_URL="https://api.openai.com/v1"  # optional

# xAI API
export XAI_API_KEY="xai-..."
export XAI_BASE_URL="https://api.x.ai/v1"  # optional

# Frontal API gateway
export FRONTAL_API_KEY="frontal-..."
export FRONTAL_BASE_URL="https://ai.frontal.dev/v1"
```

### Optional Variables

```bash
# General settings
export FCODE_LOG_LEVEL="info"  # debug, info, warn, error
export FCODE_CONFIG_DIR="$HOME/.frontal-code"
export FCODE_DATA_DIR="$HOME/.frontal-code/data"

# Provider selection
export FCODE_DEFAULT_PROVIDER="anthropic"  # anthropic, openai, xai
export FCODE_DEFAULT_MODEL="claude-opus-5"

# Permission settings
export FCODE_PERMISSION_MODE="danger-full-access"  # danger-full-access, safe-mode, ask-permissions
export FCODE_ALLOWED_TOOLS="bash,read,write,edit,grep"

# Session settings
export FCODE_SESSION_DIR="$HOME/.frontal-code/sessions"
export FCODE_AUTO_SAVE_SESSIONS="true"
export FCODE_MAX_SESSIONS="100"

# MCP settings
export FCODE_MCP_SERVERS_DIR="$HOME/.frontal-code/mcp-servers"
export FCODE_MCP_TIMEOUT="30"
```

## Config File Format

The `.frontal-code/settings.json` config file uses JSON format with the following structure:

```json
{
  "version": "1.0",
  "providers": {
    "anthropic": {
       "api_key": "${FRONTAL_API_KEY}",
      "base_url": "https://api.anthropic.com",
      "default_model": "claude-opus-5"
    },
    "openai": {
      "api_key": "${OPENAI_API_KEY}",
      "base_url": "https://api.openai.com/v1",
      "default_model": "gpt-5"
    },
    "xai": {
      "api_key": "${XAI_API_KEY}",
      "base_url": "https://api.x.ai/v1",
      "default_model": "grok-3"
    }
  },
  "runtime": {
    "default_provider": "frontal",
    "default_model": "claude-opus-5",
    "permission_mode": "danger-full-access",
    "allowed_tools": ["bash", "read", "write", "edit", "grep", "glob", "web_search", "web_fetch"],
    "max_tokens": 4096,
    "temperature": 0.7,
    "timeout": 300
  },
  "session": {
    "auto_save": true,
    "max_sessions": 100,
    "session_dir": "${FCODE_SESSION_DIR}",
    "resume_last_session": false
  },
  "mcp": {
    "servers_dir": "${FCODE_MCP_SERVERS_DIR}",
    "timeout": 30,
    "auto_start": [],
    "enabled": true
  },
  "plugins": {
    "plugins_dir": "${FCODE_CONFIG_DIR}/plugins",
    "auto_load": [],
    "enabled": true
  },
  "ui": {
    "output_format": "text",  # text, json
    "color_output": true,
    "show_thinking": false,
    "show_tool_calls": true,
    "stream_output": true
  },
  "telemetry": {
    "enabled": false,
    "endpoint": "",
    "sample_rate": 0.1
  }
}
```

## Command-Line Flags

### Global Flags

```bash
frontal-code [OPTIONS] [COMMAND]

Options:
  -m, --model <MODEL>                 AI model to use
  -p, --provider <PROVIDER>           AI provider (anthropic, openai, xai)
  -o, --output-format <FORMAT>        Output format [text|json]
  -P, --permission-mode <MODE>        Permission mode
      --dangerously-skip-permissions  Skip all permission checks
      --allowed-tools <TOOLS>         Comma-separated list of allowed tools
      --resume <SESSION>              Resume session
      --config <FILE>                 Config file path
      --version, -V                   Show version
      --help, -h                      Show help
```

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

# OpenAI models
frontal-code --provider openai --model gpt-4
frontal-code --provider openai --model gpt-4-turbo

# xAI models
frontal-code --provider xai --model grok-3
```

## Permission Modes

### danger-full-access
- All tools are allowed without confirmation
- Recommended for trusted environments and automation
- Default mode for CLI usage

### safe-mode
- Only safe tools are allowed (read, grep, web_search, web_fetch)
- Destructive tools require explicit approval
- Recommended for untrusted codebases

### ask-permissions
- Prompt for approval on every tool use
- Most secure but interactive
- Recommended for learning and debugging

## Tool Configuration

### Built-in Tools

| Tool | Description | Safe Mode | Permissions |
|------|-------------|-----------|-------------|
| bash | Execute shell commands | No | Full system access |
| read | Read file contents | Yes | File read access |
| write | Write/create files | No | File write access |
| edit | Edit existing files | No | File write access |
| grep | Search file contents | Yes | File read access |
| glob | Search file patterns | Yes | File read access |
| web_search | Search the web | Yes | Web access |
| web_fetch | Fetch web content | Yes | Web access |
| agent | Launch sub-agents | No | Full access |

### Tool Restrictions

```bash
# Allow specific tools only
frontal-code --allowed-tools "read,grep,web_search"

# Disable dangerous tools
frontal-code --allowed-tools "read,write,edit,grep,glob,web_search,web_fetch"

# Custom tool restrictions in config
{
  "runtime": {
    "allowed_tools": ["read", "grep", "web_search"],
    "tool_restrictions": {
      "bash": {
        "allowed_commands": ["ls", "cat", "grep"],
        "blocked_commands": ["rm", "sudo", "chmod"]
      }
    }
  }
}
```

## Session Configuration

### Session Persistence

```bash
# Enable session persistence
frontal-code --session auto-save

# Resume last session
frontal-code --resume latest

# Resume specific session
frontal-code --resume session-123.jsonl

# Export session
frontal-code session export --format json --output session.json
```

### Session Settings

```json
{
  "session": {
    "auto_save": true,
    "max_sessions": 100,
    "session_dir": "~/.frontal-code/sessions",
    "compression": "gzip",
    "encryption": false,
    "metadata": {
      "save_system_info": true,
      "save_environment": false,
      "save_git_state": true
    }
  }
}
```

## MCP Configuration

### Server Configuration

```json
{
  "mcp": {
    "servers": {
      "filesystem": {
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/files"],
        "env": {
          "NODE_ENV": "production"
        },
        "timeout": 30,
        "auto_start": true
      },
      "github": {
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-github"],
        "env": {
          "GITHUB_PERSONAL_ACCESS_TOKEN": "${GITHUB_TOKEN}"
        },
        "timeout": 60,
        "auto_start": false
      }
    }
  }
}
```

### MCP Settings

```bash
# List available MCP servers
frontal-code mcp list

# Start specific server
frontal-code mcp start filesystem

# Configure MCP server
frontal-code mcp config filesystem --timeout 60 --auto-start
```

## Plugin Configuration

### Plugin Discovery

```json
{
  "plugins": {
    "plugins_dir": "~/.frontal-code/plugins",
    "auto_load": ["plugin-name"],
    "registry_url": "https://github.com/frontal-labs/frontal-code",
    "update_check_interval": "24h",
    "trusted_sources": ["https://github.com", "https://github.com/frontal-labs/frontal-code"]
  }
}
```

### Plugin Settings

```bash
# Install plugin
frontal-code plugin install /path/to/plugin

# Enable/disable plugin
frontal-code plugin enable plugin-name
frontal-code plugin disable plugin-name

# List plugins
frontal-code plugin list

# Update plugin
frontal-code plugin update plugin-name
```

## UI Configuration

### Output Formatting

```json
{
  "ui": {
    "output_format": "text",
    "color_output": true,
    "show_thinking": false,
    "show_tool_calls": true,
    "stream_output": true,
    "terminal_width": 80,
    "markdown_rendering": true,
    "syntax_highlighting": true
  }
}
```

### Display Options

```bash
# JSON output
frontal-code --output-format json prompt "summarize this file"

# Disable colors
frontal-code --color=false prompt "explain this"

# Show tool calls
frontal-code --show-tool-calls prompt "list files"
```

## Telemetry Configuration

### Usage Tracking

```json
{
  "telemetry": {
    "enabled": false,
    "endpoint": "https://telemetry.frontal-code.ai/v1/events",
    "sample_rate": 0.1,
    "batch_size": 10,
    "flush_interval": "60s",
    "events": [
      "session_start",
      "session_end",
      "tool_use",
      "error",
      "command_completion"
    ]
  }
}
```

### Privacy Settings

```bash
# Disable telemetry
export FCODE_TELEMETRY_ENABLED=false

# Set sample rate
export FCODE_TELEMETRY_SAMPLE_RATE=0.1

# Custom endpoint
export FCODE_TELEMETRY_ENDPOINT="https://my-telemetry.example.com"
```

## Advanced Configuration

### Custom Prompts

```json
{
  "prompts": {
    "system": "You are Frontal Code, a helpful AI assistant...",
    "user_context": "Current working directory: {cwd}\nGit branch: {branch}",
    "tool_use_template": "Using tool: {tool} with args: {args}"
  }
}
```

### Performance Tuning

```json
{
  "performance": {
    "max_concurrent_requests": 5,
    "request_timeout": 300,
    "retry_attempts": 3,
    "retry_delay": "1s",
    "cache_size": "100MB",
    "compression": true
  }
}
```

### Security Settings

```json
{
  "security": {
    "encrypt_sessions": false,
    "encrypt_config": false,
    "api_key_rotation": false,
    "audit_logging": false,
    "sandbox_mode": false
  }
}
```

## Configuration Validation

### Check Configuration

```bash
# Validate current configuration
frontal-code config validate

# Show effective configuration
frontal-code config show

# Show specific section
frontal-code config show providers
frontal-code config show runtime
```

### Common Issues

1. **API key not found**: Ensure environment variables are set
2. **Config file not found**: Check file path and permissions
3. **Invalid JSON**: Validate JSON syntax
4. **Permission denied**: Check file permissions for config directory

## Migration Guide

### From Environment Variables

If you're currently using only environment variables, you can migrate to a config file:

```bash
# Generate config from current environment
frontal-code config init --from-env

# This creates .frontal-code/settings.json with current settings
```

### Version Upgrades

When upgrading Frontal Code versions:

1. Backup current config: `cp .frontal-code/settings.json .frontal-code/settings.json.backup`
2. Run config validation: `frontal-code config validate`
3. Update deprecated settings as needed
4. Test with `--dry-run` flag before applying changes
