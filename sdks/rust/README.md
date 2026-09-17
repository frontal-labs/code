# Frontal Code SDK (Rust)

Embed the Frontal Code agent in your Rust workflows and apps.

The SDK wraps the `frontal-code` CLI (`@frontal-labs/frontal-code`). It spawns the CLI per
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
use frontal-code_sdk::{Frontal Code, ThreadInput, ThreadOptions, ThreadRunOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let frontal-code = Frontal Code::new(Default::default());
    let thread = frontal-code.start_thread(ThreadOptions::default());
    let turn = thread
        .run(&ThreadInput::Text("Diagnose the test failure".into()), &ThreadRunOptions::default())
        .await?;
    println!("{}", turn.final_response);
    Ok(())
}
```

## Features

- `Frontal Code::start_thread` / `Frontal Code::resume_thread`
- `Thread::run` (buffered `TurnResult`) and `Thread::run_streamed` (event stream)
- Structured input entries (`text`, `local_image`)
- `--config` overrides flattened to dotted TOML literals (incl. `baseUrl` →
  `frontal_base_url`)
- Multi-turn continuation via `--resume`

## Testing

The SDK's tests are hermetic: they spawn a small mock `frontal-code` script instead of
the real binary, so no API key or network is required.

```bash
cargo test -p frontal-code-sdk
```
