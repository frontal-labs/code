# Troubleshooting Guide

This guide covers common issues, debugging techniques, and solutions for problems you might encounter with the Frontal Code CLI.

## Getting Help

### Built-in Help

```bash
# General help
frontal-code --help

# Command-specific help
frontal-code help prompt
frontal-code help repl
frontal-code help status

# Slash command help
/help
/status
/doctor
```

### Diagnostic Tools

```bash
# System diagnostics
frontal-code doctor

# Health check
frontal-code health check

# Configuration validation
frontal-code config validate

# Performance diagnostics
frontal-code diagnose performance
```

## Common Issues

### Installation and Setup

#### Problem: Cargo build fails

**Symptoms:**
```
error: failed to compile `cli v0.1.0`
error: could not compile `cli`
```

**Solutions:**
```bash
# Update Rust toolchain
rustup update stable

# Clear cargo cache
cargo clean

# Rebuild with verbose output
cargo build --workspace --verbose

# Check for missing dependencies
cargo check --workspace
```

#### Problem: Command not found

**Symptoms:**
```
zsh: command not found: frontal-code
```

**Solutions:**
```bash
# Install with Homebrew
brew install --HEAD ./homebrew/Formula/code.rb

# Verify Homebrew's bin directory is on PATH
eval "$(brew shellenv)"

# Use cargo run directly
cargo run -p cli -- --help
```

#### Problem: Permission denied

**Symptoms:**
```
Permission denied: ~/.frontal-code/config.json
```

**Solutions:**
```bash
# Create frontal-code directory with proper permissions
mkdir -p ~/.frontal-code
chmod 700 ~/.frontal-code

# Fix file permissions
chmod 600 ~/.frontal-code/config.json
chmod 700 ~/.frontal-code/sessions

# Check ownership
ls -la ~/.frontal-code
```

### Authentication Issues

#### Problem: API key not found

**Symptoms:**
```
Error: FRONTAL_API_KEY not found
```

**Solutions:**
```bash
# Set environment variable
export FRONTAL_API_KEY="sk-ant-..."

# Add to shell profile
echo 'export FRONTAL_API_KEY="sk-ant-..."' >> ~/.zshrc

# Use config file
frontal-code config set providers.anthropic.api_key "sk-ant-..."

# Verify key is set
frontal-code auth validate anthropic
```

#### Problem: Invalid API key

**Symptoms:**
```
Error: Invalid API key
```

**Solutions:**
```bash
# Verify API key format
echo $FRONTAL_API_KEY | grep -E "^sk-ant-"

# Test API connectivity
frontal-code auth test anthropic

# Regenerate API key
# Visit https://console.anthropic.com/

# Check for typos
frontal-code config show providers.anthropic
```

#### Problem: Rate limited

**Symptoms:**
```
Error: Rate limit exceeded
```

**Solutions:**
```bash
# Check rate limits
frontal-code auth limits anthropic

# Wait and retry
sleep 60
frontal-code prompt "test message"

# Use different model
frontal-code --model claude-haiku-5-5 prompt "test"

# Configure rate limiting
frontal-code config set rate_limiting.requests_per_minute 30
```

### Network Issues

#### Problem: Connection timeout

**Symptoms:**
```
Error: Connection timeout
```

**Solutions:**
```bash
# Check network connectivity
ping api.anthropic.com

# Test with curl
curl -I https://api.anthropic.com

# Configure proxy
export HTTPS_PROXY=http://proxy.example.com:8080
export HTTP_PROXY=http://proxy.example.com:8080

# Increase timeout
frontal-code config set api.timeout 600

# Use different endpoint
frontal-code config set providers.anthropic.base_url "https://api.anthropic.com"
```

#### Problem: DNS resolution failed

**Symptoms:**
```
Error: DNS resolution failed
```

