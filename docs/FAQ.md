# Frequently Asked Questions

This document answers common questions about the Frontal Code CLI.

## General Questions

### What is Frontal Code?

Frontal Code is a high-performance Rust rewrite of the Frontal Code CLI agent harness. It provides a command-line interface for interacting with AI models, with built-in tools for file operations, web access, and automation.

### What can I do with Frontal Code?

- Interact with AI models (Anthropic, OpenAI, xAI)
- Execute shell commands and scripts
- Read, write, and edit files
- Search and analyze codebases
- Browse the web and fetch content
- Manage plugins and extensions
- Use MCP (Model Context Protocol) servers
- Automate workflows and tasks

### Is Frontal Code free?

Frontal Code is open-source and free to use. However, you'll need API keys from AI providers (Anthropic, OpenAI, etc.) which may have associated costs.

## Installation and Setup

### How do I install Frontal Code?

```bash
# Install with Homebrew from this repo
git clone <repository-url>
cd claw-code-main
brew install --HEAD ./homebrew/frontal-code.rb
frontal-code --help

# Or build from source for development
cargo build --workspace
cargo run -p cli -- --help
```

### What are the system requirements?

- Rust 1.70+ (for building from source)
- 4GB+ RAM recommended
- 1GB+ disk space
- Internet connection for AI API access
- Supported OS: Linux, macOS, Windows

### How do I update Frontal Code?

```bash
# If installed with Homebrew
brew upgrade --fetch-HEAD frontal-code

# If built from source
git pull origin main
cargo build --workspace
```

## Configuration

### How do I set up API keys?

```bash
# Environment variables (recommended)
export FCODE_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export XAI_API_KEY="xai-..."

# Or in config file
frontal-code config set providers.anthropic.api_key "sk-ant-..."
```

### Where is the configuration file?

The main configuration file is located at:
- `~/.frontal-code/config.json` (user config)
- `.frontal-code/settings.json` (project config)

### How do I change the default model?

```bash
# Via command line
frontal-code --model claude-sonnet-4-6

# Via config
frontal-code config set runtime.default_model "claude-sonnet-4-6"

# Use model aliases
frontal-code --model opus  # claude-opus-5
frontal-code --model sonnet # claude-sonnet-4-6
frontal-code --model haiku  # claude-haiku-4-5
```

### Can I use different AI providers?

Yes! Frontal Code supports multiple providers:

```bash
# Anthropic (default)
frontal-code --provider anthropic

# OpenAI
frontal-code --provider openai --model gpt-4

# xAI
frontal-code --provider xai --model grok-3

# Frontal (API gateway)
frontal-code --provider frontal
```

## Usage

### How do I start an interactive session?

```bash
# Start REPL
frontal-code repl

# With specific model
frontal-code --model sonnet repl

# Resume previous session
frontal-code --resume latest repl
```

### What are the permission modes?

- **danger-full-access**: All tools allowed without confirmation
- **safe-mode**: Only safe tools allowed, destructive tools need approval
- **ask-permissions**: Prompt for approval on every tool use

```bash
frontal-code --permission-mode safe-mode prompt "analyze this code"
```

### How do I use tools?

Tools are automatically available in prompts:

```bash
# File operations
frontal-code prompt "Read the README.md file and summarize it"

# Shell commands
frontal-code prompt "Run 'ls -la' and show the output"

# Web access
frontal-code prompt "Search for 'Rust programming' and summarize the results"
```

### What tools are available?

Built-in tools include:
- `bash` - Execute shell commands
- `read` - Read file contents
- `write` - Write/create files
- `edit` - Edit existing files
- `grep` - Search file contents
- `glob` - Search file patterns
- `web_search` - Search the web
- `web_fetch` - Fetch web content
- `agent` - Launch sub-agents

## Sessions

### How do sessions work?

Frontal Code automatically saves your conversation history:

```bash
# List sessions
frontal-code session list

# Resume session
frontal-code --resume session-123.jsonl

# Export session
frontal-code session export --output session.json
```

### Where are sessions stored?

Sessions are stored in `~/.frontal-code/sessions/` by default.

### Can I disable session saving?

```bash
frontal-code config set session.auto_save false
```

## Plugins

### What are plugins?

Plugins extend Frontal Code's functionality with additional tools, commands, and providers.

### How do I install plugins?

```bash
# From local directory
frontal-code plugin install /path/to/plugin

# From Git repository
frontal-code plugin install https://github.com/user/plugin.git

# From registry
frontal-code plugin install plugin-name
```

### Where can I find plugins?

- Official plugin registry: https://github.com/frontal-labs/frontal-code
- Community plugins on GitHub
- Built-in plugins included with Frontal Code

## MCP (Model Context Protocol)

### What is MCP?

MCP is a protocol for connecting AI models to external tools and data sources.

### How do I use MCP servers?

```bash
# List available servers
frontal-code mcp list

# Start a server
frontal-code mcp start filesystem

# Use MCP tools
frontal-code prompt "Use filesystem to read /tmp/test.txt"
```

### What MCP servers are available?

Built-in servers include:
- `filesystem` - Enhanced file operations
- `github` - GitHub integration
- `postgres` - PostgreSQL database
- `slack` - Slack integration

