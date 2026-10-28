# Frontal Code Slack Connector

TypeScript Slack connector for Frontal Code hosted tasks. It creates tasks from Slack, subscribes to Frontal Code task events over WebSocket, reconstructs Slack routing from Frontal Code task snapshots and event payloads, renders approval workflows, and exposes lightweight operator commands such as orphan policy inspection.

The bot is an [OpenChannel Protocol](https://github.com/frontal-labs/open-channel) host built on [`ocp-sdk`](https://www.npmjs.com/package/ocp-sdk): every Slack request and bot message is a typed OCP event, task creation and approval decisions are policy-checked OCP actions, and the full trail is stored in Postgres. See [`docs/OCP.md`](docs/OCP.md).

## Current Capabilities

- create hosted Frontal Code tasks from `/code <prompt>` and Slack messages
- stream task updates from Frontal Code into Slack threads
- resolve orphaned hosted-lane approvals with `Retry Lane` and `Cancel Task`
- inspect control-plane orphan policy with `/code policy orphans ...`
- recover active Slack tasks and approval linkage from Frontal Code on startup
- record an auditable OCP event trail in Postgres and ignore Slack redeliveries, including across restarts

## Command Surface

### Slash commands

- `/code <prompt>`: create a hosted Frontal Code task
- `/code policy orphans`: show the default orphan policy and configured rule set
- `/code policy orphans repo=<repo> source=<source> priority=<priority>`: preview the effective orphan policy for a target task shape

### Message handling

- non-bot messages handled by the connector create hosted Frontal Code tasks
- approval buttons in task threads resolve orphaned hosted-agent approvals

### Examples

```bash
/code Fix the login bug in auth service
/code policy orphans
/code policy orphans repo=myorg/myapp source=api
/code policy orphans repo=myorg/myapp source=slack priority=high
```

## Runtime Layout

The current connector implementation lives in `src/`:

- `src/main.ts`: process entrypoint
- `src/index.ts`: app wiring, health (`/health`) and readiness (`/ready`) server, graceful shutdown
- `src/migrate.ts`: standalone OCP event store migration for deploy steps
- `src/slack.ts`: Slack socket-mode interface, command handling, thread updates, approval actions
- `src/bridge.ts`, `src/agent.ts`, `src/actions.ts`, `src/policy.ts`, `src/actors.ts`, `src/transport.ts`, `src/store.ts`: OpenChannel Protocol host (agent, actions, policy, Slack transport, Postgres store, `OcpBridge` facade)
- `src/api-client.ts`: Frontal Code HTTP client for tasks, approvals, Slack connector callbacks, context updates, and orphan policy inspection
- `src/websocket.ts`: Frontal Code WebSocket event stream client (connection, reconnect, parsing)
- `src/events.ts`: event routing for tracked-task subscriptions (replay buffer, dedupe, handler fan-out)
- `src/config.ts` and `src/env.ts`: validated environment configuration
- `src/log.ts`: connector logging
- `src/types.ts`: Slack and Frontal Code API types

## Required Environment

Copy `.env.example` to `.env` and configure:

```env
# Slack
SLACK_BOT_TOKEN=xoxb-your-bot-token
SLACK_APP_TOKEN=xapp-your-app-token
# SLACK_SIGNING_SECRET is optional in Socket Mode and can be omitted.

# Slack access control (see "Access Control" below)
SLACK_ALLOWED_USER_IDS=U0123456789   # required: empty denies all task creation
SLACK_ALLOWED_CHANNEL_IDS=           # optional: empty allows any channel
SLACK_ADMIN_USER_IDS=                # optional: may act on other users' tasks

# Frontal Code server
FRONTAL_CODE_API_URL=http://127.0.0.1:8788
FRONTAL_CODE_API_TIMEOUT=30000

# OpenChannel Protocol event store (required)
OCP_DATABASE_URL=postgres://user:password@localhost:5432/frontal_code_slack
OCP_EVENTS_TABLE=ocp_events          # optional
OCP_RUN_MIGRATIONS=true              # optional: set false when a deploy step runs `node dist/migrate.js`
OCP_AGENT_ID=agent:frontal-code      # optional

# App
NODE_ENV=development
LOG_LEVEL=info
PORT=3000

# Optional
GITHUB_TOKEN=
LINEAR_TOKEN=
LINEAR_API_URL=https://api.linear.app/graphql
GRAPHITE_TOKEN=
GRAPHITE_API_URL=https://graphite.dev/api
SENTRY_DSN=
MAX_CONCURRENT_TASKS=10
TASK_TIMEOUT=3600000
HEALTH_CHECK_INTERVAL=30000
```

## Development

Prerequisites:

- Node.js 22.18+ (required by `ocp-sdk`)
- Postgres for the OCP event store, e.g. `docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres:17-alpine`
- a running Frontal Code server with hosted task APIs enabled
- Slack app configured for Socket Mode

Typical loop:

```bash
bun install
cp .env.example .env
bun run dev
```

Useful commands:

```bash
bun run sync:code-events
bun run build
bun run typecheck
bun run test
bun run migrate
bun run lint
bun run format
```

`bun run build` now checks that `src/generated/code-events.ts` matches the Rust event contract generator. If the file is stale, run `bun run sync:code-events` and rebuild.

## Slack App Setup

1. Create a Slack app and enable Socket Mode.
2. Configure a slash command for `/code`.
3. Add bot token scopes: `app_mentions:read`, `im:history`, `chat:write`, `commands`. No user token scopes are required.
4. Subscribe to bot events `app_mention` and `message.im`. Do not subscribe to `message.channels`, `message.groups`, or `message.mpim`.

If you previously installed the app with the older manifest, remove the `channels:history`, `groups:history`, and `mpim:history` scopes and the `message.channels`, `message.groups`, and `message.mpim` subscriptions, then reinstall the app so Slack applies the change.

## Access Control

Tasks run with the organization's GitHub, Linear, and Graphite credentials, so access is deny-by-default:

- **Who can start tasks:** only users listed in `SLACK_ALLOWED_USER_IDS`. An empty list denies all task creation, including `/code` and DMs.
- **Where tasks start:** channels are only used through an explicit `@code` mention (`app_mention`). Ordinary channel messages never start a task. Direct messages from an allowlisted user start a task without a mention.
- **Channel restriction:** if `SLACK_ALLOWED_CHANNEL_IDS` is set, mentions and `/code` in other channels are ignored. DMs are not channel-restricted. An empty list allows any channel.
- **Acting on tasks:** approve, retry, and cancel buttons only work for the user who created the task or a user in `SLACK_ADMIN_USER_IDS`. The actor must also be in `SLACK_ALLOWED_USER_IDS`.

Slack user IDs look like `U0123456789` (or `W...` for enterprise grid users). Channel IDs start with `C`, `G`, or `D`. Find IDs in the Slack profile or channel details menu. Invalid entries fail startup validation.

## Operator Workflows

### Inspect orphan policy

Use `/code policy orphans` to inspect the default control-plane orphan policy and configured rule set.

Use selectors to preview the exact effective policy Frontal Code would apply:

```bash
/code policy orphans repo=myorg/myapp source=api
/code policy orphans repo=myorg/myapp source=slack priority=high
```

### Resolve orphan approvals

When Frontal Code emits `approval.requested` for an orphaned hosted agent, the connector renders `Retry Lane` and `Cancel Task` buttons in the Slack thread. A click is recorded as an OCP `message.received` from the Slack user and resolved through the policy-checked `frontal-code.task.approval.resolve` action, which calls the server-side approval API.

## Verification

```bash
bun run typecheck
bun run test
bun run build
```

`tests/ocp/postgres.integration.test.ts` also runs against a real database when `OCP_TEST_DATABASE_URL` points at a disposable Postgres.

For an end-to-end operator runbook covering hosted task inspection, orphan recovery, approvals, and policy tuning, see `docs/hosted-task-operations.md`.