**Solutions:**
```bash
# Check DNS resolution
nslookup api.anthropic.com
dig api.anthropic.com

# Use different DNS server
export DNS_SERVERS="8.8.8.8,1.1.1.1"

# Flush DNS cache
sudo dscacheutil -flushcache

# Configure DNS in Frontal Code
frontal-code config set network.dns_servers "8.8.8.8,1.1.1.1"
```

### Performance Issues

#### Problem: Slow response times

**Symptoms:**
- Commands take >30 seconds to respond
- High CPU usage
- Memory consumption grows

**Solutions:**
```bash
# Check system resources
frontal-code resources monitor

# Optimize configuration
frontal-code config set runtime.cache_size "200MB"
frontal-code config set api.connection_pool.max_connections 5

# Use faster model
frontal-code --model haiku prompt "quick test"

# Enable caching
frontal-code config set caching.memory.enabled true

# Profile performance
frontal-code profile cpu --duration 30s
```

#### Problem: Memory leaks

**Symptoms:**
- Memory usage increases over time
- System becomes unresponsive
- Out of memory errors

**Solutions:**
```bash
# Monitor memory usage
frontal-code memory monitor

# Reduce cache sizes
frontal-code config set caching.memory.max_size "50MB"
frontal-code config set runtime.memory_limit "1GB"

# Enable garbage collection
frontal-code config set runtime.gc_interval "30s"

# Restart Frontal Code
pkill cli
frontal-code prompt "test"

# Memory profile
frontal-code profile memory --duration 60s
```

### Tool Issues

#### Problem: Tool execution failed

**Symptoms:**
```
Error: Tool 'bash' execution failed
```

**Solutions:**
```bash
# Check tool permissions
frontal-code config show permissions

# Test tool manually
frontal-code tool test bash --command "echo test"

# Check tool availability
frontal-code tools list

# Enable tool
frontal-code config set permissions.allowed_tools "bash,read,write"

# Debug tool execution
frontal-code debug tool bash --command "ls -la"
```

#### Problem: File access denied

**Symptoms:**
```
Error: Permission denied: /etc/hosts
```

**Solutions:**
```bash
# Check file permissions
ls -la /etc/hosts

# Use safe mode
frontal-code --permission-mode safe-mode prompt "read /etc/hosts"

# Configure allowed paths
frontal-code config set permissions.tool_restrictions.bash.allowed_paths "/tmp,./"

# Run with elevated privileges (caution)
sudo frontal-code prompt "read /etc/hosts"
```

### Session Issues

#### Problem: Session not found

**Symptoms:**
```
Error: Session 'session-123' not found
```

**Solutions:**
```bash
# List available sessions
frontal-code session list

# Resume latest session
frontal-code --resume latest

# Check session directory
ls -la ~/.frontal-code/sessions

# Create new session
frontal-code prompt "start new session"

# Export session
frontal-code session export --session session-123 --output session.json
```

#### Problem: Session corruption

**Symptoms:**
```
Error: Session file corrupted
```

**Solutions:**
```bash
# Validate session
frontal-code session validate --session session-123

# Repair session
frontal-code session repair --session session-123

# Clear corrupted sessions
frontal-code session clean --corrupted

# Start fresh session
frontal-code prompt "new session after corruption"
```

### Plugin Issues

#### Problem: Plugin fails to load

**Symptoms:**
```
Error: Plugin 'my-plugin' failed to load
```

**Solutions:**
```bash
# Check plugin status
frontal-code plugin list

# Validate plugin
frontal-code plugin validate my-plugin

# Check dependencies
frontal-code plugin dependencies my-plugin

# Reinstall plugin
frontal-code plugin uninstall my-plugin
frontal-code plugin install my-plugin

# Debug plugin loading
frontal-code debug plugin my-plugin
```

#### Problem: Plugin permission denied

**Symptoms:**
```
Error: Plugin permission denied
```

