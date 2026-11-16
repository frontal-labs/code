# Frontal Code Hosted Worker Architecture

## Overview

Frontal Code ships with a hosted worker model for long-running agent tasks.
The control plane is a Rust backend (`code-server`), and the worker itself is a
headless run of the `frontal-code` CLI inside an isolated Frontal Sandbox
session. Operators start a backend; users authenticate to it and submit tasks.
The backend launches a sandbox session, streams the working tree into its VFS,
runs the CLI against a per-task worker token, and mirrors status, progress,
and results back into the control plane.

The current design:

- **Control plane**: `code-server` (Rust, axum) — owns task state, worker
  tokens, event storage, and API surface.
- **Execution**: a Frontal Sandbox session per task. The session is created
  by the sandbox server (template/snapshot resolved server-side), the
  checkout is written into the session's `/workspace`, and the
  `frontal-code` CLI runs in-stream with `stream=true` as the task runner.
- **Task runner**: `frontal-code hosted task run <task_id>` — a single CLI
  mode used by both the in-session run and local development.
- **Worker credentials**: short-lived per-task worker tokens minted by the
  server; the CLI calls the server's public endpoints with that token.

This document covers the current architecture, credential flow, task
lifecycle, isolation guarantees, threat model, and the path to stronger
isolation.

## Architecture

```text
                 ┌────────────────────────────────────────────┐
                 │              code-server (Rust)            │
                 │                                            │
 users ─────────▶│  axum API                                  │
                 │   ├── REST routes (auth, tasks, events)    │
                 │   ├── token middleware (worker tokens)     │
                 │   ├── event store (in-memory / Postgres)   │
                 │   ├── task runner (worker access)          │
                 │   └── worker access (token mint/verify)    │
                 │                                            │
                 │  lane transport: sandbox                   │
                 │   ├── SandboxSession::create (isolated)    │
                 │   ├── write_file → /workspace/<path>       │
                 │   ├── exec: frontal-code hosted task run   │
                 │   ├── observe_completion (poll)            │
                 │   └── destroy_session (cancel)             │
                 └────────────────────────────────────────────┘
                                   │
                                   ▼
                     ┌───────────────────────────────┐
                     │   Frontal Sandbox (sessions)  │
                     │                               │
                     │   template/snapshot           │
                     │   (resolved by sandbox server)│
                     │                               │
                     │   /workspace  (checkout)      │
                     │   .frontal-code-hosted/       │
                     │     ├── env                   │
                     │     └── task.json             │
                     │                               │
                     │   frontal-code (CLI, headless)│
                     │   runs the agent in-stream    │
                     └───────────────────────────────┘
```

In this architecture:

- `code-server` is the source of truth for task state, worker tokens, and
  the public API.
- Each task runs in a dedicated sandbox **session** (session ID doubles as
  the worker ID). The session is created from a template/snapshot resolved by
  the sandbox server (which provides `frontal-code` and the standard
  tooling); the server talks to the sandbox server over
  `FRONTAL_SANDBOX_BASE_URL` (auth via `FRONTAL_SANDBOX_API_KEY`).
- The checkout is streamed file-by-file into `/workspace` inside the
  session (`.git` is excluded), so the session starts from the same tree
  the task was submitted against.
- The `frontal-code` CLI runs inside the session as a headless task runner,
  with `FRONTAL_CODE_SERVER_API_KEY` set to the per-task worker token.

### Why in-session CLI?

The historical design ran the CLI in a locally spawned Docker container
and only mirrored task state into Postgres. That split caused:

- Two execution environments that had to be kept in sync.
- A Postgres requirement even when the operator only needed task storage.
- Docker-specific plumbing (relayed containers, `host.docker.internal`
  network exceptions, daemon health) on every host.

Moving execution into a sandbox session removes all of that:

- **Single execution environment**: the task runner runs in the same
  session that owns the checkout.
- **Worker tokens survive restarts**: tokens are stored hashed in
  Postgres (`worker_tokens.rs`), so the sandbox transport requires
  `FRONTAL_CODE_SERVER_DATABASE_URL`. The server remains the source of
  truth for tasks, events, and worker tokens; sandbox sessions are the
  execution boundary.
- **Session-level isolation**: the sandbox runtime owns the filesystem,
  process, and network boundary instead of the local Docker daemon.

## Components

### code-server (Rust control plane)

`code-server` is the Rust backend that owns:

