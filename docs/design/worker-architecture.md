# Hosted worker architecture

Status: implemented for the `local-docker` lane transport, verified end to end on 2026-11-08.

This document describes how a hosted task runs an agent, where each credential lives, what the worker can reach, and what is still open. Read it before changing the lane transport, the publish path, the token store, or the Docker launch arguments.

## Components

| Component | Where it runs | Holds |
|---|---|---|
| `code-server` (control plane) | Host process | Provider keys, `GITHUB_TOKEN`, control-plane API key, Postgres credentials. Publishes branches. |
| Postgres | Host or private network | Worker tokens (hashed), with expiry and revocation. |
| ClickHouse | Host or private network | Reserved for analytics and audit events. Not used by worker tokens. |
| Relay container `frontal-code-relay` | Docker, on the host bridge and on each task network | Nothing. It forwards one TCP port to the server. |
| Task network `frontal-task-<id>` | Docker, `--internal` | Only the task's worker and the relay. No internet, no host. |
| Worker container | Docker, one per task | A per-task token and a proxy URL. The checkout, mounted at `/workspace`. |

## Credential flow

```
              issue token (hashed in Postgres)
  code-server ───────────────────────────────────► hosted_worker_tokens
      │
      │ creates worker container with:
      │   FRONTAL_CODE_SERVER_API_KEY = task token
      │   OPENAI_API_KEY (or the matching key var) = task token
      │   OPENAI_BASE_URL = http://frontal-relay:8788/v1/model-proxy/openai
      │   FRONTAL_CODE_SERVER_URL     = http://frontal-relay:8788
      ▼
  worker ──► relay ──► code-server /v1/model-proxy/openai/... ──► provider
                        (drops the token, adds OPENAI_API_KEY)
  worker ──► relay ──► code-server /v1/tasks/<id>/publish (patch)
                        └─► fresh clone, apply, commit, push with GITHUB_TOKEN
```

What a worker can hold:

- A token that works for that task's endpoints and for the model proxy.
- The relay address and the proxy URL.
- The checkout, and the task payload file, which contains the prompt and no keys.

What a worker never holds: provider keys, `GITHUB_TOKEN`, the control-plane API key, or Postgres credentials.

## Token lifecycle

- **Storage:** Postgres table `hosted_worker_tokens`. The table holds the SHA-256 of the token, never the token. Each row has a task id, an expiry, and an optional revocation time.
- **Issue:** at bootstrap. A new token for a task revokes that task's previous token.
- **Use:** on each request. The worker may call only: read the task, read its runtime, read and write its GitHub integration, publish, and complete. Those endpoints are listed in `worker_access.rs`, and everything else returns 401.
- **Revoke:** on completion, on cancel, and on expiry (`FRONTAL_CODE_WORKER_TOKEN_TTL_SECS`, default 7200).
- **Restart:** tokens survive a restart because they are in Postgres. A running worker keeps working.

The server does not hold worker tokens in memory. The Postgres client runs on a dedicated thread, so the synchronous lane transport and the async handlers share one store.

## Publication

The worker never pushes. At the end of a run it computes a patch against the commit the server recorded at bootstrap, excluding its own state directories, and posts it to `/v1/tasks/<id>/publish`. The server then:

1. Refuses the patch if it is larger than 16 MiB, is empty, or touches `.git`, `.github/workflows`, or `.github/actions`, or creates a symbolic link or submodule.
2. Refuses the branch unless it starts with `frontal-code/`.
3. Clones the repository into a scratch directory the worker cannot reach.
4. Applies the patch with `git apply --index`, so a patch against a different base fails loudly.
5. Commits with a fixed author, and pushes to that branch.
6. Opens a pull request if a GitHub integration is configured. Failure here does not undo the push.
7. Records the branch and commit on the task.

Git runs with `core.hooksPath=/dev/null`, no system or user configuration, and no terminal prompts. The GitHub token reaches git through `GIT_CONFIG_*` environment variables, so it never appears in argv, a URL, or `.git/config`. Error messages are scrubbed of the token.