**Solutions:**
```bash
# Check plugin permissions
frontal-code plugin permissions my-plugin

# Grant required permissions
frontal-code plugin grant my-plugin network

# Configure plugin sandbox
frontal-code config set plugins.sandbox false

# Review plugin manifest
cat ~/.frontal-code/plugins/my-plugin/plugin.json
```

### MCP Issues

#### Problem: MCP server not running

**Symptoms:**
```
Error: MCP server 'filesystem' not running
```

**Solutions:**
```bash
# Check MCP server status
frontal-code mcp status filesystem

# Start MCP server
frontal-code mcp start filesystem

# Check server configuration
frontal-code mcp config show filesystem

# Debug server startup
frontal-code debug mcp filesystem

# Restart server
frontal-code mcp restart filesystem
```

#### Problem: MCP tools not available

**Symptoms:**
```
Error: Tool 'filesystem/read' not found
```

**Solutions:**
```bash
# List available MCP tools
frontal-code mcp tools

# Check server tools
frontal-code mcp tools filesystem

# Test server connection
frontal-code mcp test filesystem

# Reload server tools
frontal-code mcp reload filesystem

# Check server logs
frontal-code mcp logs filesystem
```

## Debugging Techniques

### Enable Debug Logging

```bash
# Set debug log level
export RUST_LOG=debug

# Enable specific module debugging
export RUST_LOG=frontal-code::cli=debug,frontal-code::runtime=info

# Run with debug output
RUST_LOG=debug frontal-code prompt "test message"

# Save debug logs to file
RUST_LOG=debug frontal-code prompt "test" 2>&1 | tee debug.log
```

### Verbose Mode

```bash
# Run with verbose output
frontal-code --verbose prompt "test"

# Extra verbose mode
frontal-code --verbose --verbose prompt "test"

# Show configuration
frontal-code config show --verbose
```

### Dry Run Mode

```bash
# Test command without execution
frontal-code --dry-run prompt "delete all files"

# Validate configuration
frontal-code config validate --dry-run

# Test plugin installation
frontal-code plugin install --dry-run my-plugin
```

### Step-by-Step Debugging

```bash
# Enable step-by-step mode
frontal-code --step-by-step prompt "complex task"

# Interactive debugging
frontal-code debug interactive

# Break on errors
frontal-code debug --break-on-error prompt "risky operation"
```

## Error Codes

### Common Error Codes

| Code | Description | Solution |
|------|-------------|----------|
| 1 | General error | Check logs for details |
| 2 | Configuration error | Validate config file |
| 3 | Authentication error | Check API keys |
| 4 | Network error | Check network connectivity |
| 5 | Permission error | Check file permissions |
| 6 | Tool execution error | Validate tool configuration |
| 7 | Session error | Check session files |
| 8 | Plugin error | Validate plugin installation |
| 9 | MCP error | Check MCP server status |
| 10 | Resource error | Check system resources |

### Error Details

```bash
# Show error details
frontal-code error show 12345

# Error lookup
frontal-code error lookup "permission denied"

# Error troubleshooting
frontal-code troubleshoot --error-code 4
```

## System Diagnostics

### Health Check

```bash
# Comprehensive health check
frontal-code health check --comprehensive

# Quick health check
frontal-code health check --quick

# Specific component check
frontal-code health check --component api
frontal-code health check --component tools
frontal-code health check --component mcp
```

### System Information

```bash
# Show system info
frontal-code system info

# Show configuration
frontal-code config show

# Show environment
frontal-code env show

# Show version info
frontal-code version --verbose
```

### Performance Diagnostics

```bash
# Performance check
frontal-code performance check

# Resource usage
frontal-code resources usage

# Bottleneck analysis
frontal-code analyze bottlenecks

# Optimization suggestions
frontal-code optimize suggest
```

## Getting Support

### Community Support

```bash
# Generate support bundle
frontal-code support bundle --output support-bundle.tar.gz

# Check for known issues
frontal-code issues search "connection timeout"

# Report issue
frontal-code issue report --type bug --description "Detailed description"
```

### Contact Support

