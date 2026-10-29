# Conversation Store Design

This document designs a ClickHouse-backed store for the full history of every conversation (CLI, REPL, Slack bot, IDE extensions, hosted tasks), plus semantic embeddings. It serves two uses: analytics (tokens, cost, tool error rates, usage patterns) and agent recall ("how did we fix X last time?").

It is a design only. It contains no code. Every behavior claim cites `file:line` on the base commit below.

**Verified against:** `origin/main` @ `7d44a2f`. Re-check citations before relying on a line number, because the tree moves.

## Table of Contents

- [Goals and non-goals](#goals-and-non-goals)
- [Corrections to the current assumptions](#corrections-to-the-current-assumptions)
- [Current behavior](#current-behavior)
- [Constraints from recent main](#constraints-from-recent-main)
- [Conversation origin inventory](#conversation-origin-inventory)
- [Can code-server host ingest?](#can-code-server-host-ingest)
- [Reuse of code-telemetry](#reuse-of-code-telemetry)
- [Architecture and data flow](#architecture-and-data-flow)
- [Event model](#event-model)
- [Redaction](#redaction)
- [ClickHouse choices](#clickhouse-choices)
- [Schema sketch](#schema-sketch)
- [Risk check](#risk-check)
- [Follow-ups](#follow-ups)
- [Decisions needed](#decisions-needed)

## Goals and non-goals

**Goals**

- Record every message, tool call and compaction from every client, with stable IDs, ordering and metadata.
- Keep local JSONL as the source of truth. Upload asynchronously and never block an agent turn.
- Make ingest idempotent on `(session_id, seq, block_idx)`.
- Store embeddings for recall, with a model registry so models can be swapped.
- Enforce per-user isolation on the server.

**Non-goals (this phase)**

- Replacing the local session format. Old files must keep loading.
- A web UI. Dashboards come in Phase 8.
- Syncing sessions back down to clients.

## Corrections to the current assumptions

These were checked against the code. Each one changes the design.

1. **`push_message` is not the only write path.** `Session::push_message` (`crates/code-runtime/src/session.rs:184`) is bypassed by:
   - compaction, which swaps the message list (`crates/code-runtime/src/compact.rs:115-116`);
   - auto-compaction, which replaces the in-memory session (`crates/code-runtime/src/conversation.rs:524`);
   - `fork`, which clones messages into a new session (`session.rs:215`);
   - `/clear`, which replaces the session (`cli/src/main.rs:6672`, `clear_session`);
   - the REPL's `persist_session`, which rewrites the whole file (`cli/src/main.rs:6529`);
   - tests that assign `session.messages` directly.

   Phase 1 needs one write path: private fields, plus explicit `replace_for_compaction` and `fork` operations that emit events.

2. **Session IDs are not UUIDs.** They are `format!("session-{millis}-{counter}")` (`session.rs:849`). Two processes started in the same millisecond can produce the same ID. The `uuid` crate is built with `v4` only (`crates/code-orchestrator/Cargo.toml:11`). `getrandom 0.2` is now a code-runtime dependency (added by `c121edd`), so UUIDv7 needs only the `uuid` `v7` feature.

3. **Rotated files are full older copies, not slices.** Rotation happens only inside `save_to_path` (`session.rs:159`, `:878`). Every full write includes all messages, so each rotated file is a complete older snapshot. Pruning keeps the 3 newest by modification time (`session.rs:925`).

4. **Writes are not crash-safe.** Full writes go to a temp file and are renamed (`write_atomic`, `session.rs:852`). Nothing in the workspace calls `sync_all` or `sync_data`, so a power loss can lose the rename. Appends use an unlocked `OpenOptions::append` plus `writeln!` (`session.rs:422-423`). A torn last line makes the whole file fail to load.

5. **Auto-compaction never reaches disk on its own.** It changes memory only. The file keeps the old messages until the next full save.

6. **No serde.** `session.rs` writes JSON by hand with `crate::json::JsonValue`. Unknown keys inside a record are ignored. Unknown record types and unknown block types are hard errors (`session.rs:374-378`, `:642`). New record types therefore need a version gate, not just a new key.

7. **Usage has no model or provider.** Each assistant message stores `TokenUsage` (`session.rs:532-534`), with no model or provider. Cost is computed only at display time from a hard-coded table (`crates/code-runtime/src/usage.rs:11`, `pricing_for_model`).

8. **The memory adapters drop errors.**
   - The Pinecone trait implementation ignores the result of its best-effort upsert (`crates/code-memory/src/vector_pinecone_adapter.rs:244-247`). Its vector metadata has no embedding model.
   - The file metadata store writes to a fixed temp name, ignores write errors, ignores the rename result, and has no fsync (`crates/code-memory/src/persistent_metadata_store.rs:174`, `persist_snapshot`).
   - Neo4j builds Cypher by string interpolation. Since `3156952`, scope values are escaped (`crates/code-memory/src/graph_neo4j_adapter.rs:354-363`), but the statements themselves are not parameterized.

9. **The mock parity harness is broken on main.** `scripts/run_mock_parity_harness.sh` no longer exists. `cli/tests/mock_parity_harness.rs:804` loads `../../mock_parity_scenarios.json`, which is also missing, so the test fails before running any case. The "every phase must pass `./scripts/run_mock_parity_harness.sh`" gate cannot pass until the harness is restored. This is a prerequisite for Phase 1, not optional.

10. **`/history` already exists** as a slash command (`crates/code-commands/src/lib.rs:697`). The brief's `/history` conflicts with it.

11. **There is no JetBrains plugin.** The only IDE extension is a VS Code/Cursor JavaScript extension that spawns the CLI (`extensions/code-ide/extension.js`).

## Current behavior

### Session format and storage

- Storage path: `<cwd>/.frontal-code/sessions/<session-id>.jsonl` (`crates/code-runtime/src/session_control.rs:84`, `:101`; `PRIMARY_SESSION_EXTENSION = "jsonl"` at `:10`). Legacy `.json` files still load (`:154`). The CLI has its own copy of the directory logic (`cli/src/main.rs:7060`, `sessions_dir`), which should be removed (see Follow-ups).
- File layout: one `session_meta` line, an optional `compaction` line, then one `message` line per message (`session.rs:396-453`, `:737-742`). Loading accepts those three record types (`:353-373`).
- Data structures (`session.rs:19-84`):
  - `MessageRole { System, User, Assistant, Tool }`.
  - `ContentBlock`: `Text`, `ToolUse { id, name, input }`, `ToolResult { tool_use_id, tool_name, output, is_error }`.
  - `ConversationMessage { role, blocks, usage: Option<TokenUsage> }`.
  - `SessionCompaction { count, removed_message_count, summary }`.
  - `SessionFork { parent_session_id, branch_name }`.
  - `Session { version, session_id, created_at_ms, updated_at_ms, messages, compaction, fork }`.
- **Missing fields:** messages have no ID, sequence number or timestamp. Sessions have no user, repo, branch, model, provider or client. The only "branch" is `fork.branch_name`, a label, not a git branch.

### Write paths

- `push_message` appends one line (`session.rs:184`, `:411-425`). If the file is missing or empty, it writes the full snapshot instead (`:416-419`).
- `save_to_path` rewrites the whole file through temp + rename (`session.rs:159`). The REPL calls it after turns (`cli/src/main.rs:6529`).
- Runtime callers of `push_message`: user message (`conversation.rs:303`), assistant message (`:362`), tool result (`:465`).

### Rotation

- `ROTATE_AFTER_BYTES = 256 * 1024` (`session.rs:13`), `MAX_ROTATED_FILES = 3` (`:14`).
- On a full save, a file of at least 256 KB is renamed to `<stem>.rot-<millis>.jsonl` (`session.rs:878-892`). Old rotated files beyond 3 are deleted (`:925`).

### Compaction

- `compact_session` (`crates/code-runtime/src/compact.rs:96`) keeps the last `preserve_recent_messages` (default 4, `:18`). Everything earlier becomes one System message holding a rule-based summary (counts, tool names, recent requests), not an LLM summary. It is not archived.
- Callers: auto-compaction (`conversation.rs:519-537`), the slash command (`crates/code-commands/src/lib.rs`), and the CLI (`cli/src/main.rs:5133`).

### Session IDs

- `session-{millis}-{counter}` (`session.rs:849`). Used by `new` (`:134`), `fork` (`:215`), and as the fallback when loading legacy files.

### Usage and cost

- `TokenUsage { input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens }` (`crates/code-types/src/usage.rs:29`), all `u32`.
- The runtime records usage on each model call (`conversation.rs:342`). It is attached to the assistant message (`session.rs:532-534`).
- `TurnSummary.usage` is the running total for the session, not the per-turn figure (`conversation.rs:479`).
- The `turn_completed` trace event carries no usage.

### Embeddings

- Only `local-hash-embedding-v1` exists. It hashes words into vectors, so it matches shared words, not meaning. It uses provider name `"local-hash"`, dimension 384, normalized (`crates/code-embeddings/src/lib.rs:19-27`, `LocalMlEmbeddingProvider` at `:102`).
- The `EmbeddingProvider` trait is at `:75`. `EmbeddingModelInfo` (`:30`) carries `provider, model_name, dimension, normalize, revision`. `revision` is always `None`.

### Memory

- Three traits, all synchronous and infallible (`crates/code-memory/src/lib.rs`):
  - `MemoryMetadataStore` (`:273`);
  - `MemoryVectorStore` (`:285`);
  - `MemoryGraphStore` (`:297`).
- Scope: `MemoryScope { session_id, repo_id, branch_id }` (`crates/code-types/src/memory.rs:4-8`).
- Backend selection from `FRONTAL_CODE_MEMORY_*` variables (`lib.rs:27-34`, read at `:204-211`):
  - `_METADATA_PATH` (file store);
  - `_PINECONE_URL`, `_PINECONE_NAMESPACE`, `_PINECONE_API_KEY`;
  - `_NEO4J_URL`, `_NEO4J_DATABASE`, `_NEO4J_USERNAME`, `_NEO4J_PASSWORD`.
- Memory items already record `embedding_model`, `embedding_provider`, `embedding_dimension` and `embedding_revision` (`lib.rs:74-85`). Items that don't match the active provider are skipped at search time.

### Telemetry

- `code-telemetry` has a `TelemetrySink` trait (`crates/code-telemetry/src/lib.rs:171`) and a `SessionTracer` that adds a sequence and timestamp (`:246`). It has no async, batching, HTTP, retry or redaction.

## Constraints from recent main

These commits since the original brief add constraints the design must respect.

- **Workspace trust gate** (`cf83258`). Project and local settings are withheld until the workspace is trusted. User-level config is always honored. The trust list lives in user config (`crates/code-runtime/src/config.rs:16` `TRUSTED_WORKSPACES_FILE_NAME`, `:301` `is_workspace_trusted`, `:311` `trust_workspace`). A repository cannot trust itself.
  - *Design rule:* exporting can be switched on only from user-level settings or a trusted workspace. A per-repo opt-out is always honored, even in an untrusted workspace.
- **cargo-deny blocks CI** (`d1c0bed`; `.github/workflows/security.yml:36-37`; policy in `deny.toml`). It enforces a license allowlist and crates.io-only sources.
  - *Design rule:* the `clickhouse` client must use `rustls-tls-ring` plus `rustls-tls-webpki-roots`. `rustls-tls` pulls in `aws-lc-sys`, which brings a license outside the allowlist. Phase 3 must run `cargo deny check`.
- **Windows CI** (`dad0251`; `.github/workflows/rust-ci.yml`, job `test-windows`, `cargo test -p cli -p code-runtime -p code-plugins`).
  - *Design rule:* the crash-safe write path and its tests must run on Windows. That means renaming over an existing file, skipping directory fsync on non-Unix, and hashing home paths of the form `C:\Users\…`.
- **code-server runs read-only** (`1bd4c38`; `infrastructure/terraform/frontal-code-server.tf:194`). The writable mounts are `/var/lib/frontal-code/server`, `/var/lib/frontal-code/agents` and `/tmp` (emptyDir).
  - *Design rule:* ingest has no durable local spool. Under pressure it returns 429 with `Retry-After`, and the client holds the data.
- **OAuth authorize is POST behind the control-plane key** (`d22c9db`; `crates/code-server/src/lib.rs:1568`). There is still no identity endpoint.
- **code-api requires `FRONTAL_API_KEY` for non-loopback binds** (`b16be25`; `crates/code-api/src/service.rs:73`). That key is a shared secret, not a user.
- **Slack allowlist** (`20e9634`; `extensions/code-slack/src/access.ts`). Task creation is gated on Slack user IDs, and `user_id` sent to code-server is the Slack ID (`extensions/code-slack/src/actions.ts:45`, `api-client.ts:323`).
  - *Design rule:* a Slack ID → Frontal user mapping is needed before rule 6 (per-user isolation) can hold for Slack-originated data.
- **File tools are confined to the workspace root** (`184d064`). This is precedent only. The exporter reads `.frontal-code/sessions` directly.

## Conversation origin inventory

Every place a conversation can start, and whether it persists a session today:

| Origin | Where | Persists a session today? |
|---|---|---|
| CLI one-shot and REPL | `cli/src/main.rs:5433` (`run_repl`), `:6172` (`create_managed_session_handle`) | Yes, JSONL |
| `/clear`, `/session new`, fork, resume | `cli/src/main.rs:6672`, `:6681`, `:6887` | Yes, JSONL |
| IDE extensions (VS Code, Cursor) | `extensions/code-ide/extension.js` (spawns the CLI) | Yes, via the CLI |
| Rust and TypeScript SDKs | `sdks/rust/src/spawn.rs`, `sdks/typescript/src/spawn.ts` (spawn the CLI, `--resume`) | Yes, via the CLI |
| code-api | `crates/code-api/src/service.rs:404` (spawns the CLI) | Yes, via the CLI |
| Hosted task, docker worker | `cli/src/main.rs:3710` (`run_hosted_task_worker`) | Yes, but inside a throwaway container |
| Hosted task, `tools-agent` transport (Fly default) | `crates/code-server/src/lib.rs:384` → `crates/code-tools/src/lib.rs:4746-4758` (`build_agent_runtime`, `Session::new()`) | **No: in memory only** |
| Sub-agents (Agent tool) | `crates/code-tools/src/lib.rs:4582` (`execute_agent`), same runtime | **No: in memory only** |
| Slack bot | `extensions/code-slack/src/actions.ts:45` → `api-client.ts:323` (`createTask`) | No. It becomes a hosted task. |

**Consequences**

- Slack-originated and hosted-task conversations need **server-side emission**. The CLI exporter cannot see them.
- Tools-agent hosted tasks and sub-agent transcripts are lost today. Phase 1 needs a decision on whether to persist them (see Decisions).

## Can code-server host ingest?

**Mechanically, yes.** code-server is an axum 0.7 + tokio service (`crates/code-server/Cargo.toml`). It already runs as a hosted service on Fly, and on Kubernetes with 2 replicas (`infrastructure/terraform/frontal-code-server.tf`). It has a router with public and protected groups (`crates/code-server/src/lib.rs:1568`), and a raw-`Bytes` handler (`:3086-3090`) that shows the pattern for NDJSON bodies.

**Gaps**

1. **Auth is one shared operator key**, from `FRONTAL_CODE_SERVER_API_KEY` (`lib.rs:198`, `:1556`). `user_id` comes from the client (`HostedTaskContext` at `:1022-1026`; `CreateTaskRequest` at `:1334-1353`). This breaks rule 6, because a caller can claim any user.
2. **No gzip decoding.** Add `tower-http` with `decompression-gzip`. `tower-http` 0.5.2 and 0.6.11 are both in `Cargo.lock`, so use 0.6 to match newer tower.
3. **Axum's default 2 MB body limit** applies. Ingest needs an explicit limit.
4. **No metrics, no tracing subscriber, no rate limiting.** Logging is `println!`/`eprintln!`.
5. **Read-only root filesystem** (see Constraints).
6. **Size.** `lib.rs` is large, so the new code belongs in a `conversations/` module.

**Recommendation.** Host ingest in code-server, as a separate router. Put it behind a new per-user auth middleware. That middleware resolves the `FRONTAL_API_KEY` or OAuth bearer token to a `user_id` through a Frontal identity endpoint, with a short-lived cache. The operator key keeps protecting the existing routes.

If no identity endpoint can be built, the fallback is a separate `code-ingest` service with its own key-to-user mapping. That is Decision 2.

**Sibling server.** `crates/code-api` (`service.rs:260-266`) spawns the CLI and needs `FRONTAL_API_KEY`. It is not a good ingest host, because it is a per-CLI-invocation bridge, not a control plane.

## Reuse of code-telemetry

`code-telemetry` is a poor transport. It is synchronous, has no HTTP, and has no batching or retry. Its `TelemetrySink` trait (`crates/code-telemetry/src/lib.rs:171`) would let us add an adapter later, but the exporter does not need it.

**Verdict:** the exporter reads JSONL past a per-session cursor and uploads itself. Reuse only the event naming conventions from `AnalyticsEvent`.

## Architecture and data flow

```
 CLI / REPL / IDE / SDK          Slack bot, hosted tasks (server-side)
          │                                   │
  JSONL on disk (source of truth)              │
          │                                   │
  exporter (background task)                  │
   - reads past cursor, never writes session  │
   - redacts (client pass)                    │
          │                                   │
          └──────────── POST /v1/conversations/events ─────────┘
                         gzip NDJSON, per-user auth
                                   │
                  code-server ingest router
                   - auth → user_id (server-derived)
                   - redact (server pass)
                   - validate, dedupe on (session_id, seq, block_idx)
                                   │
                  ClickHouse batched async inserts
                   (async_insert=1, wait_for_async_insert=1)
                                   │
                  embedding worker → embeddings_<model>
                                   │
                  search endpoint → ClickHouseMemoryStore → SearchHistory tool
```

**Sequence for one upload batch**

1. The exporter reads up to N events past the cursor from the session's JSONL file.
2. It redacts each event and serializes NDJSON, then gzips the body.
3. It sends `POST /v1/conversations/events` with the auth header.
4. Server: 202 with the highest accepted `seq` per session. The client advances its cursor to the accepted value.
5. Server: 429 with `Retry-After`. The client backs off and keeps its cursor.
6. Network failure or 5xx: exponential backoff with jitter. The cursor is unchanged, so the batch is re-sent later. Duplicates are safe because the key is idempotent.

**Failure-mode table**

| Failure | Behavior | Test (phase) |
|---|---|---|
| Offline | Cursor stays put, backoff, local file unaffected | offline retry (5) |
| 429 | Honor `Retry-After`, keep cursor | 429 (4, 5) |
| Crash mid-write of the session file | Torn last line is tolerated by the exporter, which stops at the last complete line | crash mid-write (1) |
| Duplicate batch | Idempotent insert on the key | duplicate batch (4) |
| Old file format | Legacy `session-…` IDs and no `seq` are backfilled from line order | old format (1) |
| Untrusted workspace | Project-level export setting ignored | trust (5) |
| One bad event | Rejected individually, the rest of the batch is written | bad event (4) |

**Cursor location.** The exporter stores its cursor in a sidecar file, `<session-id>.export-cursor`, next to the session file. It does not write into the session file, so the session format is unchanged.

## Event model

**Envelope (v1)**

```
schema_version  u16         = 1
event_id        string      UUIDv7 (idempotency-friendly, time-ordered)
session_id      string      legacy `session-…` IDs kept as-is
seq             u64         monotonic per session
block_idx       u16         index of the content block within the message
kind            enum        session_started | session_updated | message
                            | tool_call | tool_result | compaction | session_forked
ts              RFC 3339    client clock, plus server ingest time
client          enum        cli | repl | slack | ide | hosted | sdk
client_version  string
```

**Idempotency key:** `(session_id, seq, block_idx)`. The same key arriving twice is a no-op.

**Legacy backfill.** Old files have no `seq`. Assign `seq` from line order, starting at 1. Set `created_at` to null and `legacy = true`. Keep the `session-…` IDs as strings. Do not rewrite old files.

**Versioning.** New record types in the JSONL need a version gate, because the loader rejects unknown record types. The plan is a `format_version` on `session_meta`. Readers accept the current version and the one before it.

## Redaction

Redaction runs twice: once in the client exporter, once in the server ingest path. The shared code lives in a new `code-conversation::redact` module.

**Rules**

- Provider and GitHub API keys, AWS access keys, JWTs, and PEM private-key blocks are replaced with `[REDACTED:<kind>]`.
- `.env`-style `KEY=value` lines are redacted when the key name looks secret-like.
- Bearer headers (`Authorization: Bearer …`) are redacted.
- Connection strings with passwords (`scheme://user:pass@host`) have the password redacted.
- Tool outputs longer than a cap are cut to head and tail, with `original_bytes` recorded.
- Home-directory paths are hashed: `/Users/<name>/…` and `C:\Users\<name>\…` become `~h:<sha256-prefix>/…`.

**Precedent.** `masked_authorization_header` (`crates/code-providers/src/providers/anthropic.rs:79`) is the only existing secret masking. It covers one header only.

**Test plan.** Fixture tests for each leak class, plus fixture tests for clean text, to prove the redactor does not over-redact.

## ClickHouse choices

- **Server version.** 25.8 LTS at minimum. Approximate vector search with vector similarity indexes became GA in 25.8, and the HNSW index definition is used in the schema. At implementation time, pin the newest LTS in compose and CI.
- **Client crate.** The official `clickhouse` crate (`clickhouse/clickhouse-rs`).
  - Features: `inserter`, `uuid`, `chrono`, `lz4`, `rustls-tls-ring`, `rustls-tls-webpki-roots`.
  - Writes use server-side `async_insert=1` and `wait_for_async_insert=1`. A 202 from ingest then means the rows are durable in ClickHouse.
  - It belongs only in code-server and the embedding worker. The CLI never holds ClickHouse credentials.
  - It must pass `cargo deny check`.
- **Rust toolchain.** Pinned at 1.95.0 (`rust-toolchain.toml:5`). There is no workspace MSRV, so a current crate version is fine.
- **Bazel.** Bazel only wraps cargo. There is no `rules_rust`, and `third_party/` is scaffolding (`MODULE.bazel`).
  - A new dependency needs no Bazel change.
  - A new crate needs a `BUILD.bazel` (copy `crates/code-server/BUILD.bazel`) and an entry in `crates/BUILD.bazel`.
- **Migrations.** None exist in the repo today. Use plain versioned `.sql` files, applied by a small runner in code-server that tracks a `schema_migrations` table.
- **Local development and CI.** Compose files live in `infrastructure/compose/`, not `docker/`. Add a ClickHouse service there. CI has no `services:` containers today, so Phase 3 adds a smoke job.

## Schema sketch

Phase 3 writes the real migrations. This is the shape they will take.

- `sessions`: `ReplacingMergeTree(version)`, keyed by `(user_id, session_id)`. Holds repo ID, branch, client, model, provider and fork info.
- `messages`: `PARTITION BY toYYYYMM(created_at)`, `ORDER BY (user_id, session_id, seq, block_idx)`. Holds role, block kind, text, and per-message usage.
- `tool_calls`: one row per tool call and result, with `is_error`, tool name, and output head/tail plus `original_bytes`.
- `compactions`: one row per compaction, with the summary and removed count. Archived originals go here (Phase 1 archives them locally first).
- `embedding_models`: registry of `(model, provider, dimension, revision, active)`.
- `embeddings_<model>`: `Array(Float32)` vector, with an HNSW `vector_similarity('hnsw', 'cosineDistance', <dim>)` index. One table per model, so models can be migrated side by side (`reembed --model`).
- `LowCardinality(String)` for `client`, `role`, `kind`, `provider`, `model`.
- Configurable TTLs on `messages` and `tool_calls`.
- **Isolation.** `user_id` is in every table and every search filter. The server derives it from authentication, never from the client body. A ClickHouse row policy is added as defense in depth.

## Risk check

Answers to the three questions the brief requires every design to answer.

1. **Can anything block a turn?** No. The exporter runs in its own task and only reads files. It never takes the session's lock or waits on a turn.
2. **Can anything lose local history?** Rotation is gated on the upload cursor, with a disk-usage cap. Compaction archives the original messages to a local file before the in-memory replacement. Writes gain fsync in Phase 1.
3. **Can a secret or another user's data reach ClickHouse or a search result?** Redaction runs on the client and on the server. `user_id` is derived by the server. Every search filter is injected server-side. Project-level settings cannot enable export in an untrusted workspace.

**Known gap.** Until the server identity endpoint exists, the Slack ID and the operator key are not per-user identities. Slack-originated and hosted-task rows cannot satisfy rule 6 until that is resolved. This is Decision 1.

## Follow-ups

Not in Phase 0 scope. Tracked here.

- Restore the mock parity harness and its script. This is a prerequisite for the phase gate.
- Fix stale `crates/*` paths in `README.md` (around lines 261-285) and `docs/ARCHITECTURE.md`, `docs/DEVELOPMENT.md` (around lines 80-94).
- Make the memory adapters report errors instead of dropping them (Pinecone, file metadata store).
- Parameterize the Neo4j statements.
- Remove the duplicated session-directory logic (`cli/src/main.rs:7060` vs `crates/code-runtime/src/session_control.rs:84`).
- Document `.frontal-code/sessions` path behavior in `USAGE.md` after Phase 1 changes it.

## Decisions needed

1. **Identity source for per-user isolation.** Is there, or will there be, a Frontal endpoint that maps an API key or OAuth token to a stable `user_id`? How do Slack user IDs map to Frontal users?
2. **Ingest host.** code-server (per-user auth added, read-only fs, 2 replicas), or a new `code-ingest` service?
3. **ClickHouse deployment.** ClickHouse Cloud or self-hosted? Region and data residency? Default retention TTLs for `messages` and `tool_calls`?
4. **In-memory transcripts.** Persist `tools-agent` hosted-task and sub-agent transcripts, or capture them only server-side?
5. **Session ID migration.** Keep `session-{ms}-{n}` for existing sessions and use UUIDv7 for new ones?
6. **Mock parity gate.** Restore the harness before Phase 1 (recommended), or drop the requirement?
7. **The existing `/history` command.** Extend it, or name the new recall command differently (for example `/recall`)?
8. **Embeddings gateway.** Does `FRONTAL_BASE_URL` serve `/embeddings`? Which model and dimension should be the default?
9. **Tool-output policy.** Store full redacted outputs, or head and tail only? Which tools go on the embedding allowlist?
10. **Deletion semantics.** For `forget <session>`: a lightweight DELETE, or a tombstone plus TTL? Where does the audit log live?
11. **Export opt-in scope.** Allow opt-in from user-level settings only, or also from a trusted workspace's project settings?
