# Homebrew Formula for Frontal Code

This directory contains the Homebrew formula for installing the Frontal Code CLI tool.

## Installation

Install Frontal Code using the local Homebrew formula:

```bash
brew install --HEAD ./homebrew/frontal-code.rb
```

This will:
- Build the Frontal Code CLI from source using Rust
- Install the `frontal-code` binary to your Homebrew prefix
- Enable tab completion and shell integration

## Formula Details

The `frontal-code.rb` formula:

- **Description**: High-performance Rust AI agent harness
- **Homepage**: https://github.com/frontal-labs/frontal-code
- **License**: MIT
- **Source**: Installs from the main git branch (`--HEAD`)
- **Dependencies**: Rust toolchain for building
- **Install Target**: Builds and installs from `crates/cli`

## Development

When developing locally, you can reinstall the formula after making changes:

```bash
brew reinstall --HEAD ./homebrew/frontal-code.rb
```

Or build directly from source:

```bash
cargo build --workspace
cargo run -p cli -- ...
```

## Verification

After installation, verify the CLI is working:

```bash
frontal-code --version
frontal-code --help
```

## Uninstallation

Remove Frontal Code using Homebrew:

```bash
brew uninstall frontal-code
```