```bash
# Generate diagnostic report
frontal-code diagnostics report --output diagnostics.json

# Export configuration
frontal-code config export --output config.json

# Export logs
frontal-code logs export --days 7 --output logs.tar.gz
```

## Recovery Procedures

### Configuration Recovery

```bash
# Reset configuration
frontal-code config reset

# Restore from backup
frontal-code config restore --backup config-backup.json

# Initialize default configuration
frontal-code config init --defaults

# Validate configuration
frontal-code config validate
```

### Session Recovery

```bash
# List corrupted sessions
frontal-code session list --corrupted

# Repair sessions
frontal-code session repair --all

# Export sessions
frontal-code session export --all

# Clear sessions
frontal-code session clear --all
```

### Plugin Recovery

```bash
# List broken plugins
frontal-code plugin list --broken

# Reinstall all plugins
frontal-code plugin reinstall --all

# Reset plugin registry
frontal-code plugin registry reset

# Validate plugins
frontal-code plugin validate --all
```

## Prevention Tips

### Regular Maintenance

```bash
# Clean up old sessions
frontal-code session cleanup --older-than 30d

# Clear cache
frontal-code cache clear --all

# Update plugins
frontal-code plugin update --all

# Check system health
frontal-code health check
```

### Monitoring

```bash
# Enable monitoring
frontal-code monitoring enable

# Set up alerts
frontal-code alerts enable --type error

# Performance monitoring
frontal-code performance monitor

# Resource monitoring
frontal-code resources monitor
```

### Backup Strategies

```bash
# Backup configuration
frontal-code config backup --output config-backup.json

# Backup sessions
frontal-code session backup --output sessions-backup.tar.gz

# Backup plugins
frontal-code plugin backup --output plugins-backup.tar.gz

# Automated backup
frontal-code backup schedule --daily --retain 7
```

This troubleshooting guide provides comprehensive coverage of common issues and solutions for using Frontal Code CLI effectively.

## Verified behavior and known issues (2026-11-08)

These were found by running the CLI, SDKs, HTTP API, and hosted server against a live model: first a free OpenRouter model, and from the later checks on, a local Ollama model (`qwen2.5:1.5b`). Each item says whether it is fixed or still open.

### Fixed

