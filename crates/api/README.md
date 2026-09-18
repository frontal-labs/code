# Frontal Code API

HTTP API service crate for Frontal Code, plus the existing provider API re-exports.

## What It Provides

- Library surface: re-exports `frontal-code-providers` (`pub use frontal-code_providers::*;`)
- Binary: `frontal-code-api` HTTP server for CLI-compatible operations

## Run

```bash
cargo run -p frontal-code-api --bin frontal-code-api
```

By default it binds to `127.0.0.1:8787`.

## Environment Variables

- `FCODE_API_HOST` (default: `127.0.0.1`)
- `FCODE_API_PORT` (default: `8787`)
- `FCODE_CLI_BIN` (optional path to `frontal-code` binary)
- `FCODE_API_WORKDIR` (optional working directory for executed CLI commands)
- `FRONTAL_API_KEY` (required API key for server authentication; accepts `x-api-key` or `Authorization: Bearer ...`)

## REST Endpoints

- `GET /health`
- `POST /v1/cli/run` - generic CLI execution (`args` array)
- `POST /v1/prompt` - prompt request with model/provider/options
- `GET /v1/status`
- `GET /v1/sandbox`
- `GET /v1/version`

All command endpoints run the CLI with JSON output by default and return:
- command args
- exit code and success flag
- stdout/stderr
- parsed `json` payload when stdout is valid JSON
