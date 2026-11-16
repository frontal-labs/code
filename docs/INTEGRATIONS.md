# Integrations

Frontal Code integrations are **fully typed, config-driven manifests**. The
server, CLI, and SDKs all consume the same `IntegrationManifest` shape — no
hardcoded provider code beyond the MCP transport layer.

## Built-in providers

Four first-party integrations ship as built-in manifests. They are seeded into
the registry on startup and can be overridden by project-level configuration:

| id | display name | transport | auth |
| --- | --- | --- | --- |
| `github` | GitHub | `http` | API key (`GITHUB_TOKEN`) |
| `linear` | Linear | `http` | API key (`LINEAR_API_KEY`) |
| `posthog` | PostHog | `http` | API key (`POSTHOG_API_KEY`) |
| `sentry` | Sentry | `http` | API key (`SENTRY_AUTH_TOKEN`) |

### GitHub

| capability | permission | idempotent |
| --- | --- | --- |
| `create_pr` | `workspace_write` | no |
| `create_issue_comment` | `workspace_write` | no |
| `create_check_run` | `workspace_write` | no |

### Linear

| capability | permission | idempotent |
| --- | --- | --- |
| `create_issue_comment` | `workspace_write` | no |

### PostHog

| capability | permission | idempotent |
| --- | --- | --- |
| `capture_event` | `workspace_write` | yes |
| `capture_events_batch` | `workspace_write` | yes |

### Sentry

| capability | permission | idempotent |
| --- | --- | --- |
| `create_issue` | `workspace_write` | no |
| `create_issue_comment` | `workspace_write` | no |

## Manifest schema

An `IntegrationManifest` is a JSON document with the following shape:

```json
{
  "id": "my-integration",
  "version": "1.0.0",
  "display_name": "My Integration",
  "description": "What this integration does.",
  "transport": "http",
  "transport_config": {
    "kind": "http",
    "url": "https://api.example.com",
    "headers": {}
  },
  "capabilities": [
    {
      "name": "do_thing",
      "description": "Performs a thing.",
      "input_schema": { "type": "object" },
      "output_schema": { "type": "object" },
      "permission_mode": "workspace_write",
      "idempotent": false
    }
  ],
  "auth": {
    "kind": "api_key",
    "env_var": "MY_API_KEY",
    "header": "Authorization"
  },
  "events": [],
  "context_fields": []
}
```

### Fields

| field | type | required | description |
| --- | --- | --- | --- |
| `id` | `string` | yes | Unique identifier, lowercase alphanumeric + hyphens |
| `version` | `string` | yes | Semver version |
| `display_name` | `string` | yes | Human-readable name |
| `description` | `string` | yes | What the integration does |
| `transport` | `string` | yes | `stdio`, `http`, `sse`, `websocket`, `sdk`, or `managed_proxy` |
| `transport_config` | `object` | yes | Transport-specific config, tagged by `kind` |
| `capabilities` | `array` | yes | Capabilities exposed by the integration |
| `auth` | `object` | no | Auth spec: `none`, `oauth`, or `api_key` |
| `events` | `array` | no | Webhook events this integration can emit |
| `context_fields` | `array` | no | Fields written into task context |

### Capability fields

| field | type | required | description |
| --- | --- | --- | --- |
| `name` | `string` | yes | Capability identifier |
| `description` | `string` | yes | What the capability does |
| `input_schema` | `object` | yes | JSON Schema for input validation |
| `output_schema` | `object` | no | JSON Schema for output validation |
| `permission_mode` | `string` | yes | `read_only`, `workspace_write`, or `danger_full_access` |
| `idempotent` | `boolean` | yes | If true, safe to retry on failure |

### Auth spec variants

| kind | fields |
| --- | --- |
| `none` | — |
| `api_key` | `env_var` (string), `header` (string, optional) |
| `oauth` | `client_id_env`, `client_secret_env` (optional), `auth_url`, `token_url`, `scopes` (array), `redirect_path` (optional) |

## Configuration

### Environment variable