- **Hosted lanes simulated work.** The default lane transport was an in-memory simulator that never ran an agent. The simulator is now test-only, and the default is `local-docker`, which runs each task in a worker container. See [CONTAINERS.md](CONTAINERS.md#hosted-worker-containers).
- **Worker containers could not report back.** The CLI refused cleartext `http://host.docker.internal` for hosted server URLs. The exception now applies only inside worker containers, which the server marks with `FRONTAL_CODE_HOSTED_WORKER=1`.
- **Docker Desktop storage errors (`input/output error` on blob reads).** Restarting Docker Desktop cleared them. The ClickHouse tests run with `FRONTAL_TEST_CLICKHOUSE_URL` set, and pass on the restarted daemon.
- **Hosted Ollama tasks failed with `invalid or revoked worker token`.** The Ollama client sent a placeholder key, so the model proxy could not identify the task. The client now sends the task token in `OLLAMA_API_KEY`.
- **A worker's network stayed behind after its task finished.** It is removed when the worker exits. Each server port has its own relay container.
- **A model that returned an empty reply failed the whole task.** The runtime asks again, up to two times, first.
- **A plugin hook that ignored its input was reported as failed.** A broken pipe on stdin is no longer an error.
- **Tests that depend on `PATH` passed on macOS and failed on Linux.** Commands are looked up on `PATH` directly, not through a login shell.
- **`npm install` failed without a release binary.** The postinstall script now warns and exits 0.
- **The server started with a callback URL whose port did not match its listening port.** It now refuses to start and names the port it needs.

- **`HTTP 402 Payment Required` for an unknown model.** Models the CLI does not recognize default to a 64K output budget, which a prepaid balance may not cover. Set `FRONTAL_CODE_MAX_OUTPUT_TOKENS`.
- **`unknown option: --stream` (or any `--flag`) inside a prompt.** Flags placed where prompt text is expected are now rejected. Before, they were sent to the model as text.
- **`frontal-code repl` sent the word "repl" to the model.** It now opens the REPL.
- **`frontal-code plugins list` asked for credentials.** It now lists plugins.
- **Rust and TypeScript SDK turns failed.** They need `prompt -p ... --output-format json --stream --resume`, and the CLI now implements those flags. See [CLI_REFERENCES.md](CLI_REFERENCES.md#jsonl-events).
- **`changeset` refused to run.** Fixed the config, the baseline, and the missing changelog package. See `RELEASE.md`.
- **`.gitignore` got an orphan `# FrontalCode local artifacts` header.** `trust` and `init` now write only when an entry is missing.

### Open

- **The REPL needs credentials to start.** Without them it exits before any slash command runs, so `/doctor` cannot be reached. Use `frontal-code doctor` instead, which works without credentials.
- **`auth status` and `mcp list` can report missing credentials in an untrusted workspace.** Reproduce with `frontal-code trust --revoke`, then `frontal-code auth status` with no API keys set. Trusted workspaces do not show the problem. The root cause is not yet isolated.
- **`--image` is not supported.** No provider accepts image input yet. The SDKs' `local_image` input fails with a clear error.
- **Local embeddings are a hashing function, not a model.** `LocalMlEmbeddingProvider` (model id `local-hash`) hashes tokens into a fixed-size vector. It is deterministic and cheap, but it is not semantic. The name predates this audit and is kept for compatibility.
- **`lane_completion` is dormant.** `crates/code-tools/src/lane_completion.rs` is intentionally not wired into lane lifecycle.
- **A Postgres integration test in `extensions/code-slack` is skipped** because it needs a database.
- **SDK end-to-end suites skip silently** when `FRONTAL_CODE_E2E_BIN` is unset (and `OPENAI_API_KEY` for the `openai` provider). A skipped run reports as passing. Check the environment before reading a green result as proof.
- **A Linux test run can hang.** Under Rosetta, the `code-runtime` library tests once deadlocked on a futex, with the threads of two tests blocked and their `git` children unreaped. It reproduced in one of four full-suite runs and did not occur in serial runs or on macOS. The test that changes the working directory (`prompt::tests`, around `set_current_dir`) holds the env lock while the other tests run unlocked, and that is the first place to look. `scripts/linux-amd64-tests.sh` is bounded by `TEST_TIMEOUT` (default 5400 seconds), so a hang fails the run instead of blocking it.
- **Tracked files disappeared from the working tree during Linux runs.** `docs/design/` was deleted while containers that mounted the repository were running the test suite. The cause was not isolated. The Linux scripts now mount a copy of the source (`~/.frontal-code-dev/src-snapshot`), so a test can only damage the copy. The deleted files were restored from git.
- **Native Linux Docker hosts are not verified.** The Linux checks ran on `linux/amd64` under Rosetta, with Docker Desktop as the daemon. On a native Linux host, `host-gateway` resolves to the real host bridge, so the relay and callback address need a run there (`scripts/linux-amd64-worker-e2e.sh` is the starting point).
- **Cloud OpenAI-compatible gateways are not verified in this round.** The `openai` provider path has been checked against Ollama's compatible endpoint and against an earlier free OpenRouter model. Nothing has been run against a current paid gateway.
- **A small local model sometimes narrates a command instead of running it.** `qwen2.5:1.5b` does this with the `curl` and `rm` steps in the worker E2E prompt. The E2E checks that tasks finish and publish, not each command's exit code. Use a larger model when you need that check.
- **Another `cargo` process can clobber a shared `target/` directory.** A second build in the same repository wrote partial fingerprints and deleted others while tests ran. Give each concurrent build its own `CARGO_TARGET_DIR`.
