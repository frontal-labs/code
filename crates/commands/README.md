# Frontal Code Commands

Command registry and execution system for the Frontal Code CLI.

## Overview

This crate provides the core command infrastructure for the Frontal Code CLI, including command registration, execution, and plugin integration. It defines the command system that powers both built-in and plugin-provided commands.

## Features

- Command registry with built-in and plugin commands
- Slash command specification and parsing
- Command execution framework
- Plugin integration for extensible command sets
- Command manifest and source tracking

## Key Components

- **CommandRegistry**: Central registry for all available commands
- **SlashCommandSpec**: Specification for slash command definitions
- **CommandManifestEntry**: Command metadata and source information
- Plugin integration for dynamic command loading

## Dependencies

- `frontal-code-plugins` for plugin system integration
- `frontal-code-runtime` for core runtime functionality
- `serde_json` for command data serialization
