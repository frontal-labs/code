# Frontal Code API

HTTP API service crate for Frontal Code, plus the existing provider API re-exports.

## What It Provides

- Library surface: re-exports `code-providers` (`pub use frontal-code_providers::*;`)
- Binary: `code-api` HTTP server for CLI-compatible operations

## Run

```bash
cargo run -p code-api --bin code-api
```

By default it binds to `127.0.0.1:8787`.

## Environment Variables

- `FRONTAL_CODE_API_HOST` (default: `127.0.0.1`)
- `FRONTAL_CODE_API_PORT` (default: `8787`)
- `FRONTAL_CODE_CLI_BIN` (optional path to `frontal-code` binary)
- `FRONTAL_CODE_API_WORKDIR` (optional working directory for executed CLI commands)
- `FRONTAL_API_KEY` (API key for server authentication; accepts `x-api-key` or `Authorization: Bearer ...`. Required for `POST /v1/cli/run` and for any non-loopback bind)

## Request Guards

Every route checks the request before it is handled:

- The `Host` header must name `localhost`, a loopback IP, or the address the server is bound to. A missing or other hostname returns `403`. This blocks DNS-rebinding attacks.
- If an `Origin` header is present, it must match the request's own `Host` (same origin). Cross-origin browser requests return `403`.
- `POST /v1/cli/run` returns `403` unless `FRONTAL_API_KEY` is configured, because it can run agent prompts. Other routes keep the optional-key behaviour.

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
