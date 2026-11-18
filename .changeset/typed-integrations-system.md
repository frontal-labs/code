---
'@frontal-labs/code': minor
'@frontal-labs/code-sdk': minor
---

Add a typed, manifest-driven integrations system. Integrations are declared as `IntegrationManifest` files and consumed by the server, CLI, and SDKs. Four first-party providers ship as built-ins — `github`, `linear`, `posthog`, and `sentry` — and can be overridden per-project via `FRONTAL_CODE_INTEGRATIONS_CONFIG` or the `mcp.integrations` settings file. New server routes expose `GET /v1/integrations`, `GET /v1/integrations/{id}`, `GET /v1/integrations/{id}/capabilities`, `GET /v1/integrations/{id}/events`, and a stubbed `POST /v1/integrations/{id}/capabilities/{name}/invoke` (501). The CLI gains `integrations list|show|capabilities|validate` commands, and both the Rust and TypeScript SDKs gain an `IntegrationClient`. The legacy `call_*` MCP wrappers remain functional as a deprecated adapter layer for one release.
