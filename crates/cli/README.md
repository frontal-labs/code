# Frontal Code CLI

Main command-line interface for the Frontal Code ecosystem, providing comprehensive AI-powered development workflows.

## Overview

The Frontal Code CLI is the primary user interface for interacting with the Frontal Code AI assistant system. It provides interactive REPL mode, one-shot command execution, comprehensive tool integration, and multi-provider support for AI-powered development workflows.

## Features

- **Interactive REPL**: Full-featured REPL with readline support, tab completion, and command history
- **One-shot Commands**: Direct command execution for automation and scripting
- **Multi-Provider Support**: Anthropic, OpenAI, xAI, Frontal, Bedrock, Azure, and Ollama
- **Rich Output Rendering**: Markdown rendering, syntax highlighting, and ANSI formatting
- **Plugin System**: Extensible architecture with plugin management and installation
- **Permission Management**: Configurable permission modes for security and safety
- **Session Persistence**: Save and resume conversations across sessions
- **Slash Commands**: 50+ built-in commands for system management and automation
- **Tool Integration**: Comprehensive tool system for file operations, web access, and more
- **Compatibility Testing**: Built-in testing and validation tools
- **JSON Output**: Machine-readable output for automation and integration

## Key Components

- **Interactive Mode**: Full-featured REPL with command history, completion, and slash commands
- **Command Processing**: Unified command parsing, argument handling, and execution
- **Output Rendering**: Rich formatting for AI responses, code, and structured data
- **Provider Management**: Multi-provider routing, authentication, and streaming
- **Plugin Integration**: Extensible architecture with install/enable/disable workflows
- **Session Management**: Persistence, resumption, and state management
- **Permission System**: Configurable access controls and safety policies
- **Tool Execution**: Built-in tools for file operations, web access, and system integration

## Dependencies

- `frontal-code-api` for AI provider integration and HTTP services
- `frontal-code-commands` for slash command system and registry
- `frontal-code-compat-harness` for testing and compatibility validation
- `frontal-code-runtime` for core functionality and session management
- `frontal-code-plugins` for extensibility and plugin management
- `frontal-code-tools` for tool integration and execution
- `frontal-code-providers` for multi-provider AI client support
- `crossterm` for terminal handling and cross-platform support
- `rustyline` for readline functionality and completion
- `pulldown-cmark` for markdown parsing and rendering
- `syntect` for syntax highlighting and code formatting

## Usage

The main binary is named `frontal-code` and can be used in multiple ways:

### Interactive Mode
```bash
# Start interactive REPL
frontal-code

# Start with specific model
frontal-code --model claude-opus-5

# Start with specific permissions
frontal-code --permission-mode workspace-write
```

### One-shot Commands
```bash
# Simple prompt
frontal-code prompt "explain this codebase"

# With specific provider
frontal-code --provider anthropic prompt "your question"
frontal-code --provider openai prompt "your question"
frontal-code --provider xai prompt "your question"

# With JSON output for automation
frontal-code --output-format json prompt "summarize crates/cli/src/main.rs"

# With specific permissions
frontal-code --permission-mode read-only prompt "analyze this file"
```

### Direct Subcommands
```bash
# Check system status
frontal-code status

# List available agents
frontal-code agents

# Check MCP servers
frontal-code mcp

# Run system diagnostics
frontal-code doctor

# Show sandbox information
frontal-code sandbox
```

### Provider Selection
```bash
# Force specific provider
frontal-code --provider anthropic prompt "your question"
frontal-code --provider openai prompt "your question"
frontal-code --provider xai prompt "your question"

# With model aliases
frontal-code --provider anthropic --model opus prompt "complex task"
frontal-code --provider openai --model gpt-4 prompt "your question"
```

### Session Management
```bash
# Resume latest session
frontal-code --resume latest

# Resume specific session
frontal-code --resume session-123

# Resume and run command
frontal-code --resume latest /status
```

## Configuration

The CLI supports multiple configuration methods:

### Environment Variables
```bash
export FRONTAL_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export XAI_API_KEY="xai-..."
export FRONTAL_BASE_URL="https://ai.frontal.dev/v1"
```

### Configuration Files
- `~/.frontal-code/settings.json` - Global user configuration
- `~/.config/frontal-code/settings.json` - System configuration
- `.frontal-code/settings.json` - Workspace configuration
- `.frontal-code/settings.json` - Workspace settings
- `.frontal-code/settings.local.json` - Local workspace overrides

## Slash Commands

The REPL provides 50+ slash commands organized by category:

### Session Management
- `/help`, `/status`, `/sandbox`, `/cost`, `/resume`, `/session`, `/version`, `/usage`, `/stats`

### Workspace & Git
- `/compact`, `/clear`, `/config`, `/memory`, `/init`, `/diff`, `/commit`, `/pr`, `/issue`, `/export`, `/hooks`, `/files`, `/branch`, `/release-notes`, `/add-dir`

### Discovery & Debugging
- `/mcp`, `/agents`, `/skills`, `/doctor`, `/tasks`, `/context`, `/desktop`, `/ide`

### Automation & Analysis
- `/review`, `/advisor`, `/insights`, `/security-review`, `/subagent`, `/team`, `/telemetry`, `/providers`, `/cron`

### Plugin Management
- `/plugin`, `/plugins`, `/marketplace` - Install, enable, disable, update plugins

## Permission Modes

- `read-only` - Safe mode with read access only
- `workspace-write` - Write access within workspace bounds
- `danger-full-access` - Full system access (use with caution)

## Model Aliases

- `opus` - `claude-opus-5`
- `sonnet` - `claude-sonnet-4-6`
- `haiku` - `claude-haiku-4-5`

## Development

For development from source:

```bash
# Build the workspace
cargo build --workspace

# Run the CLI
cargo run -p cli -- [args]

# Run tests
cargo test -p cli
```

## Integration

The CLI integrates with:
- `frontal-code-runtime` for session management and core functionality
- `frontal-code-providers` for multi-provider AI client support
- `frontal-code-tools` for comprehensive tool integration
- `frontal-code-plugins` for extensibility and customization
- `frontal-code-memory` for semantic memory and context management