- Authentication (user API keys, session tokens).
- Task lifecycle (creation, launch, completion, cancellation).
- Worker token minting and verification.
- Event storage and SSE streaming.
- Worker orchestration through the sandbox session transport.

Relevant modules:

- `lib.rs` — server wiring, lane transport selection, sandbox session
  transport (`FRONTAL_CODE_SERVER_LANE_TRANSPORT=sandbox`).
- `worker_access.rs` — worker token minting/verification and the provider
  proxy env map injected into each session.
- `event_store.rs` — in-memory and Postgres event storage.

### Sandbox session transport

When a task is submitted, the sandbox lane transport:

1. Creates an isolated session (template/snapshot resolved by the sandbox
   server), with a network policy that allowlists only the code-server host
   (`FRONTAL_CODE_SERVER_URL`).
2. Writes the checkout into `/workspace` in the session VFS (skipping
   `.git`).
3. Writes the task payload (task ID, provider, model, mode) to
   `/workspace/.frontal-code-hosted/task.json` and the worker environment
   to `/workspace/.frontal-code-hosted/env`.
4. Runs `frontal-code hosted task run <task_id>` in the session
   (working directory `/workspace`, streaming output).
5. Polls the sandbox server for completion and translates the finish reason
   into `WorkerStatus::Completed` or `WorkerStatus::Failed`.
6. On cancellation, destroys the session.

The session ID is the worker ID; the audit event records
`worker.launched ... session=<id>`.

### frontal-code CLI

The `frontal-code` CLI is the agent binary. In the hosted worker model it is
invoked in one specific mode:

```text
frontal-code hosted task run <task_id>
```

The CLI:

- Resolves the task from the server using the worker token
  (`FRONTAL_CODE_SERVER_API_KEY`).
- Runs the agent in-stream against the server's endpoints.
- Streams progress back to the server as events.
- Emits a final completion marker that the server maps to task status.

## Credential Flow

```text
┌─────────┐    1. POST /tasks (user auth)     ┌───────────────┐
│  User   │ ─────────────────────────────────▶│  code-server │
└─────────┘                                   └───────┬───────┘
                                                      │
        2. sandbox session create (isolated)          │
                                                      ▼
                                        ┌────────────────────────┐
                                        │ Frontal Sandbox        │
                                        │  (session per task)    │
                                        │                        │
                                        │  /workspace/…          │
                                        │  .frontal-code-hosted/ │
                                        │    env  (token + env)  │
                                        │    task.json           │
                                        └───────────┬────────────┘
                                                    │
        3. frontal-code hosted task run <task_id>   │  (in session)
                                                    │
        4. CLI → /v1/tasks/<id>/* with              │
           FRONTAL_CODE_SERVER_API_KEY              │
                                                    ▼
                                        ┌────────────────────────┐
                                        │  code-server           │
                                        │  (worker token auth)   │
                                        └────────────────────────┘
```

1. The user submits a task to `code-server` with normal user credentials.
2. `code-server` creates a sandbox session for the task (isolated tier),
   streams the checkout into `/workspace`, and writes the worker
   environment and task payload under `.frontal-code-hosted/`.
3. The CLI runs inside the session with the per-task worker token as
   `FRONTAL_CODE_SERVER_API_KEY`.
4. The CLI calls back to `code-server` using the worker token.
5. `code-server` verifies the token, enforces task scoping, and records
   events.

### Worker token model

Worker tokens are minted by `code-server` at task launch:

- One worker token per task.
- Bound to the task ID and the server.
- Short-lived: they carry a sliding TTL (`FRONTAL_CODE_WORKER_TOKEN_TTL_SECS`,
  default 7200) refreshed on use.
- Verified on every worker callback.
- Carried by the CLI as `FRONTAL_CODE_SERVER_API_KEY` inside the session.

The token grants access to task-scoped endpoints only:

- Task metadata.
- Event streaming.
- Task status updates.
- Model proxy endpoints (for provider calls).

It does not grant:

- Task creation.
- Task cancellation (owner-only).
- Other users' tasks.

### Why the env file?

Sandbox sessions can sleep and wake; the environment is dropped across that
boundary. The worker environment (server URL, worker token, provider env) is
therefore serialized to `/workspace/.frontal-code-hosted/env` in the session
VFS, where it survives, and re-injected for the in-session run.

## Task Lifecycle