Set `FRONTAL_CODE_INTEGRATIONS_CONFIG` to a JSON array of manifests. This is
layered **on top of** the built-in providers; a manifest whose `id` collides
with a built-in overrides it.

```bash
export FRONTAL_CODE_INTEGRATIONS_CONFIG='[
  {
    "id": "github",
    "version": "2.0.0",
    "display_name": "GitHub (custom)",
    "description": "Custom GitHub config",
    "transport": "http",
    "transport_config": { "kind": "http", "url": "https://api.github.com", "headers": {} },
    "capabilities": [],
    "auth": null,
    "events": [],
    "context_fields": []
  }
]'
```

### Settings file

Declare integrations under `mcp.integrations` in `.frontal-code/settings.json`.
The CLI walks up from the current directory to find the settings file. Entries
with the same `id` as a built-in override it.

```json
{
  "mcp": {
    "integrations": {
      "linear": {
        "id": "linear",
        "version": "2.0.0",
        "display_name": "Linear (custom)",
        "description": "Custom Linear config",
        "transport": "http",
        "transport_config": { "kind": "http", "url": "https://api.linear.app/graphql", "headers": {} },
        "capabilities": [],
        "auth": { "kind": "api_key", "env_var": "LINEAR_API_KEY", "header": "Authorization" },
        "events": [],
        "context_fields": []
      }
    }
  }
}
```

## CLI

```bash
# List all integrations (built-ins + config overrides)
frontal-code integrations list

# Show a single integration manifest
frontal-code integrations show github

# List capabilities for an integration
frontal-code integrations capabilities github

# Validate a manifest file
frontal-code integrations validate ./my-manifest.json
```

Add `--json` to any command for machine-readable output.

## REST API

| method | path | description |
| --- | --- | --- |
| `GET` | `/v1/integrations` | List all registered integrations |
| `GET` | `/v1/integrations/{id}` | Get a single manifest |
| `GET` | `/v1/integrations/{id}/capabilities` | List capabilities |
| `POST` | `/v1/integrations/{id}/capabilities/{name}/invoke` | Invoke a capability (501 stub in v1) |
| `GET` | `/v1/integrations/{id}/events` | List webhook events |

### Examples

```http
GET /v1/integrations
Authorization: Bearer <API_KEY>
```

**Response:**
```json
[
  {
    "id": "github",
    "version": "1.0.0",
    "display_name": "GitHub",
    "description": "Manage pull requests, issues, and checks.",
    "capability_count": 3,
    "auth_required": true,
    "event_count": 0
  }
]
```

```http
GET /v1/integrations/github/capabilities
Authorization: Bearer <API_KEY>
```

**Response:**
```json
[
  {
    "name": "create_pr",
    "description": "Create a pull request.",
    "permission_mode": "workspace_write",
    "idempotent": false
  }
]
```

## SDK clients

### Rust

```rust
use frontal_code_sdk::integrations::IntegrationClient;

let client = IntegrationClient::new("http://localhost:8080");
let integrations = client.list().await?;
let manifest = client.get("github").await?;
let capabilities = client.capabilities("github").await?;
```

### TypeScript

```typescript
import { IntegrationClient } from "@frontal-labs/code-sdk";

const client = new IntegrationClient("http://localhost:8080");
const integrations = await client.list();
const manifest = await client.get("github");
const capabilities = await client.capabilities("github");
```

## Backward compatibility

The legacy `IntegrationRegistry::call_*` methods in
`crates/code-integrations/src/mcp/integration.rs` remain as deprecated
adapters over the corresponding capabilities for one release. New code should
use the manifest-driven API. The deprecated wrappers currently cover:

- `call_github_create_pr` → `github.create_pr`
- `call_github_create_issue_comment` → `github.create_issue_comment`
- `call_github_create_check_run` → `github.create_check_run`
- `call_linear_create_issue_comment` → `linear.create_issue_comment`
- `call_graphite_create_stack_comment` → (Graphite is not a built-in; this is
  a legacy adapter only)
- `call_slack_post_message` → (Slack is not a built-in; this is a legacy
  adapter only)
