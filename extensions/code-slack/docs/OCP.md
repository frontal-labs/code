# OpenChannel Protocol host

The Slack bot hosts its behavior as an [OpenChannel Protocol](https://github.com/frontal-labs/open-channel) (OCP) application built with [`ocp-sdk`](https://www.npmjs.com/package/ocp-sdk) 2.x. Every Slack request the bot acts on and every message it sends is a typed OCP v2.0 event in a durable Postgres event store. Task creation and approval decisions run as policy-checked OCP actions.

Code lives in `src/`: `bridge.ts`, `agent.ts`, `actions.ts`, `policy.ts`, `actors.ts`, `transport.ts`, and `store.ts`. `SlackInterface` (`src/slack.ts`) only talks to OCP through `OcpBridge` (`src/bridge.ts`).

## Components

| Piece | File | Role |
| --- | --- | --- |
| Agent `agent:frontal-code` | `agent.ts` | Receives every `message.received`. Plain messages become task prompts; messages with metadata `kind=frontal-code.approval.decision` are approval clicks. |
| Actions | `actions.ts` | `frontal-code.task.create` → `POST /v1/tasks`; `frontal-code.task.approval.resolve` → `POST /v1/tasks/:id/approval`. Inputs are validated with zod before they are recorded. |
| Policy | `policy.ts` | Rule-based, default deny. Only the two actions above are granted. |
| Slack channel | `transport.ts` + `ocp-sdk/slack` | The SDK normalizes DMs and delivers `message.sent` through a Bolt-backed transport. Block Kit travels as an `application/vnd.slack.blocks+json` artifact. |
| Event store | `store.ts` + `ocp-sdk/postgres` | Postgres tables `ocp_events` (configurable) and `ocp_actions`. |
| Migration | `src/migrate.ts` | Idempotent schema migration that reads only `OCP_DATABASE_URL` and `OCP_EVENTS_TABLE`. |

Who may trigger an action is still decided by `src/access.ts` (user/channel allowlists, task owner or admin) before a request reaches the agent. The OCP policy bounds what the agent itself may do.

## Event flow

| Slack input | OCP event ID | Events recorded |
| --- | --- | --- |
| DM to the bot | Slack `event_id` | `message.received` (actor `slack:<user>`) → `policy.allowed` → `action.executed`/`action.failed` (`frontal-code.task.create`) → `message.sent` |
| `@code` mention | Slack `event_id` | same as DM |
| `/code <prompt>` | `slack-command:<trigger_id>` | same as DM, top-level reply |
| Approval button | `slack-action:<user>:<action_ts>` | `message.received` (metadata `kind=frontal-code.approval.decision`) → `policy.allowed` → `action.executed`/`action.failed` (`frontal-code.task.approval.resolve`) |
| Bot posts a task update | random UUID | `message.sent` (actor `agent:frontal-code`), then delivered by the Slack sink |
| Bot edits a message | random UUID | `message.sent` with metadata `kind=update`, `slack_ts=<ts>`, recorded after `chat.update` |

Event IDs are unique keys in the store. A Slack redelivery or a double click with the same ID is recorded once and never re-runs its action, even after a restart.

Bot messages and approval actions about a task share the correlation ID `frontal-code-task:<task id>`. Task-creation actions are correlated by their action ID, `task-create:<event id>`, because the task ID only exists once the control plane answers.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `OCP_DATABASE_URL` | required | Postgres connection string (`postgres://` or `postgresql://`). |
| `OCP_EVENTS_TABLE` | `ocp_events` | Event table name (lowercase identifier). `ocp_actions` is fixed by the SDK. |
| `OCP_RUN_MIGRATIONS` | `true` | Migrate on startup. Set `false` when a deploy step runs `node dist/migrate.js`. |
| `OCP_AGENT_ID` | `agent:frontal-code` | Actor ID of the agent. |

The bot refuses to start if Postgres is unreachable. `GET /ready` returns 503 until the Slack socket is listening and the store answers `select 1`. `GET /health` only reports that the process is alive.

Deployment wiring:

- **Kubernetes** (`infrastructure/kubernetes/base`, `infrastructure/terraform`): an `ocp-migrate` init container runs the migration, and the app runs with `OCP_RUN_MIGRATIONS=false`. Concurrent first runs can race on `CREATE`; the kubelet retries the failed init container.
- **Fly** (`deploy/fly-slack.toml`): `release_command = "node dist/migrate.js"`. Set the URL with `fly secrets set OCP_DATABASE_URL=...`.
- **Compose** (`infrastructure/compose`): an `ocp-postgres` service, migrated on startup.

Provisioning the database itself (RDS, Neon, Fly Postgres, ...) is up to the deployer.

## Audit queries

```sql
-- Everything the bot said and decided about one task.
select occurred_at, event_type, event_json->'actor'->>'id' as actor
from ocp_events
where correlation_id = 'frontal-code-task:<task id>'
order by sequence;

-- Approval decisions: who clicked what.
select occurred_at,
       event_json->'actor'->>'id'                            as slack_user,
       event_json->'messageReceived'->'metadata'->>'taskId'  as task_id,
       event_json->'messageReceived'->'metadata'->>'action'  as action
from ocp_events
where event_type = 'message.received'
  and event_json->'messageReceived'->'metadata'->>'kind' = 'frontal-code.approval.decision'
order by sequence;

-- Failed actions with their error.
select occurred_at, event_json->'actionFailed'->'action'->>'type' as action,
       event_json->'actionFailed'->>'errorMessage' as error
from ocp_events
where event_type = 'action.failed'
order by sequence desc;
```

From code, `OcpBridge.app.store` exposes the same data, e.g. `app.store.query({ eventType: "action.failed" })`.

## Scaling

The bot keeps no OCP state in process memory that another replica would need. Approvals are resolved when they are clicked, and their state lives in the Frontal Code control plane. Duplicate suppression is enforced by the shared event store. Running several replicas is safe as far as OCP is concerned.

## Testing

- `tests/ocp/*.test.ts` run the real `openChannel` runtime on an in-memory store (`tests/helpers/memory-event-store.ts`).
- `tests/ocp/postgres.integration.test.ts` runs against Postgres when `OCP_TEST_DATABASE_URL` is set. It covers migration, the audit trail, and redelivery across a restart.
