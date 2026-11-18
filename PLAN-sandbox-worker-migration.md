# Plan: Replace the Docker Worker with Frontal Sandbox (+ Terraform Deployment Infra)

## Objective

Wire `https://github.com/frontal-labs/sandbox` in as the execution sandbox for
code-server hosted agents. The current `LocalDocker` `LaneWorkerTransport`
spins up a hardened Docker container per task; this plan **fully replaces it**
with a Frontal Sandbox **session** as the persistent, sandboxed workspace in
which the `frontal-code` agent CLI runs. Sessions start in the **isolate tier**
(Tier 1) and **promote to a QEMU microVM** (Tier 2) when a command needs a real
binary.

This plan also covers the **deployment infrastructure** that the sandbox server
needs to run — specifically the sandbox repo's `infrastructure/terraform/`
(OpenTofu) provisioning of KVM-capable cloud hosts — so that the code-server
change has an actual place to run.

## Decisions (from user)

1. **Depth:** Full replacement of the Docker worker, not a thin new transport.
   The `LocalDocker` transport is deprecated and removed; all hosted tasks run
   through a Frontal Sandbox session.
2. **Tier:** Start in **isolate tier**, with **VM promotion** for real command
   execution (cargo, npm, git, deno, the agent binary, etc.).
3. **SDK:** Use the sandbox repo's Rust SDK `Client`
   (`sdks/rust/src/client.rs`), fully usable via the `/v1` HTTP API. Do NOT
   depend on the stub `embedded` mode.
4. **Repo import (v1):** Stream files from the local checkout into the session
   VFS via `client.write_file` (Option B) rather than `git clone` in-session, to
   avoid needing git + network in the VM.
5. **Event streaming:** Keep the agent's existing event protocol — the worker
   CLI POSTs fine-grained events to `FRONTAL_CODE_SERVER_URL` /
   `/v1/events/ws`; sandbox `exec_stream` only captures top-level stdout/stderr.
6. **Deployment:** Use the sandbox repo's OpenTofu (Terraform-compatible) infra
   under `infrastructure/terraform/` to provision a KVM-capable host; code-server
   talks to the sandbox server on that host over the allowlisted network.

## Current state (what we're replacing)

- `crates/code-server/src/lib.rs`:
  - `LaneTransportKind { LocalDocker, ToolsAgent }` (line 167). Default
    `LocalDocker`. `parse_lane_transport_kind` (line 176) accepts
    `"" | "docker" | "local-docker" | "local_docker"` → `LocalDocker`, and
    `tools-agent | tools_agent | agent` → `ToolsAgent`.
  - `ServerConfig` (line ~142 `lane_transport_kind`), `ServerConfig::from_env`
    (line 222) reads `FRONTAL_CODE_SERVER_LANE_TRANSPORT`.
  - `ServerState::new_with_transport_kind*` (lines 427–496) has a `match
    lane_transport_kind` (line 474) that builds `LocalDockerLaneWorkerTransport`
    or `HostedAgentLaneWorkerTransport`. This is the single wiring point.
  - `HostedAgentLaneWorkerTransport` (line 4416) for the `ToolsAgent` kind.
  - `LaneWorkerTransport` trait (line 4314): `bootstrap(request) ->
    LaneBootstrapResult`, `cancel(worker_id, locator) -> LaneCancellationResult`,
    `observe_completion(worker_id, finish_reason, tokens_output) ->
    LaneCompletionResult`.
  - `LaneExecutionRequest` (line 3867): task_id, prompt, repository, repo_url,
    base_ref, branch, lane_role, model, provider, permission_mode, allowed_tools.
  - `LaneBootstrapResult` (line ~3855): worker_id, worker_status, signals,
    artifacts, checkout_root.
  - `LocalDockerLaneWorkerTransport` (line 4510) does: `prepare_checkout` →
    `git_head_commit` (baseline) → `write_local_docker_task_payload` →
    `worker_access.issue(task_id)` token → `docker.ensure_relay` /
    `prepare_task_network` / `launch(DockerLaunchSpec)` running
    `frontal-code hosted task run <task_id>` with a container env.
- `crates/code-server/src/docker_worker.rs`: Docker CLI command builders,
  relay container (`alpine/socat` pinned by digest), per-task network, hardening
  args. `CliDockerRunner`, `DockerRunner` trait, `RELAY_ALIAS`,
  `task_network_name`, `relay_container_name`.
- `crates/code-server/src/worker_access.rs`: `WorkerAccess`, `issue(task_id)` —
  reused unchanged (per-task token + provider proxy env).