```text
            ┌────────────┐
            │  Created   │  user submits task via API
            └─────┬──────┘
                  ▼
            ┌────────────┐
            │  Launching │  worker token minted, sandbox session
            └─────┬──────┘  created, checkout + env streamed
                  ▼
            ┌────────────┐
            │   Running  │  frontal-code in session streams events
            └─────┬──────┘
                  ▼
        ┌─────────┴─────────┐
        ▼                   ▼
  ┌────────────┐      ┌────────────┐
  │ Completed  │      │   Failed   │  session finished with an
  └────────────┘      └────────────┘  error/failed finish reason
        ▲
        │
  ┌────────────┐
  │ Cancelled  │  user cancels; session destroyed
  └────────────┘
```

State transitions:

| From        | To          | Trigger                                    |
|-------------|-------------|--------------------------------------------|
| Created     | Launching   | Worker session created.                    |
| Launching   | Running     | CLI started in session.                    |
| Running     | Completed   | Finish reason ok.                          |
| Running     | Failed      | Finish reason error/failed, or launch failure. |
| Created     | Cancelled   | User cancels before launch.                |
| Running     | Cancelled   | User cancels; sandbox session destroyed.   |

## Isolation Guarantees

The sandbox session provides several isolation properties:

1. **Session boundary**: each task runs in its own session with its own
   VFS and process space. There is no shared mutable state between tasks.
2. **Network policy**: the session's network access is limited to the
   code-server host (and whatever the sandbox runtime allowlists by
   default). The worker cannot reach arbitrary internal hosts.
3. **Worker token scoping**: the token is bound to one task and cannot
   access other tasks or create new tasks.
4. **Checkout hygiene**: the checkout is written file-by-file and `.git`
   is excluded, so credentials or hooks inside `.git/config` are not
   carried into the session by the transport.
5. **Reproducible environment**: sessions start from a template/snapshot
   resolved by the sandbox server, so the execution environment is
   consistent across tasks.
6. **Destructive cancellation**: cancelling a task destroys the session,
   so no state outlives the task except what the server recorded.

### What is NOT isolated

Be explicit about the limits of the current model:

- **The server is trusted**: `code-server` holds the checkout, the worker
  token, and the event stream. A compromised server can see all task data.
- **Model proxy is server-mediated**: provider credentials live on the
  server and the worker calls through the server's proxy endpoints.
- **Session limits/secrets**: per-session resource limits and secret
  injection are not implemented on the server yet (501); the session
  runs under the sandbox server's defaults.
- **No per-session egress allowlist beyond the server host**: network
  policy allowlists the code-server host; broader egress rules depend on
  the sandbox runtime.

## Threat Model

### Threat: malicious task content

A submitted task can contain prompt injection or malicious instructions.

**Mitigations**:

- The agent runs in a sandbox session, not on the host.
- Network access is limited by the session network policy.
- The worker token cannot escape the task boundary.

**Residual risk**:

- The agent can still see the full checkout.
- Prompt injection can influence agent behavior within the session.

### Threat: malicious agent behavior

The agent can exfiltrate data, install tools, or behave unpredictably.

**Mitigations**:

- Session-level isolation (VFS, process, network).
- Destructive cancellation.
- Event audit trail on the server.

**Residual risk**:

- The agent has the full toolset of `frontal-code` inside the session.
- Any tool that can reach the network can reach the code-server host.

### Threat: server compromise

If `code-server` is compromised, the attacker has:

- All task data.
- All worker tokens.
- Provider credentials.

**Mitigations**:

- The server should be deployed with standard security practices
  (TLS, least-privilege DB access, secret management).

**Residual risk**:

- The current model assumes a trusted server. There is no client-side
  verification of server integrity.

### Threat: session escape

If the sandbox runtime has an escape vulnerability, a task could reach
the host.

**Mitigations**:

- The sandbox runtime owns the boundary; keep it updated.
- The network policy limits what an escaped session can reach.

**Residual risk**:

- Runtime vulnerabilities are outside the scope of this project.

## Still Open

- Per-session resource limits and secret injection (currently 501 on the
  server side).
- Broader egress allowlisting (model endpoints, package registries) if
  tasks need direct provider or package access.
- Sandbox session telemetry and cost accounting per task.
- Multi-node sandbox server topology and failover.

## Legacy: tools-agent transport

The `tools-agent` lane transport still exists for operators who want to run
the `frontal-code` CLI directly on the host. It is gated behind
`FRONTAL_CODE_SERVER_ALLOW_HOST_AGENT=1` and is not the recommended
transport. Hosted tasks on that lane run as a local process with
`FRONTAL_CODE_HOSTED_WORKER=1` and no sandbox session boundary.
