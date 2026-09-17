# Rust SDK Architecture

## Overview

The Frontal Code Rust SDK (`frontal-code-sdk`) is a thin async wrapper around the `frontal-code` CLI
(`@frontal-labs/frontal-code`). It spawns the CLI as a child process per turn and
communicates via JSONL over stdin/stdout using `--output-format json --stream`.

## High-Level Components

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontal Code Client                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ Frontal Code::new  │  │ start_thread│  │ resume_thread       │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                     │             │
│         ▼                ▼                     ▼             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                  ThreadHandle                         │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │    │
│  │  │ run()       │  │ run_streamed│  │ id()        │   │    │
│  │  └─────────────┘  └─────────────┘  └─────────────┘   │    │
│  └─────────────────────────────────────────────────────┘    │
│         │                │                     │             │
│         ▼                ▼                     ▼             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              ChildProcessManager                      │    │
│  │  - spawns `frontal-code` CLI with --output-format json      │    │
│  │  - manages stdin/stdout/stderr pipes                 │    │
│  │  - parses JSONL event stream                         │    │
│  │  - handles --resume for multi-turn conversations     │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Core Types

### `Frontal Code` - Main Entry Point
```rust
pub struct Frontal Code {
    config: FrontalCodeConfig,
    command: String,           // path to `frontal-code` binary
    env: HashMap<String, String>,
}
```

### `Thread` - Conversation Handle
```rust
pub struct Thread {
    id: String,                // session ID from ~/.frontal-code/sessions
    frontal-code: Arc<Frontal Code>,
    options: ThreadOptions,
}
```

### `ThreadInput` - Structured Input
```rust
pub enum ThreadInput {
    Text(String),
    LocalImage { path: PathBuf },
}
```

### `ThreadRunOptions` - Per-Turn Configuration
```rust
pub struct ThreadRunOptions {
    pub output_schema: Option<Value>,      // JSON Schema for structured output
    pub config_overrides: HashMap<String, Value>, // --config overrides
    pub skip_git_repo_check: bool,
    pub working_directory: Option<PathBuf>,
}
```

## Event Stream Processing

The CLI emits JSONL events on stdout. The SDK parses these into typed events:

```rust
pub enum StreamEvent {
    ItemCompleted(Item),
    TurnCompleted(TurnResult),
    Error(ErrorEvent),
}
```

Events are yielded via `run_streamed()` as an async stream (`futures::Stream`).

## Configuration System

Configuration flows through three layers (highest priority last):

1. **CLI defaults** - Built into `frontal-code` binary
2. **Global config** - `~/.frontal-code/config.toml`
3. **SDK `FrontalCodeConfig`** - `base_url`, `env`, `command` overrides
4. **Thread options** - `working_directory`, `skip_git_repo_check`
5. **Run options** - `output_schema`, `config_overrides` (highest priority)

All config overrides are flattened to dotted TOML and passed as repeated
`--config key=value` flags.

## Testing Architecture

Tests are hermetic - they spawn a mock `frontal-code` script instead of the real CLI:

```rust
// tests/mock_frontal-code.sh - emits predetermined JSONL events
#[tokio::test]
async fn test_thread_run() {
    let frontal-code = Frontal Code::new(FrontalCodeConfig {
        command: Some("tests/mock_frontal-code.sh".into()),
        ..Default::default()
    });
    // ... test assertions
}
```

This enables fast, offline, deterministic testing without API keys or network.

## Error Handling

All fallible operations return `Result<T, FrontalCodeError>`:

```rust
pub enum FrontalCodeError {
    Io(std::io::Error),
    Json(serde_json::Error),
    CliNotFound(String),
    ProcessExited(ExitStatus),
    InvalidEvent(String),
    ThreadNotFound(String),
}
```

## Async Runtime

Requires `tokio` with `rt-multi-thread` and `macros` features. All public APIs
are `async fn` returning `Result`.

## Cargo Features

```toml
[features]
default = ["stream"]
stream = ["futures-util/stream"]  # enables run_streamed()
```