Trust boundary: the worker's `.git` directory is never read by the server. The server clones its own copy from the configured repository. A malicious worker can therefore change only the patch, and the patch is checked.

## Isolation of the worker

Each layer is independent. Removing one does not remove the others.

1. **Network.** Each task has its own `--internal` network. Containers on it have no route to the internet or to the host. The only peer is the relay, which forwards one port to the server. Inter-container traffic is allowed on task networks, but a task network holds only its own worker and the relay, so workers cannot reach each other.
2. **Capabilities.** `--cap-drop ALL` and `--security-opt no-new-privileges:true`. The worker runs as the unprivileged `frontal-code` user.
3. **Filesystem.** `--read-only` root. Writes go to the checkout (the only bind mount) and to two tmpfs mounts for `/tmp` and the home directory, both `nosuid,nodev`.
4. **Resources.** `--pids-limit 512`, `--memory 4g`, `--cpus 2`, `nofile` limits.
5. **Credentials.** No `GITHUB_TOKEN`, no provider key, no control-plane key. The environment is passed through a `0600` file that is deleted after `docker run` reads it, so no value appears in the process list.
6. **Relay.** Read-only, no capabilities, forwards only to `host.docker.internal:<server port>`. It is not a general proxy.

The Docker daemon socket is never mounted. Containers are never privileged. The host network is never used.

## Permission mode

Workers run in `workspace-write` by default. Writes to the checkout are allowed. Shell commands are allowed only when an allow rule grants them: read and build commands such as `ls`, `cat`, `rg`, `git status`, `cargo`, and `npm test`. Network tools, `git push`, `env`, `rm -rf`, and `docker` are denied, and deny rules from the repository apply as well. A task can ask for `danger-full-access` explicitly. The container is the boundary, and the network layer removes the main exfiltration path: a compromised agent cannot reach an arbitrary host. The remaining channels are the model proxy, which the provider receives every prompt through anyway, and the relay, which reaches only the server.

The allow list is the constant `WORKER_BASH_ALLOW` in `crates/code-server/src/worker_access.rs`, and the deny list is `WORKER_BASH_DENY`. The CLI applies them to the worker. A repository's own allow rules are replaced by the worker's, so a repository cannot widen what a worker may run.

## Threat model

| Threat | Mitigation |
|---|---|
| Agent exfiltrates a provider key | Keys never enter the container. |
| Agent exfiltrates `GITHUB_TOKEN` | Not in the container, for any task. |
| Agent uses the worker token against other tasks | The token is scoped to its task, and the endpoint list is fixed. |
| Agent uses the worker token to cancel or create tasks | Those endpoints are not in the allowed list. |
| Agent reaches the internet to fetch or send data | No route: the task network is internal. |
| Agent reaches other workers or the host | Separate task networks. The relay forwards one port. |
| Agent changes the repository's `.git` to redirect a push | The server never reads the checkout's `.git`. It clones its own copy. |
| Agent adds a workflow that runs with CI credentials | `.github/workflows` and `.github/actions` are refused. |
| Agent writes a symlink to read host files on publish | Symlinks and submodules are refused. |
| Agent pushes to `main` | Only branches under `frontal-code/` are accepted. |
| Agent consumes all host resources | PID, memory, and CPU limits. |
| Token database is read by an attacker | Only hashes are stored. |
| Token replayed after completion | Revoked on completion, cancel, and expiry. |
| Server restart mid-task | Tokens persist. The task continues. |

## Decisions and remaining limits

Resolved:

1. **Permission default.** Workers run in `workspace-write`. Shell commands run only when an allow rule grants them: a list of read and build commands (`WORKER_BASH_ALLOW` in `worker_access.rs`). Network tools, `git push`, `env`, and `rm -rf` are denied (`WORKER_BASH_DENY`). A task can request `danger-full-access` explicitly. The worker's allow rules replace any the repository would set, and deny rules from both sources apply.
2. **GitHub token scope.** Publishing with a token is refused for any repository not named in `FRONTAL_CODE_GITHUB_ALLOWED_REPOS` (`owner/repo` or `owner/*`), and the check runs before any clone. An empty allowlist with a token set means no GitHub publication. Pull requests use the REST API directly. Use a fine-grained token with only Contents and Pull requests write access on the named repositories.
3. **Postgres TLS.** Verified TLS by default, against system roots plus `FRONTAL_CODE_SERVER_DATABASE_CA_FILE` for a private CA. Plaintext only to loopback with `sslmode=disable`. Verified against a private-CA Postgres 17 on 2026-11-08.
4. **Relay image.** Pinned by digest (`DEFAULT_RELAY_IMAGE`). Override with `FRONTAL_CODE_WORKER_RELAY_IMAGE` after reviewing the image.
5. **ClickHouse audit.** Token issue and revocation, worker launches, refused and failed publications, and publications are written to `frontal_audit_events_v2` as epoch milliseconds. Writes are best effort and never block a request. Verified against a live ClickHouse 26.3.

Still open:

- **Model access.** All live runs (worker, SDK, and CLI) use a local Ollama server with `qwen2.5:1.5b`, so they have no quota. Cloud gateways are optional and were checked separately.
- **Linux hosts.** The code-server and CLI test suites pass on `linux/amd64` (Rosetta, `rust:1.95-bookworm`), including the Postgres tests over verified TLS. code-server also ran inside that container, driving the host Docker daemon through the socket, and two Ollama tasks completed and published. That run used Docker Desktop, so a native Linux Docker host (where `host-gateway` is the real host bridge) is still unverified.
- **One server per Docker host.** The relay is one container, and a second server on the same host would replace it.
- **Finished-task networks.** A finished task's network is removed by the next launch's sweep, not immediately.
- **Parity harness.** The `mock_parity_harness` bash-approval scenario failed once under full workspace load and has not been reproduced since, including a full run on `linux/amd64`. Stderr is now reported on failure.

## Operating it

Required for `local-docker` lanes:

- `FRONTAL_CODE_SERVER_DATABASE_URL` — Postgres for worker tokens.
- `FRONTAL_CODE_SERVER_API_KEY` — control-plane key (required unless anonymous mode is set).
- `FRONTAL_CODE_SERVER_CALLBACK_URL` — the server's address as seen from the host, for example `http://host.docker.internal:8788`. Its port is the relay's port.

Ollama needs no key. The server forwards `ollama` requests to `OLLAMA_BASE_URL` on the host (default `http://localhost:11434`). The worker receives the proxy URL as its `OLLAMA_BASE_URL`, and its task token as `OLLAMA_API_KEY`, which the proxy reads to identify the task and never forwards. Use a model the host has pulled, for example `qwen2.5:1.5b`. Models below about 1.5B parameters did not emit tool calls in testing.

Optional:

- `FRONTAL_CODE_SERVER_DATABASE_CA_FILE` — CA for a private Postgres certificate
- `FRONTAL_CODE_SERVER_CLICKHOUSE_URL`, `FRONTAL_CODE_SERVER_CLICKHOUSE_USER`, `FRONTAL_CODE_SERVER_CLICKHOUSE_PASSWORD` — audit events (HTTPS, or loopback)
- `FRONTAL_CODE_GITHUB_ALLOWED_REPOS` — required when `GITHUB_TOKEN` is set
- `FRONTAL_CODE_GITHUB_API_BASE` — default `https://api.github.com`
- `FRONTAL_CODE_WORKER_TOKEN_TTL_SECS` (default 7200)
- `FRONTAL_CODE_WORKER_RELAY_IMAGE` (default `alpine/socat:latest`)
- `FRONTAL_CODE_GIT_AUTHOR_NAME`, `FRONTAL_CODE_GIT_AUTHOR_EMAIL` — author of published commits
- `GITHUB_TOKEN` — server-side only, used to push and open pull requests

Verification: `scripts/worker-e2e.sh` runs one task end to end, checks the worker's isolation settings and environment names while it runs, and checks that the branch reached the remote.
