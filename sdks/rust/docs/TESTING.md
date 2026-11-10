# Rust SDK Testing Guide

## Test Architecture

Most tests are hermetic. They do **not** require:
- Network access
- Provider API keys
- The real `frontal-code` CLI

`tests/common/mod.rs` writes a small fake `frontal-code` executable at runtime. The fake emits predefined JSONL events and records the arguments it was called with, so the tests can assert on the exact flags the SDK passes.

The end-to-end suite (`tests/e2e_test.rs`) is the exception. It runs the real CLI against a live model and is skipped unless `FRONTAL_CODE_E2E_BIN` is set.

## Running Tests

```bash
# Hermetic tests (no CLI, no network)
cargo test -p frontal-code-sdk

# End-to-end against a local Ollama model (see below)
FRONTAL_CODE_E2E_BIN=$PWD/target/debug/code \
  cargo test --manifest-path sdks/rust/Cargo.toml --test e2e_test
```

## Test Structure

```
tests/
├── common/mod.rs        # Shared fake CLI and helpers
├── integration_test.rs  # Thread run, streaming, resume, and flag mapping (mocked)
├── config_test.rs       # --config overrides and env handling (mocked)
└── e2e_test.rs          # Real CLI against a live model (gated)
```

## End-to-End Suite

The default provider is a local Ollama server, so no key is needed:

```bash
cargo build -p cli
ollama pull qwen2.5:1.5b
FRONTAL_CODE_E2E_BIN=$PWD/target/debug/code \
  cargo test --manifest-path sdks/rust/Cargo.toml --test e2e_test
```

Set `FRONTAL_CODE_E2E_PROVIDER=openai` with `OPENAI_API_KEY` (and `OPENAI_BASE_URL`) to run against a cloud gateway instead. A skipped run reports as passing, so check that `FRONTAL_CODE_E2E_BIN` is set before you read a green result.