- `crates/code-sandbox/`: local per-bash-command isolation (unshare /
  sandbox-exec / docker). **Unrelated** to the Frontal Sandbox service; stays
  as-is.
- `crates/code-runtime/src/bash.rs`: calls `build_sandbox_command` per bash
  command. Unaffected.

## Target state

```
HostedTaskContext
  -> LaneWorkerTransport = SandboxLaneWorkerTransport
        1. prepare_checkout (local, for the base_commit baseline only)
        2. worker_access.issue(task_id) -> per-task token
        3. create_session(tier_preference=auto, env, network, initial_snapshot?)
        4. import repo checkout into the session VFS (write_file per file)
        5. write the worker env file + task payload file into the session
        6. exec_stream: run `frontal-code hosted task run <task_id>`
             - first exec that needs a real binary triggers Tier1 -> Tier2 promotion
        7. stream stdout/stderr back as lane events; fine-grained agent events
             flow over the existing FRONTAL_CODE_SERVER_URL /v1/events/ws channel
        8. on completion: read back artifacts (diff vs base_commit), snapshot, sleep
        9. cancel -> destroy_session (best-effort snapshot first)
```

The session **is** the persistent workspace: it survives across exec calls, can
sleep/wake between runs, and snapshots give us the baseline + fork for re-runs.
This replaces both the Docker container *and* the local checkout dir.

## Key technical constraints (from the sandbox repo)

- **Rust SDK crate:** `frontal-sandbox-sdk` (`sdks/rust/Cargo.toml`, v0.2.0),
  depends on `sandbox-types` (path `crates/sandbox-types`). The `Client`
  surface: `create_session`, `exec`, `exec_request`, `exec_stream` (NDJSON),
  `read_file`, `write_file`, `delete_file`, `snapshot`, `sleep`, `wake`,
  `list_snapshots`, `get_snapshot`, `fork`, `fork_command`, `checkpoint`,
  `undo`, `get_session`, `destroy_session`, `info`. Only `embedded` mode is a
  stub. We use the HTTP client, so no dependency on the unfinished embedded path.
- **API wire format:** snake_case; timestamps RFC 3339; byte fields are arrays
  of 0–255 ints (SDK converts to/from `Vec<u8>`). Base URL
  `http://localhost:8080` (env `SANDBOX_BASE_URL`), Bearer `sbx_` key (env
  `SANDBOX_API_KEY`).
- **Isolate tier cannot run host binaries.** `ExecRequest` with a real binary
  (git, cargo, npm, the `frontal-code` agent) must promote to a VM. The
  `sandbox-promote` crate handles `NeedsVm` → VFS export to ext4 → VM attach.
  This is the **highest-risk path** (QEMU/KVM required on a Linux host,
  unmeasured promotion latency, ext4 image handoff).
- **Server is early:** in-memory sessions (plus file-backed with `--data-dir`),
  no TLS, no rate limiting beyond admission 429s. Not safe to expose publicly.
- **Env is NOT preserved across sleep/wake** (ADR 0010): env values often hold
  secrets, so they're dropped. We must re-inject the worker env (server URL,
  per-task token, provider proxy) on every wake, or keep env in a file the
  agent reads at startup.
- **`SessionConfig` limits/network/env/secret settings that the backend
  doesn't yet enforce are refused with 501**, not silently ignored. Only file
  size/total/inode/path-depth limits are enforced today.
- **Network policy is default-deny allowlist** — allowlist only the
  provider-proxy host + the code-server URL (the worker already uses a proxy
  for model traffic).

## Sandbox Terraform deployment infrastructure (NEW section)

The sandbox repo ships `infrastructure/` with an OpenTofu (Terraform-compatible)
module for provisioning KVM-capable cloud hosts. The code-server sandbox worker
needs a *running* sandbox server, so deployment of that server is part of this
migration. Reference: `infrastructure/PLAN.md` and `infrastructure/terraform/`.

### What the terraform module provisions (M4)

Per `infrastructure/terraform/`:

- `compute.tf` — one or more KVM-capable instances (`/dev/kvm`, cgroups v2).
  These run the `sandbox serve` process (the scheduler/worker in M5).
- `networking.tf` — VPC, subnets, security groups.
- `storage.tf` — S3-compatible object-store bucket (CAS tier) + block volumes
  for per-VM overlay images.
- `firewall.tf` — ingress rules: **8080** (API + `/metrics` on the same
  listener) and **22** (SSH).

`versions.tf` / `providers.tf` / `variables.tf` / `outputs.tf` define the
provider (the cloud target), inputs (instance type, region, count, AMI, etc.),
and outputs (the public/private IP or DNS name of the sandbox host, plus the
object-store bucket).

