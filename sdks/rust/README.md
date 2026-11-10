# Frontal Code SDK (Rust)

Embed the Frontal Code agent in your Rust workflows and apps.

The SDK wraps the `frontal-code` CLI (`@frontal-labs/code-cli`). It spawns the CLI per
turn and continues conversations via the CLI's `--resume <sessionId>` flag,
parsing the `--output-format json --stream` JSONL event stream.

## Installation

```toml
[dependencies]
frontal-code-sdk = "0.1"
```

Requires the `frontal-code` CLI on `PATH` (or provide a custom `command`).

## Quickstart

```rust
use frontal_code_sdk::{FrontalCode, ThreadInput, ThreadOptions, ThreadRunOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = FrontalCode::new(Default::default());
    let thread = sdk.start_thread(ThreadOptions::default());
    let turn = thread
        .run(&ThreadInput::Text("Diagnose the test failure".into()), &ThreadRunOptions::default())
        .await?;
    println!("{}", turn.final_response);
    Ok(())
}
```

## Features

- `FrontalCode::start_thread` / `FrontalCode::resume_thread`
- `Thread::run` (buffered `TurnResult`) and `Thread::run_streamed` (event stream)
- Structured input entries (`text`; `local_image` is accepted in the protocol but
  the CLI does not yet forward images)
- `--config` overrides flattened to dotted TOML literals (incl. `baseUrl` →
  `frontal_base_url`, `output_schema`)
- Multi-turn continuation via `--resume`
- Permission control: `--permission-mode`, `--dangerously-skip-permissions`
- Tool restriction: `--allowed-tools` (repeatable)

## Testing

The SDK's tests are hermetic: they spawn a small mock `frontal-code` script instead of
the real binary, so no API key or network is required. Authentication is handled by the
`frontal-code` CLI through provider-specific environment variables (e.g.
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `FRONTAL_API_KEY`) that you set in your shell;
this SDK does not inject any environment variables of its own.

```bash
cargo test -p frontal-code-sdk
```
