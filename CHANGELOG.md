# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Project policy: a `policy` object in `.frontal-code/settings.json` (or the user or local settings) limits `allowedProviders`, `allowedModels` (with `*` globs), per-provider `authMethods`, a `permissionCeiling`, and `deny` tools. Each source only adds restrictions. Enforced by the CLI on client construction and permission setup, and by the hosted server before a worker starts. See CONFIGURATION.md#project-policy.
- `code login [anthropic|openai]`, `logout`, and `auth status`: OAuth (authorization code + PKCE) sign-in as an alternative to API keys. Providers are configured per name under `settings.oauth`, and no vendor client ID is bundled.
- OpenAI accepts a saved OAuth login in place of `OPENAI_API_KEY`, and the Anthropic provider uses saved logins as well as env keys.
- Codex-family models (for example `gpt-5-codex`) on the OpenAI provider now go through the Responses API (`/responses`), since they are not served by chat completions. Requests are sent with `store: false`. A Codex model selects the OpenAI provider without `--provider`, even when the default provider is `frontal`.
- Biome lint now covers the JS and TS packages without errors. Markdown is excluded from the Biome check, because Biome's Markdown formatter rewrites code samples.
- Concurrent refreshes of the same saved login are serialized with a lock on `credentials.json.lock`, and the credentials are re-read after the lock is taken, so a refresh token is not used twice.
- The Slack bot (`extensions/code-slack`) is now an OpenChannel Protocol host built on `ocp-sdk` 2.2.1. Slack messages, `/code`, mentions, and approval clicks become OCP events. Task creation and approval resolution run as policy-checked OCP actions. Every event is stored in Postgres for audit, so Slack redeliveries are ignored even across restarts. **Breaking:** the bot now requires `OCP_DATABASE_URL` (Postgres) and Node.js 22.18+. Its entrypoint is `dist/main.js`, plus `dist/migrate.js` for deploy-time migrations. It serves a readiness endpoint at `/ready`. See `extensions/code-slack/docs/OCP.md`.

### Security
- `code-api` rejects requests whose `Host` is not `localhost`, a loopback IP, or the bound address, and requests with a cross-origin `Origin`, to block DNS-rebinding attacks. `POST /v1/cli/run` now returns `403` unless `FRONTAL_API_KEY` is set. Callers that used `/v1/cli/run` without a key must set one.
- The hosted server URL (`FRONTAL_CODE_SERVER_URL`) must use `https`/`wss` unless the host is `localhost`, `127.0.0.1`, or `[::1]`. Cleartext `http`/`ws` to other hosts is refused, so the API key is not sent in the clear.
- WebFetch refuses loopback, link-local (including `169.254.169.254`), RFC 1918 private, unique-local, unspecified, multicast, and CGNAT addresses. The check applies to the initial URL, to every redirect hop, and to the addresses each hostname resolves to at connect time.

### Changed
- Docs: `FRONTAL_API_KEY` is sent as a bearer token to the Frontal gateway by default. Anthropic and Anthropic-compatible proxies need `--provider anthropic`.
- `settings.oauth` accepts a map keyed by provider. The legacy single-object form still works and is read as `anthropic`.

### Security
- Project and local settings can no longer widen access. Their `permissionMode` cannot raise the mode the user set (the lower value wins), and their `permissions.allow` rules are ignored with a warning. Allow rules belong in user settings. Repositories that relied on project-level `allow` rules need to move them to `~/.frontal-code/settings.json`.
- `credentials.json` is written with mode `0600`.
- Saved OAuth tokens are stored per provider. Refreshing one provider's token no longer touches the Anthropic entry.
- Token refresh uses only the token endpoint recorded at login. It no longer reads `settings.oauth` from project config, which could have sent a refresh token to a host set by a repository. Token endpoints must be `https`, and token requests do not follow redirects. Logins saved before this change need to be redone with `frontal-code login`.
- `code login` reads `settings.oauth` from user-level settings only. Project and local settings are ignored, so a repository cannot send an authorization code to a host it chooses. Move any login config from `.frontal-code/settings.json` to `~/.frontal-code/settings.json`.