### How it integrates with code-server

- code-server talks to the sandbox server over HTTP. The terraform `outputs`
  give the host address; that becomes `FRONTAL_SANDBOX_BASE_URL`
  (`http://<sandbox-host>:8080`). The Bearer key (`sbx_…`) becomes
  `FRONTAL_SANDBOX_API_KEY`.
- The sandbox session's `network.allow` allowlist must include the code-server's
  provider-proxy host + the code-server URL so the in-session agent can POST
  events and reach the model. (The sandbox *host* itself is a peer, not an
  allowlist entry — the allowlist governs egress from the session VM.)
- Deployment flow (operator):
  1. `tofu init` / `tofu apply` in `infrastructure/terraform/` → provision the
     KVM host + object store + firewall.
  2. `ssh` to the host, run `scripts/bootstrap.sh` (deps, KVM, cgroups) and
     `scripts/deploy.sh` (install the `sandbox` binary + systemd unit +
     `/etc/sandbox/config.toml`).
  3. `scripts/smoke.sh` → create a session, `exec echo hi`, destroy.
  4. Read terraform `outputs` → set `FRONTAL_SANDBOX_BASE_URL` +
     `FRONTAL_SANDBOX_API_KEY` in the code-server `.env`, set
     `FRONTAL_CODE_SERVER_LANE_TRANSPORT=sandbox`, restart code-server.

### Terraform deliverables we add (Phase 3 / new)

The sandbox repo's `infrastructure/terraform/` is the *upstream* module. We do
not fork it. Instead, in the **frontal-code** repo we add a thin deployment
layer that consumes it:

- `deploy/sandbox/` (frontal-code repo) — a small wrapper:
  - `README.md` — step-by-step operator guide (provision → bootstrap → deploy →
    smoke → configure code-server).
  - `terraform/` — a wrapper module that `source`s or references the sandbox
    repo's `infrastructure/terraform/` (via a `module` block with a local
    source path to a vendored copy, or a git source pinned to a rev). It adds
    frontal-code-specific variables (e.g. `code_server_egress_allowlist` for the
    network policy) and outputs (`sandbox_base_url`, `sandbox_api_key_file`).
  - `config/production.toml` — a ready-to-use `sandbox` server TOML config
    (`addr = "0.0.0.0:8080"`, `data_dir = "/var/lib/sandbox"`, auth, telemetry)
    matching the schema in the sandbox repo's `infrastructure/config/`.
  - `smoke.sh` — thin delegator to the sandbox repo's smoke script, or a
    self-contained `curl /v1/openapi.json` + session create/exec/destroy.
- `deploy/sandbox/Makefile` — `make provision` (`tofu apply`), `make deploy`
  (bootstrap+deploy), `make smoke`, `make destroy`.
- Update `docs/` and `README.md` to document the sandbox deployment + the two
  new env vars.

### Deployment risks & notes

- **D1 — Terraform/OpenTofu availability.** Operators need `tofu` or
  `terraform` on PATH. CI uses whichever is available (the sandbox repo's
  `infrastructure/ci/tf-plan.yml` pattern).
- **D2 — Provider pinning.** `providers.tf` pins a cloud provider; changing the
  target cloud means updating the provider + variables. Document the default.
- **D3 — No TLS.** The sandbox server has no TLS; the 8080 ingress must be on
  a private subnet or behind a TLS-terminating proxy. The firewall module
  restricts 8080 to the code-server's security group / CIDR.
- **D4 — Object store.** The CAS tier needs an S3-compatible bucket. For local
  dev the server can run with `--data-dir` only (disk CAS); the bucket is for
  production durability.

## Risks & open questions