## Performance

### Why is Frontal Code slow?

Common causes:
- Network latency to AI providers
- Large file operations
- Insufficient system resources
- Model response time

Solutions:
```bash
# Use faster model
frontal-code --model haiku prompt "quick task"

# Enable caching
frontal-code config set caching.memory.enabled true

# Optimize configuration
frontal-code optimize suggest
```

### How can I improve performance?

- Use appropriate models for tasks
- Enable caching
- Optimize tool usage
- Monitor system resources
- Use parallel processing when possible

## Troubleshooting

### Why do I get "API key not found"?

Check your API key configuration:

```bash
# Verify environment variable
echo $FCODE_API_KEY

# Test API connectivity
frontal-code auth test anthropic

# Check config
frontal-code config show providers.anthropic
```

### Why do I get permission denied errors?

Check file permissions and configuration:

```bash
# Check file permissions
ls -la ~/.frontal-code/

# Fix permissions
chmod 700 ~/.frontal-code
chmod 600 ~/.frontal-code/config.json

# Use safe mode
frontal-code --permission-mode safe-mode
```

### How do I debug issues?

```bash
# Enable debug logging
RUST_LOG=debug frontal-code prompt "test"

# Run diagnostics
frontal-code doctor

# Check system health
frontal-code health check
```

## Security

### Is Frontal Code secure?

Frontal Code implements multiple security layers:
- Permission system for tool access
- Sandboxing for isolated execution
- Secure API key management
- Audit logging

### How do I secure my API keys?

- Use environment variables (recommended)
- Don't commit API keys to version control
- Rotate keys regularly
- Use different keys for different environments

### Can Frontal Code access my files?

Frontal Code can only access files based on your permission settings:
- **Safe mode**: Only read operations
- **Ask permissions**: Prompts for file access
- **Danger mode**: Full file access

## Development

### How do I contribute to Frontal Code?

1. Fork the repository
2. Create a feature branch
3. Make changes and add tests
4. Run tests: `cargo test --workspace`
5. Submit a pull request

### How do I build Frontal Code from source?

```bash
git clone <repository-url>
cd claw-code-main
cargo build --workspace
```

### How do I run tests?

```bash
# Run all tests
cargo test --workspace

# Run specific test
cargo test -p cli test_name

# Run with output
cargo test --workspace -- --nocapture
```

## Comparison

### How does Frontal Code compare to other AI CLI tools?

Frontal Code offers:
- Rust-based performance and safety
- Multiple AI provider support
- Extensible plugin system
- MCP integration
- Comprehensive tool ecosystem
- Session persistence
- Advanced permission system

### Why choose Frontal Code over alternatives?

- **Performance**: Rust implementation for speed
- **Flexibility**: Multiple providers and plugins
- **Security**: Advanced permission system
- **Extensibility**: Plugin and MCP support
- **Usability**: Rich tool ecosystem

## Licensing

### What license does Frontal Code use?

Frontal Code is licensed under the MIT license. See the LICENSE file for details.

### Can I use Frontal Code commercially?

Yes, the MIT license permits commercial use. However, you'll need to comply with the terms of service of your chosen AI providers.

## Support

### How do I get help?

```bash
# Built-in help
frontal-code --help
frontal-code help <command>

# Diagnostics
frontal-code doctor

# Community support
# GitHub Issues: https://github.com/frontal-labs/frontal-code/issues
```

### How do I report bugs?

1. Check existing issues
2. Create a new issue with:
   - Detailed description
   - Steps to reproduce
   - System information
   - Error logs

### Where can I ask questions?

- GitHub Discussions
- Community forums
- Documentation
- Slack/Discord community (if available)

## Advanced Topics

### Can I use Frontal Code in CI/CD?

Yes! Frontal Code is designed for automation:

```bash
# JSON output for automation
frontal-code --output-format json prompt "analyze code" > results.json

# Non-interactive mode
frontal-code --permission-mode safe-mode prompt "run tests"
```

### How do I integrate Frontal Code with other tools?

Frontal Code provides:
- JSON output format
- API for programmatic access
- Plugin system for custom integrations
- MCP for external tool connections

### Can I customize Frontal Code?

Yes! Customization options include:
- Configuration files
- Custom plugins
- MCP servers
- Model aliases
- Permission rules

## Future Plans

### What's coming in future versions?

- Enhanced plugin ecosystem
- More AI providers
- Advanced automation features
- Improved performance
- Additional MCP servers
- Web interface

### How can I request features?

1. Check existing feature requests
2. Create a new feature request
3. Provide detailed requirements
4. Participate in discussions

## Miscellaneous

### What does "Frontal Code" mean?

Frontal Code refers to the concept of agents frontal-codeing around tasks and tools, providing a comprehensive AI-powered development environment.

### Who maintains Frontal Code?

Frontal Code is maintained by the Frontal Code team and community contributors.

### How can I stay updated?

- Follow the GitHub repository
- Subscribe to releases
- Join the community
- Read the changelog

---

Still have questions? Check the [documentation](./README.md) or [open an issue](https://github.com/frontal-labs/frontal-code/issues).