## [0.1.1] - 2026-04-09

### Changed
- Updated `extensions/frontal-code-slack` test tooling to `vitest` 3.2.4 and pinned `vite` 6.4.2 to resolve Dependabot alerts in the lockfile.
- Removed the unused `ultracite` dependency from the Slack extension in favor of the existing Biome-based linting workflow.

### Fixed
- Resolved vulnerable transitive `vite` and `esbuild` versions in `extensions/frontal-code-slack/package-lock.json`.

### Added
- **Core Configuration System**: New type-safe configuration system with `code-core` crate
- **Project Configuration**: Centralized `config/project.json` for feature flags and internal settings
- **Configuration Manager**: Bridge between core and runtime configuration systems
- **Type-safe Configuration**: Rust structs for all configuration values with validation
- **Feature Flags**: Comprehensive feature flag system for telemetry, plugins, caching, metrics
- **Provider Configuration**: Centralized AI provider configuration with default models
- **Service Configuration**: Database, Redis, and memory service configuration
- **UI Configuration**: Theme, colors, and progress bar settings
- **Sandbox Configuration**: Docker and execution limit settings
- **Configuration Documentation**: Comprehensive configuration guides and API references
- **Configuration Examples**: Working examples demonstrating configuration usage
- **Doctor Integration**: Core configuration status in `frontal-code doctor` command
- **Environment Variable Overrides**: Support for overriding configuration via environment variables
- **Multiple Configuration Locations**: Support for user, project, and local configuration files
- **Backward Compatibility**: Seamless integration with existing runtime configuration system
- Complete development infrastructure setup
- Nix flake for reproducible development environment
- Pre-commit hooks configuration
- Docker and Docker Compose setup
- VS Code workspace configuration
- PostgreSQL database schema for structured memory
- Development documentation
- Homebrew formula for installing the `frontal-code` CLI from source

### Changed
- Repository structure moved to root level
- Updated documentation references
- Improved development workflow
- Release assets now include Homebrew-friendly tarballs and SHA-256 checksum files

### Fixed
- Corrected file path references in documentation
- Fixed Docker container permissions
- Resolved build configuration issues

## [0.1.0] - 2026-04-03

### Added
- Initial Rust implementation of Frontal Code
- Core CLI functionality
- Basic agent runtime
- Tool system implementation
- Session management
- Permission system
- MCP server lifecycle management
- Mock parity harness for testing
- Comprehensive test suite

### Features
- Interactive REPL with slash commands
- One-shot prompt execution
- Session persistence and resume
- Multi-provider support (Anthropic, OpenAI, xAI, Frontal, Bedrock, Azure)
- Comprehensive tool ecosystem
- Plugin management
- Configuration management
- Git integration
- Markdown terminal rendering

### Infrastructure
- 9-lane parity checkpoint achieved
- All 9 requested features merged to main
- Mock service for deterministic testing
- CI/CD pipeline configuration
- Container deployment support

### Documentation
- Complete usage documentation
- Development setup guide
- Architecture philosophy
- Roadmap and parity status
- API documentation

## [0.0.1] - 2026-03-31

### Added
- Project initialization
- Basic repository structure
- Initial documentation
- Development workflow setup

---

## Release Notes

### Version 0.1.0
This marks the first stable release of Frontal Code with complete Rust implementation. The project has achieved full parity with the original design specifications and includes all core functionality for both local development and server deployment.

Key achievements:
- Complete 9-lane parity implementation
- Production-ready infrastructure
- Comprehensive development tooling
- Full documentation and guides

### Upcoming Releases
Future releases will focus on:
- Enhanced TUI experience
- Multi-agent coordination
- Autonomous server features
- Advanced memory systems
- Event-driven automation