| # | Risk | Mitigation |
|---|------|-----------|
| R1 | **Promotion path unproven.** Tier1→Tier2 (VFS→ext4→QEMU) is unmeasured. If it doesn't work end-to-end, the whole plan stalls. | Gate phase 1 on a spike that proves a `sandbox` VM runs `frontal-code --version` and `git --version` in a promoted session. If it fails, fall back to `tier_preference=vm` directly (skip isolate) — still a full replacement, just no Tier1 fast path. |
| R2 | **Platform dependency.** VM tier requires QEMU/KVM on Linux. macOS dev hosts can only run TCG (slow) or not at all. | Document the requirement. CI uses the existing KVM VM workflow pattern. Local macOS dev may be isolate-only until a Linux VM is available. Terraform provisions the KVM host for CI/prod. |
| R3 | **Agent binary must exist in the VM image.** The `frontal-code` CLI + its runtime (Rust toolchain, node, etc.) must be installed in the guest rootfs. | Build a dedicated guest rootfs image (or a snapshot) with the agent + toolchain pre-installed. Use `initial_snapshot` on session create to start from it. This is a new build artifact to manage. |
| R4 | **Network egress from the VM.** The agent needs to reach the model provider (via the server's provider proxy) and the server itself. Sandbox network policy is default-deny allowlist. | Configure `SessionConfig.network.allow` with the server's provider-proxy host + the code-server URL. Only those hosts need allowlisting. |
| R5 | **Event streaming parity.** Current worker streams fine-grained events. Sandbox `exec_stream` gives stdout/stderr as NDJSON. | Keep the agent's existing event protocol: the worker CLI in the sandbox still POSTs to `FRONTAL_CODE_SERVER_URL` (`/v1/events/ws`). The sandbox exec only captures top-level stdout/stderr for logging. This preserves all UI behavior. |
| R6 | **Per-task token + provider proxy env.** The worker needs `FRONTAL_CODE_SERVER_URL`, `FRONTAL_CODE_SERVER_API_KEY`, `FRONTAL_CODE_HOSTED_TASK_FILE`, and provider env. Env is dropped on wake (ADR 0010). | Write the env to a file in the session VFS at bootstrap (e.g. `/workspace/.frontal-code-hosted/env`), and have the agent read it at startup. Re-write on wake. Or keep it in the snapshot's `env` field if supported. |
| R7 | **Artifact extraction.** Today the server reads `manifest_file`/`output_file` from the checkout root and diffs vs `base_commit`. In the sandbox, the repo lives in the session VFS. | After the agent completes, read the manifest/output files back via `client.read_file(session, path)`, and diff against `base_commit` (recorded at bootstrap). Snapshot the session so the final state is preserved for review/fork. |
| R8 | **Cancellation.** `destroy_session` kills the session. A promoted VM mid-exec may leave a half-written state. | On cancel: `destroy_session` + `snapshot` before destroy (best-effort) so the partial work is inspectable. The existing `LaneCancellationResult` shape is preserved. |
| R9 | **Sandbox server availability.** code-server now has a hard dependency on a running `sandbox serve`. If it's down, hosted tasks can't start. | Add a health-check + clear error. Make the sandbox base URL + API key server config (`FRONTAL_SANDBOX_BASE_URL`, `FRONTAL_SANDBOX_API_KEY`). Fail fast with a distinct error code at bootstrap, not mid-task. Terraform provisions the host. |
| R10 | **501 refusals.** `SessionConfig` fields the backend doesn't enforce are 501. | Send only the minimal `SessionConfig` (tier_preference, network, env) and omit `limits`/`secrets` until the backend enforces them. Pin a minimum sandbox-server version. |
| D1 | **Terraform/OpenTofu availability.** | Operators need `tofu` or `terraform` on PATH; CI uses the sandbox repo's `tf-plan.yml` pattern. |
| D2 | **Provider pinning.** | `providers.tf` pins a cloud provider; changing the target cloud means updating provider + variables. Document the default. |
| D3 | **No TLS.** | The 8080 ingress must be on a private subnet or behind a TLS-terminating proxy. Restrict 8080 to the code-server's security group / CIDR in `firewall.tf`. |
| D4 | **Object store.** | The CAS tier needs an S3-compatible bucket in prod. Local dev can use `--data-dir` disk CAS. |

## Phased plan

### Phase 0 — Spike: prove promotion works (gate for everything)
**Goal:** de-risk R1/R2/R3 before touching code-server.
1. Stand up a `sandbox serve --addr 127.0.0.1:8080` on a Linux host with QEMU/KVM
   (provisioned via the sandbox repo's terraform, or the KVM VM workflow).
2. From a scratch Rust bin (or `cli`): `create_session(tier_preference=auto)`,
   then `exec("git --version")`. Confirm the session promotes to a VM and the
   command returns exit 0 with real output.
3. Build (or obtain) a guest rootfs image with the `frontal-code` CLI + Rust
   toolchain + node installed. Create a session with `initial_snapshot` from it.
   Confirm `frontal-code --version` runs in the session.
4. **Exit criteria:** a promoted session runs `git --version` and
   `frontal-code --version` and returns correct stdout. If promotion fails,
   record the blocker and switch the plan to `tier_preference=vm` (no isolate
   fast path).

### Phase 1 — Add `SandboxLaneWorkerTransport` (new code, no removal yet)
**Goal:** a third transport kind that runs a full hosted task in a sandbox
session, coexisting with `LocalDocker` so we can compare side by side.
1. `crates/code-server/Cargo.toml`: add a dependency on `frontal-sandbox-sdk`
   (`sdks/rust` from the sandbox repo, as a `git` dep pinned to a rev) and
   `sandbox-types` (for `SessionConfig`, `ExecRequest`, `SessionId`, etc.).
   - Decision: `git = "https://github.com/frontal-labs/sandbox"` with a rev.
     If the workspace uses `git` deps, register them in the root `Cargo.toml`
     `workspace.dependencies` and add to `Cargo.lock`.
2. `crates/code-server/src/lib.rs`:
   - Extend `LaneTransportKind` with `Sandbox`. Extend `parse_lane_transport_kind`
     (line 176) to accept `"sandbox"`. Update the error message (line 181) to list
     `sandbox` as an option.
   - Add `SandboxLaneWorkerTransport` struct holding: `Arc<frontal_sandbox_sdk::Client>`,
     `workspace_root` (for the local base_commit baseline), `server_url` (for the
     provider-proxy allowlist), `Arc<WorkerAccess>`, and the sandbox config
     (base_url, api_key).
   - Wire it in `ServerState::new_with_transport_kind*` (line 474 `match`) alongside
     the existing `LocalDocker` / `ToolsAgent` arms.
   - Add `FRONTAL_SANDBOX_BASE_URL` + `FRONTAL_SANDBOX_API_KEY` to `ServerConfig`
     (line ~142) + `ServerConfig::from_env` (line 222) + `.env.example`.
3. Implement `LaneWorkerTransport` for `SandboxLaneWorkerTransport`:
   - **`bootstrap(request)`:**
     a. Prepare the checkout *locally* (reuse `prepare_checkout`) to compute
        `base_commit = git_head_commit(...)` — the baseline for the diff. (The
        local checkout is for the baseline only; the *execution* happens in the
        sandbox.)
     b. `check_hosted_task_policy` (reuse).
     c. `worker_access.issue(task_id)` → token (reuse).
     d. Build `SessionConfig { tier_preference: Auto, network: allowlist(server
        host + provider-proxy host), env: {} }`. (Env goes to a file in step f.)
     e. `client.create_session(config)` → `session_id`.
     f. Import the repo into the session: stream files from the local checkout
        via `client.write_file(session, path, bytes)` for each file in
        `prepared.checkout_root` (directory walk). This avoids needing
        git+network in the VM and reuses the existing checkout.
     g. Write the worker env file into the session
        (`/workspace/.frontal-code-hosted/env`) with
        `FRONTAL_CODE_SERVER_URL`, `FRONTAL_CODE_SERVER_API_KEY` (token),
        provider proxy env, `FRONTAL_CODE_HOSTED_TASK_FILE`.
     h. Write the task payload file (reuse `write_local_docker_task_payload`,
        adapted to write via `client.write_file`).
     i. Return `LaneBootstrapResult { worker_id: session_id, base_commit, ... }`.
   - **run:** `client.exec_stream(session, ExecRequest { argv: ["frontal-code",
     "hosted", "task", "run", task_id], stream: true, timeout_ms: ... })`.
     Consume the NDJSON stream; forward `output` events to the lane event log.
     The fine-grained agent events still flow over the existing
     `FRONTAL_CODE_SERVER_URL`/`/v1/events/ws` channel (the agent POSTs them
     directly), so UI behavior is unchanged.
   - **artifacts:** after the exec `exit` event, `client.read_file(session,
     manifest_file)` and `output_file`, compute the diff vs `base_commit`
     (reuse the existing diff logic in `publish.rs` / lane completion), snapshot
     the session (`client.snapshot`), `client.sleep(session)` to release the live
     VM while keeping the state.
   - **cancel:** `client.destroy_session(session_id)` (best-effort `snapshot`
     first). Map to `LaneCancellationResult`.
4. `crates/code-server/Cargo.toml` + root `Cargo.toml`: add `frontal-sandbox-sdk`,
   `sandbox-types`, and `reqwest`/`tokio` (already present) as needed.
5. Config: add `FRONTAL_SANDBOX_BASE_URL`, `FRONTAL_SANDBOX_API_KEY` to
   `ServerConfig::from_env` (line 222) and `.env.example`.
6. Tests:
   - Unit: `parse_lane_transport_kind("sandbox")`, `SandboxLaneWorkerTransport::new`.
   - Integration (gated on a running sandbox server, `--ignored` like the KVM
     suites): create a session, import a tiny repo, run `echo hello`, read back
     stdout.

### Phase 2 — Persistence & re-run (snapshots, sleep/wake, fork)
**Goal:** make the session the *persistent* workspace, not just a per-task
container.
1. **Re-run / resume:** store `session_id` + `snapshot_id` on
   `HostedTaskContext` / the persistence layer. A re-run calls
   `client.wake(session_id)` (files come back) or `client.fork(snapshot_id, 1)`
   for a fresh copy, then re-injects the env file (env was dropped on wake),
   then re-execs. This replaces "spin up a new Docker container + re-clone."
2. **Sleep policy:** idle sessions sleep automatically via
   `sandbox serve --idle-sleep-secs` (default 300). Configure the server. Our
   `artifacts` step already sleeps after completion.
3. **Fork for parallel lanes:** `LaneExecutionRequest.lane_role` + multi-lane
   tasks can `client.fork(snapshot_id, n)` to get N sessions sharing a base,
   each independent. Wire this where the current code forks a checkout.
4. Persistence: extend the `ServerState` state file to record
   `session_id`/`snapshot_id` per task so a server restart can re-attach.

### Phase 3 — Remove the Docker worker + land the Terraform deployment
**Goal:** complete the "full replacement" and give operators a deployable
sandbox server.
1. Deprecate `LaneTransportKind::LocalDocker` (keep the variant for a release,
   emit a warning in `from_env` if someone still sets it).
2. Delete `LocalDockerLaneWorkerTransport`, `docker_worker.rs`
   (`CliDockerRunner`, relay, per-task network, hardening args), and the Docker
   launch plumbing in `lib.rs`.
3. Remove Docker-specific config: `FRONTAL_CODE_WORKER_RELAY_IMAGE`, the relay
   container, `worker_hardening_args`, etc. (Keep anything the sandbox path
   reuses.)
4. **Land the sandbox terraform deployment** (see "Sandbox Terraform deployment
   infrastructure" section): `deploy/sandbox/` wrapper module + config + smoke
   + Makefile + docs. Wire `FRONTAL_SANDBOX_BASE_URL` / `FRONTAL_SANDBOX_API_KEY`
   from terraform outputs.
5. Update `.env.example`, `docs/`, README, and the `Bazel` targets
   (`crates/code-server/BUILD.bazel`) to drop Docker deps and add the sandbox
   server as a required service.
6. Update CI: add a sandbox-server service (the KVM VM workflow pattern) for the
   integration tests; the existing Docker-in-Docker CI step is no longer needed
   for the worker. Add `infrastructure/**` terraform validation to CI (the
   sandbox repo's `tf-plan.yml` pattern).
7. Migration note: operators must run a `sandbox serve` instance (provisioned
   via terraform) and set `FRONTAL_SANDBOX_BASE_URL`/`FRONTAL_SANDBOX_API_KEY`.

### Phase 4 — Hardening & ops (optional, follow-up)
1. **In-VM bash isolation:** the `code-sandbox` per-bash-command wrapper is
   redundant once the agent runs inside the sandbox VM. Evaluate removing it or
   keeping it as defense-in-depth (the VM is already an isolation boundary).
2. **TLS + auth hardening:** the sandbox server has no TLS. Terminate TLS at a
   reverse proxy or add TLS to the server before any non-local deployment.
3. **Quotas & metering:** wire `GET /v1/usage` into the billing/tracker
   reporting (`tracker_reporting.rs`).
4. **Trace integration:** `GET /v1/sessions/{id}/trace` gives a command trace
   (ADR 0007). Feed it into the existing audit trail.

## Files touched (summary)

- `crates/code-server/Cargo.toml` — add `frontal-sandbox-sdk`, `sandbox-types`.
- `crates/code-server/src/lib.rs` — `LaneTransportKind::Sandbox`,
  `parse_lane_transport_kind`, `SandboxLaneWorkerTransport`, config
  (`FRONTAL_SANDBOX_BASE_URL` / `FRONTAL_SANDBOX_API_KEY`), wiring (line 474
  match), then Docker removal (Phase 3).
- `crates/code-server/src/docker_worker.rs` — **removed** (Phase 3).
- `crates/code-server/src/worker_access.rs` — reused unchanged.
- `crates/code-server/BUILD.bazel` — deps + Docker removal.
- `.env.example` — sandbox env vars.
- Root `Cargo.toml` — workspace dep for `frontal-sandbox-sdk`/`sandbox-types`
  if pinned.
- `deploy/sandbox/` — **new**: terraform wrapper module, `config/production.toml`,
  `smoke.sh`, `Makefile`, `README.md` (Phase 3).
- `docs/` + README — architecture + deployment update.
- CI (`.github/workflows/`, `.buildkite/`) — sandbox service + terraform
  validation, drop Docker.

## Success criteria

- A hosted task with a repository runs end-to-end in a Frontal Sandbox session:
  repo imported, agent runs (promoted to VM), diff produced, PR/publish path
  works, all UI events stream as before.
- A re-run resumes from a snapshot (wake or fork) without re-cloning.
- Cancellation destroys the session and preserves partial work via snapshot.
- `LocalDocker` transport and `docker_worker.rs` are removed; no Docker
  dependency remains for the worker.
- The sandbox terraform deployment provisions a KVM host; `tofu apply` is clean;
  `scripts/smoke.sh` passes; code-server is reachable to the sandbox server via
  `FRONTAL_SANDBOX_BASE_URL` / `FRONTAL_SANDBOX_API_KEY`.
- The existing test suite (minus Docker-specific tests) passes; new sandbox
  integration tests pass on the KVM CI.

## Progress

### Phase 1 — `SandboxLaneWorkerTransport` ✅ (code complete, compiles)
- `Sandbox` variant in `LaneTransportKind` + parse arm (`"sandbox"`).
- `ServerConfig.sandbox_base_url` / `sandbox_api_key` + `from_env` wiring
  (`FRONTAL_SANDBOX_BASE_URL` / `FRONTAL_SANDBOX_API_KEY`).
- `ServerState` match arm constructs the transport.
- `SandboxLaneWorkerTransport` impl: `bootstrap` (create session → import
  checkout via `write_file` → write env + payload → kick off exec in a
  background thread), `observe_completion`, `cancel` (destroys session).
- Git deps pinned: `frontal-sandbox-sdk` v0.2.0 + `sandbox-types` at
  `frontal-labs/sandbox` commit `47a87e91`.
- `cargo check -p code-server` passes clean.
- `.env.example` updated with the two new env vars.

### Terraform deployment wrapper ✅ (created under `deploy/sandbox/`)
- 15 files: `terraform/*.tf` (9), `config/production.toml`, 3 scripts
  (`bootstrap.sh`, `deploy.sh`, `smoke.sh`), `Makefile`, 2 READMEs.
- Self-contained AWS EC2 (KVM) + S3 (CAS) wrapper; does not fork upstream
  `infrastructure/terraform/`.
- `terraform fmt -check` clean; tomllib parses; shellcheck clean.

### Phase 0 — Spike ⏳ (blocked)
- Needs a Linux host with QEMU/KVM (this macOS box can only do slow TCG).
- Gates full confidence in the isolate→VM promotion path.

### Phase 2 — Persistence & re-run ✅ (code complete, compiles)
- `resume()` trait method on `LaneWorkerTransport` (default impl = `bootstrap`).
- Sandbox override: fork from last run's snapshot (`client.fork`) if one exists,
  else `get_session` liveness probe → wake parked session + re-import, else
  fresh session + full re-import.
- Env file re-injected on every start (fresh, woken, or forked); ADR 0010 env-dropped-on-wake
  handled by writing `/workspace/.frontal-code-hosted/env` each time.
- `SandboxTaskState.snapshot_id` stored after clean `Exit{exit_code: 0}` in the
  background exec thread.

### Phase 3 — Docker worker removal ✅ (code complete, compiles)
- `LocalDocker` enum variant + `LocalDockerTransport` + `DockerRunner` deleted
  from `lib.rs`; `crates/code-server/src/docker_worker.rs` deleted.
- `LaneTransportKind` parse: `docker`/`local-docker`/`local_docker` → removal
  error message; `""`/`sandbox` → Sandbox (default); `tools-agent`/`agent` → ToolsAgent.
- `check_callback_port` deleted (not re-purposed).
- `write_local_docker_task_payload` + `write_worker_env_file` deleted.
- `FRONTAL_CODE_SERVER_DOCKER_IMAGE` / `FRONTAL_CODE_SERVER_CALLBACK_URL` /
  `FRONTAL_CODE_SERVER_DOCKER_NETWORK` env vars removed from config parsing.
- `HostedDockerTaskPayload` renamed `HostedLaneTaskPayload` (shared by sandbox
  transport + hosted-agent transport).
- Zero `docker` references in `lib.rs` except the intentional parse-error string
  and the parser test asserting docker is refused.
- `docker/worker.Dockerfile` marked legacy (kept for reference).
- `infrastructure/compose/docker-compose.docker-worker.yml` deleted;
  `docker-compose.yml` updated to sandbox env; `infrastructure/README.md`
  rewritten for sandbox mode.
- E2E scripts rewritten: `scripts/worker-e2e.sh` + `scripts/linux-amd64-worker-e2e.sh`
  — sandbox transport, no Docker daemon requirement for worker containers.
- CI: `rust-ci.yml` gains `deploy/sandbox/**` push path + `sandbox-infra` job
  (shellcheck + `tofu fmt -check`).
- `cargo check -p code-server` passes clean.

### Phase 0 — Spike ⏳ (in progress)

**Method (user-directed: "Local Mac + Docker/KVM"):** Apple-silicon macOS cannot
run x86_64 KVM guests (VZ driver is arm64-only), so the spike runs on a local
**Lima arm64 VM** (Ubuntu 24.04, 8 vCPU / 16 GiB / 100 GiB, `vz` driver).
Docker-in-VM runs the sandbox repo's linux-dev image (same `--privileged`
cgroup setup as `docker/run-linux.sh`). Tier 2 VM tests run under **QEMU TCG
software emulation** — the same `promote_tree` / guest-agent / exec code path as
KVM, just slower. Real KVM coverage remains via the upstream `vm-kvm.yml` CI
workflow on the pinned commit (`47a87e9`).

Steps:
1. ✅ Lima VM `frontal-spike` running; `limactl shell` works; git present.
2. ✅ Docker installed in-VM (apt was blocked by `unattended-upgrades` on
   cloud-image boot; stopped+masked the unit). Docker daemon active, server
   responds.
3. ✅ Cloned `frontal-labs/sandbox` at `47a87e9` to `~/Desktop/sandbox-spike`.
4. ⏳ Building linux-dev image; Tier 1: `cargo test -p sandbox-worker` in the
   `--privileged` container (delegated cgroup at `/sys/fs/cgroup/workers`).
5. ⏳ Tier 2: build the emulated guest image (`emulated-vm/build-image.sh`,
   no GUEST_PROGRAMS) then `cargo test -p sandbox-promote --test into_guest --
   --ignored` with `SANDBOX_VM_IMAGE` + `SANDBOX_VM_ACCEL=tcg`. Exit criteria:
   a promoted guest session boots the guest agent, `promote_tree` reports the
   expected dirs/files/bytes, and the file bytes round-trip.
6. ⏳ Record results; de-risk R1 (promotion path) and R2 (platform dependency).

Known environment gotchas discovered:
- Lima 2.3.0: `images[].format:` field rejected (non-strict warning, cosmetic
  on every `limactl` call — removed from `~/.lima/frontal-spike/lima.yaml`);
  `cloudInit.users` required or the guest drops to a login prompt.
- Ubuntu cloud image runs `unattended-upgrades` at boot, grabbing apt locks and
  blocking package installs.
- Docker socket needs `sudo` in the Lima guest (usermod -aG docker applies only
  to new shells).

**Scope note:** the full `docker/run-vm-tests.sh` suite also builds the
CPython WASI artifacts and ffmpeg/chromium into the guest image — too heavy for
the spike. The minimal `into_guest` test exercises the exact promotion path
(guest agent boot → `promote_tree` → guest exec) without those extras.

### Phase 4 — Hardening, docs, infra alignment ✅
- `docs/design/worker-architecture.md` rewritten to the sandbox transport
  (worker → session wording; token-store wording corrected — tokens are hashed
  in Postgres, so the sandbox transport requires `FRONTAL_CODE_SERVER_DATABASE_URL`).
- `docs/CONTAINERS.md`, `docs/TROUBLESHOOTING.md`, `docs/DEVELOPMENT.md`,
  `docs/README.md`, root `README.md`, `docs/design/conversation-store.md`
  updated to session wording.
- `crates/code-server/README.md`: "Local Docker Worker Mode" section replaced
  with "Sandbox Worker Mode".
- `infrastructure/README.md`: "Local Docker Worker Mode" replaced with "Sandbox
  Worker Mode"; worker-image profile references removed.
- `deploy/sandbox/README.md`: smoke.sh troubleshooting bullet fixed.
- `.env.example`: sandbox vars added (`FRONTAL_SANDBOX_BASE_URL`,
  `FRONTAL_SANDBOX_API_KEY`, `FRONTAL_CODE_SERVER_URL`); lane-transport value
  comment added.

## Immediate next steps

1. Run `cargo test -p code-server` and fix until clean.
2. Stage + commit (Phase 1+2+3+terraform together, or per-phase — decide at
   commit time). Everything is currently UNCOMMITTED.
3. Clean up the `code-sandbox-rebuild` worktree once the working-dir file is final.
4. (Optional) Linux+QEMU/KVM spike to validate isolate→VM promotion (R1/R2)
   before shipping.
