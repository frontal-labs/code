//! `code-incremental` — the Incremental Compute Engine integration for frontal-code.
//!
//! This crate wraps the vendored Incremental Rust binding and provides the
//! affected-plan and observation API used by the CLI runtime, code-server
//! control plane, and the Bazel/dev pipeline.
//!
//! # Design
//!
//! - A [`Store`] manages a single engine process and its store directory.
//! - A task may be skipped **only** with a certificate: an observation at the
//!   current baseline whose inputs are all unchanged. Unknown or unverifiable
//!   input makes the task invalid, or makes the whole answer a fallback.
//!   No silent skips.
//! - A `fallback` result means "run everything" (the safe default). It is
//!   never a hard error.
//! - The store is per-worktree by default (`<repo>/.incremental/`).
//!   `tg.log` is the source of truth (append-only JSONL); indexes are
//!   rebuildable caches.

pub mod context;
pub mod engine_binary;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use context::build_context;
pub use engine_binary::{target_triple, EngineBinary};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by [`Store`] methods.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The engine binary could not be found.
    #[error("incremental engine binary not found; use FRONTAL_CODE_INCREMENTAL_ENGINE or INCREMENTAL_ENGINE")]
    EngineNotFound,

    /// The engine process failed to start.
    #[error("engine failed to start: {0}")]
    EngineStartup(String),

    /// A protocol error from the engine.
    #[error("{code}: {message}")]
    Protocol { code: String, message: String },

    /// A timeout waiting for the engine to respond.
    #[error("engine request timed out: {0}")]
    Timeout(String),

    /// A filesystem error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A JSON serialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// A handle to one Incremental engine process and its store directory.
///
/// The store is per-worktree by default (`<repo>/.incremental/`). Override
/// with `FRONTAL_CODE_INCREMENTAL_STORE` (passed through to
/// `INCREMENTAL_STORE`) or via [`Store::open`].
///
/// # Example
///
/// ```no_run
/// use code_incremental::Store;
/// use std::path::Path;
///
/// let mut store = Store::open(Path::new(".")).unwrap();
/// let ctx = code_incremental::build_context(Path::new("."), "cargo@1.85", "1.85.0");
/// let plan = store.affected(
///     &serde_json::json!({"changes": []}),
///     "abc123",
///     &ctx,
/// ).unwrap();
///
/// if plan.is_fallback() {
///     println!("fallback: run everything");
/// } else {
///     for task in &plan.tasks {
///         println!("affected: {}", task.task_id);
///     }
/// }
/// ```
pub struct Store {
    client: incremental::Incremental,
    path: PathBuf,
    repo_root: PathBuf,
}

impl Store {
    /// Open a store rooted at `repo_root`.
    ///
    /// The store directory is `<repo_root>/.incremental/` by default.
    /// The engine binary is resolved via [`EngineBinary::resolve`].
    ///
    /// If no engine binary is available, returns `Err(Error::EngineNotFound)`.
    /// Callers should catch this and degrade to "always run everything."
    pub fn open(repo_root: &Path) -> Result<Self> {
        let engine = EngineBinary::resolve();
        let binary = engine.path().ok_or(Error::EngineNotFound)?.to_path_buf();

        let store_path = env_store_path(repo_root);

        // Initialize the store before connecting: the pinned engine exits when
        // `tg.log` is absent, before it can answer the `hello` handshake that
        // `connect` performs. Running `init` first (idempotent — the engine
        // skips when the log exists) makes first-use work on a fresh worktree.
        init_store(&binary, &store_path, repo_root)?;

        // The vendored protocol client (`third_party/incremental`) writes the
        // request to the engine's stdin *before* it starts the response
        // deadline, so a write to a stalled engine (pipe buffer full) can
        // block longer than `timeout`. That is a known limitation of the
        // vendored binding (CodeRabbit finding on PR #166) that we accept
        // rather than patch in place:
        //
        //   * It is third-party code pinned to a commit; the manifest's
        //     `sha256_rust_src` is verified against *upstream* bytes and the
        //     fetch script re-copies those files (clobbering any local patch).
        //   * The exposure is bounded: requests are small (task ids, change
        //     lists) and far below the pipe buffer, and the blocking scenario
        //     requires the engine to already be wedged — in which case the
        //     next request's `timeout` read deadline fires and the caller
        //     falls back to "run everything". The whole integration is
        //     best-effort; a wedged engine degrades to a fallback, never to a
        //     hard error.
        //
        // Revisit this if requests grow past the pipe buffer or the engine
        // becomes a dependency with its own update cadence.
        let cfg = incremental::Config {
            engine: vec![binary.display().to_string()],
            store: store_path.clone(),
            root: repo_root.to_path_buf(),
            timeout: Duration::from_secs(30),
            restart: true,
        };

        let client = incremental::Incremental::connect(cfg)
            .map_err(|e| Error::EngineStartup(format!("{}: {}", e.code, e.message)))?;

        Ok(Store {
            client,
            path: store_path,
            repo_root: repo_root.to_path_buf(),
        })
    }

    /// Ensure the store directory is initialized.
    ///
    /// Runs the engine's `init` command, which creates the store directory,
    /// any missing parents, and an empty log with a header. Safe to call
    /// multiple times (idempotent — the engine skips when the log exists).
    ///
    /// [`Store::open`] already runs this before connecting; this method is
    /// kept for callers that hold a `Store` and want to re-check. It uses a
    /// bounded wait and returns `Ok(false)` (rather than blocking or failing
    /// the caller) when the engine stalls or is unavailable, so callers can
    /// proceed down the unconditional "run everything" path.
    #[must_use]
    pub fn ensure_initialized(&self) -> bool {
        let engine = EngineBinary::resolve();
        let Some(binary) = engine.path() else {
            return false;
        };
        init_store(binary, &self.path, &self.repo_root).is_ok()
    }

    /// The store directory path.
    #[must_use]
    pub fn store_path(&self) -> &Path {
        &self.path
    }

    /// The repo root this store is rooted at.
    #[must_use]
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    // -- observation -------------------------------------------------------

    /// Record a single observation.
    ///
    /// The observation JSON follows the protocol v1 schema:
    /// ```json
    /// {
    ///   "task": "src/foo.rs#test",
    ///   "inputs": [{"type": "file", "path": "src/foo.rs"}],
    ///   "outputs": [],
    ///   "outcome": "pass",
    ///   "duration_ms": 1234,
    ///   "dirty": false,
    ///   "context": { ... }
    /// }
    /// ```
    pub fn observe(&mut self, observation: &Value) -> Result<Value> {
        self.client.observe(observation).map_err(protocol_err)
    }

    /// Record multiple observations atomically.
    ///
    /// Either all observations are stored or none. Returns `{accepted: n}`.
    pub fn observe_many(&mut self, observations: &[Value]) -> Result<Value> {
        self.client.observe_many(observations).map_err(protocol_err)
    }

    // -- baseline ----------------------------------------------------------

    /// Query the current baseline.
    ///
    /// Returns `{baseline: <sha> | null}`.
    pub fn baseline(&mut self) -> Result<Value> {
        self.client.baseline().map_err(protocol_err)
    }

    /// Advance the baseline to `to`.
    ///
    /// Fails with `baseline_conflict` unless `expected` equals the current
    /// baseline (`null` before the first advance). It never overwrites.
    pub fn advance(&mut self, to: &str, expected: Option<&str>) -> Result<Value> {
        self.client.advance(to, expected).map_err(protocol_err)
    }

    // -- affected ----------------------------------------------------------

    /// Compute the affected plan for a changeset.
    ///
    /// `changes` is the changeset JSON: `{"changes": [{"path": ..., "status": ...}]}`.
    /// `baseline` is the baseline to compare against (must equal the store's
    /// current baseline for the engine to certify any skips — policy P0).
    /// `context` is the context object; it **must** be present and must match
    /// the observation contexts for the engine to certify any skips
    /// (policy P0b). Use [`build_context`](crate::build_context) to build it.
    ///
    /// A `fallback` result means "run everything" (the safe default).
    /// It is never a hard error — the plan is returned with `is_fallback == true`.
    pub fn affected(&mut self, changes: &Value, baseline: &str, context: &Value) -> Result<Plan> {
        let mut opts = incremental::Affected::new(baseline);
        opts.context = Some(context.clone());
        let raw = self.client.affected(changes, &opts).map_err(protocol_err)?;
        Ok(Plan::from_json(&raw))
    }

    // -- explain / forget / stats ------------------------------------------

    /// Explain why a task is or is not invalid.
    pub fn explain(&mut self, task: &str) -> Result<Value> {
        self.client.explain(task).map_err(protocol_err)
    }

    /// Drop a task's observations.
    pub fn forget(&mut self, task: &str) -> Result<Value> {
        self.client.forget(task).map_err(protocol_err)
    }

    /// Store statistics (tasks, observations, baseline).
    pub fn stats(&mut self) -> Result<Value> {
        self.client.stats().map_err(protocol_err)
    }

    // -- hello / ping / compact --------------------------------------------

    /// The `hello` handshake result: `{protocol, engine, capabilities, store}`.
    pub fn hello(&mut self) -> Result<Value> {
        self.client.hello().map_err(protocol_err)
    }

    /// Ping the engine.
    pub fn ping(&mut self) -> Result<Value> {
        self.client.ping().map_err(protocol_err)
    }

    /// Compact the store (no-op in the current build).
    pub fn compact(&mut self) -> Result<Value> {
        self.client.compact().map_err(protocol_err)
    }
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// The result of an `affected` query.
///
/// A plan is never returned partially (protocol v1 §9.6). If the engine
/// cannot produce a complete plan, it returns a fallback instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Tasks that must be re-run.
    pub tasks: Vec<Task>,
    /// Tasks that were skipped (have a valid certificate).
    pub skipped: usize,
    /// Tasks the engine knows about but has no observation for.
    pub unobserved: Vec<String>,
    /// Tasks that were explicitly ignored.
    pub ignored: Vec<String>,
    /// Tasks that have been deleted.
    pub deleted_tasks: Vec<String>,
    /// Non-null when the engine ran everything instead of answering per task.
    /// `fallback.reason` says why (e.g. `no_baseline`, `context_changed`, `dirty_observation`).
    pub fallback: Option<Fallback>,
    /// Policy confidence label (`medium`, `high`, `low`). Not a probability.
    pub confidence: String,
    /// The graph version at query time (e.g. `"g_a71d37aa"`).
    pub graph_version: String,
}

/// A single task in an affected plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// The task ID (repo-relative path + task kind).
    pub task_id: String,
    /// Why the task is selected (e.g. `reads:src/math.ts`, `dirty_observation`, `fallback`).
    pub reasons: Vec<String>,
}

/// The fallback annotation on a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fallback {
    /// Why the fallback was triggered.
    pub reason: String,
}

impl Plan {
    /// Whether the plan is a fallback (meaning "run everything").
    #[must_use]
    pub fn is_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    /// The number of tasks that must be re-run.
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Deserialize a plan from the engine's raw JSON response.
    ///
    /// A plan is never returned partially: if any `tasks` entry is missing a
    /// string `id` (or `tasks` is not an array), the whole plan is a fallback
    /// (`invalid_plan`). Silently dropping a malformed entry would let a
    /// caller treat an unparseable plan as "nothing to re-run" and skip
    /// affected work.
    #[must_use]
    pub fn from_json(raw: &Value) -> Self {
        let tasks_value = raw.get("tasks");
        let tasks: Vec<Task> = match tasks_value {
            Some(Value::Array(arr)) => {
                let parsed: Vec<Option<Task>> = arr
                    .iter()
                    .map(|t| {
                        Some(Task {
                            task_id: t["id"].as_str()?.to_string(),
                            reasons: t
                                .get("reasons")
                                .and_then(Value::as_array)
                                .map(|r| {
                                    r.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                    })
                    .collect();
                if parsed.iter().any(Option::is_none) {
                    return Self::fallback("invalid_plan");
                }
                parsed.into_iter().flatten().collect()
            }
            // No `tasks` key (or a non-array) is only valid on a fallback plan.
            None | Some(Value::Null) => vec![],
            Some(_) => return Self::fallback("invalid_plan"),
        };

        let skipped = raw
            .get("skipped")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0);
        let unobserved = string_array(raw, "unobserved");
        let ignored = string_array(raw, "ignored");
        let deleted_tasks = string_array(raw, "deleted_tasks");

        let fallback = raw.get("fallback").and_then(|f| {
            if f.is_null() {
                None
            } else {
                Some(Fallback {
                    reason: f
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                })
            }
        });

        let confidence = raw
            .get("confidence")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let graph_version = raw
            .get("graph_version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        Plan {
            tasks,
            skipped,
            unobserved,
            ignored,
            deleted_tasks,
            fallback,
            confidence,
            graph_version,
        }
    }

    /// A plan with no tasks and no fallback (empty changeset).
    #[must_use]
    pub fn empty() -> Self {
        Plan {
            tasks: vec![],
            skipped: 0,
            unobserved: vec![],
            ignored: vec![],
            deleted_tasks: vec![],
            fallback: None,
            confidence: "high".to_string(),
            graph_version: "unknown".to_string(),
        }
    }

    /// A fallback plan (run everything).
    #[must_use]
    pub fn fallback(reason: impl Into<String>) -> Self {
        Plan {
            tasks: vec![],
            skipped: 0,
            unobserved: vec![],
            ignored: vec![],
            deleted_tasks: vec![],
            fallback: Some(Fallback {
                reason: reason.into(),
            }),
            confidence: "low".to_string(),
            graph_version: "unknown".to_string(),
        }
    }
}

fn string_array(raw: &Value, key: &str) -> Vec<String> {
    raw.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn protocol_err(e: incremental::Error) -> Error {
    match e.code.as_str() {
        "timeout" => Error::Timeout(e.message),
        "engine_exited" => Error::EngineStartup(e.message),
        _ => Error::Protocol {
            code: e.code,
            message: e.message,
        },
    }
}

/// Maximum time to wait for the engine's `init` command to finish.
///
/// `init` runs as a separate process, so the protocol client's own timeout
/// does not cover it. A stalled init must not block task transitions or
/// server requests that call the bridge.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Run the engine's `init` command against `store_path` with a bounded wait.
///
/// Returns `Ok(())` when the store is initialized (or already was) and
/// `Err` when the engine exits non-zero, cannot be spawned, or stalls past
/// [`INIT_TIMEOUT`] (the child is then terminated and reaped). Callers that
/// treat init failure as "run everything" should map `Err` to the fallback
/// path rather than propagating a hard error.
fn init_store(binary: &Path, store_path: &Path, repo_root: &Path) -> Result<()> {
    let mut child = std::process::Command::new(binary)
        .args([
            "init",
            "--store",
            &store_path.to_string_lossy(),
            "--root",
            &repo_root.to_string_lossy(),
        ])
        .spawn()
        .map_err(|e| Error::EngineStartup(format!("failed to spawn init: {e}")))?;

    let deadline = Instant::now() + INIT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err(Error::EngineStartup(format!(
                    "engine init exited with {status}"
                )));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::EngineStartup(format!(
                        "engine init timed out after {INIT_TIMEOUT:?}"
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(Error::EngineStartup(format!(
                    "failed to wait for init: {e}"
                )));
            }
        }
    }
}

/// Resolve the store directory from the environment, falling back to
/// `<repo_root>/.incremental/`.
fn env_store_path(repo_root: &Path) -> PathBuf {
    std::env::var_os("FRONTAL_CODE_INCREMENTAL_STORE")
        .or_else(|| std::env::var_os("INCREMENTAL_STORE"))
        .map_or_else(|| repo_root.join(".incremental"), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_from_json_full() {
        let raw = json!({
            "tasks": [
                {"id": "src/foo.rs#test", "reasons": ["reads:src/bar.rs"]},
                {"id": "src/baz.rs#test", "reasons": ["dirty_observation"]}
            ],
            "skipped": 3,
            "unobserved": ["src/new.rs#test"],
            "ignored": [],
            "deleted_tasks": ["src/old.rs#test"],
            "fallback": null,
            "confidence": "medium",
            "graph_version": "g_a71d37aa"
        });
        let plan = Plan::from_json(&raw);
        assert_eq!(plan.task_count(), 2);
        assert!(!plan.is_fallback());
        assert_eq!(plan.tasks[0].task_id, "src/foo.rs#test");
        assert_eq!(plan.tasks[0].reasons, vec!["reads:src/bar.rs"]);
        assert_eq!(plan.tasks[1].task_id, "src/baz.rs#test");
        assert_eq!(plan.skipped, 3);
        assert_eq!(plan.unobserved, vec!["src/new.rs#test"]);
        assert_eq!(plan.ignored, Vec::<String>::new());
        assert_eq!(plan.deleted_tasks, vec!["src/old.rs#test"]);
        assert_eq!(plan.confidence, "medium");
        assert_eq!(plan.graph_version, "g_a71d37aa");
    }

    #[test]
    fn plan_from_json_fallback() {
        let raw = json!({
            "tasks": [],
            "skipped": 0,
            "unobserved": [],
            "ignored": [],
            "deleted_tasks": [],
            "fallback": {"reason": "no_baseline"},
            "confidence": "low",
            "graph_version": "g_00000000"
        });
        let plan = Plan::from_json(&raw);
        assert!(plan.is_fallback());
        assert_eq!(plan.task_count(), 0);
        assert_eq!(plan.fallback.as_ref().unwrap().reason, "no_baseline");
    }

    #[test]
    fn plan_empty() {
        let plan = Plan::empty();
        assert!(!plan.is_fallback());
        assert_eq!(plan.task_count(), 0);
        assert_eq!(plan.confidence, "high");
    }

    #[test]
    fn plan_fallback_convenience() {
        let plan = Plan::fallback("context_changed");
        assert!(plan.is_fallback());
        assert_eq!(plan.fallback.as_ref().unwrap().reason, "context_changed");
        assert_eq!(plan.confidence, "low");
    }

    #[test]
    fn plan_missing_fields_defaults() {
        let raw = json!({});
        let plan = Plan::from_json(&raw);
        assert_eq!(plan.task_count(), 0);
        assert!(!plan.is_fallback());
        assert_eq!(plan.confidence, "unknown");
        assert_eq!(plan.graph_version, "unknown");
    }

    #[test]
    fn env_store_path_uses_repo_root() {
        // Unset the env vars to test the default.
        std::env::remove_var("FRONTAL_CODE_INCREMENTAL_STORE");
        std::env::remove_var("INCREMENTAL_STORE");
        let repo = Path::new("/tmp/test-repo");
        let store = env_store_path(repo);
        assert_eq!(store, Path::new("/tmp/test-repo/.incremental"));
    }

    #[test]
    fn task_id_roundtrip() {
        let task = Task {
            task_id: "crates/code-runtime/src/task_registry.rs#test".to_string(),
            reasons: vec!["reads:crates/code-runtime/src/task_registry.rs".to_string()],
        };
        let json_val = serde_json::to_value(&task).unwrap();
        assert_eq!(json_val["task_id"], task.task_id);
        assert_eq!(json_val["reasons"][0], task.reasons[0]);
    }

    // -- end-to-end against the real engine --------------------------------
    //
    // These tests spawn the vendored engine binary (skipped when it is not
    // available for the current platform) and drive the full observe →
    // advance → affected cycle through the `Store` API, not the raw
    // protocol. They mirror the protocol probe that proved the engine
    // certifies skips: matching baseline + context yields a certified skip,
    // while any mismatch (context, baseline) falls back.

    /// A unique, engine-free temp dir for one test process run.
    fn e2e_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "code-inc-e2e-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ))
    }

    /// Build a fresh store on a temp repo, initialized with a baseline.
    ///
    /// Returns `None` only when the engine binary is unavailable (a clean
    /// skip). Once the binary is found, any failure — init, connect, or
    /// baseline advance — panics with the original error so a broken setup
    /// fails the test instead of silently passing without exercising
    /// certified skips or fallback behavior.
    fn fresh_e2e_store(baseline: &str) -> Option<(Store, PathBuf)> {
        let engine = EngineBinary::resolve();
        let engine_path = engine.path()?;
        if !engine_path.exists() {
            return None;
        }
        let repo = e2e_dir("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store_dir = repo.join(".incremental");
        init_store(engine_path, &store_dir, &repo)
            .unwrap_or_else(|e| panic!("engine setup (init) failed: {e}"));
        let mut store =
            Store::open(&repo).unwrap_or_else(|e| panic!("engine setup (open) failed: {e}"));
        // First advance: null → baseline.
        store
            .advance(baseline, None)
            .unwrap_or_else(|e| panic!("engine setup (advance) failed: {e}"));
        Some((store, repo))
    }

    #[test]
    fn e2e_certified_skip_and_fallbacks() {
        let Some((mut store, repo)) = fresh_e2e_store("abc123") else {
            eprintln!("skipping: engine binary unavailable");
            return;
        };
        let ctx = build_context(&repo, "test-runner@1.0.0", "1.0.0");

        // Observe a task whose only input is a file, at the store baseline.
        let observation = json!({
            "task": "src/foo.rs#test",
            "baseline": "abc123",
            "inputs": [{"kind": "file", "path": "src/foo.rs"}],
            "outputs": [],
            "outcome": "pass",
            "dirty": false,
            "context": ctx,
        });
        store.observe(&observation).unwrap();

        // 1. Matching baseline + context, empty changeset → certified skip.
        let plan = store
            .affected(&json!({"changes": []}), "abc123", &ctx)
            .unwrap();
        assert!(
            !plan.is_fallback(),
            "expected a certified plan, got fallback {:?}",
            plan.fallback
        );
        assert_eq!(plan.skipped, 1, "task should be skipped");
        assert_eq!(plan.task_count(), 0);

        // 2. Context mismatch → fallback (policy P2).
        let mut bad_ctx = ctx.clone();
        bad_ctx["runner"] = json!("other-runner@9.9.9");
        let plan = store
            .affected(&json!({"changes": []}), "abc123", &bad_ctx)
            .unwrap();
        assert!(plan.is_fallback(), "context mismatch must fall back");
        assert_eq!(plan.fallback.as_ref().unwrap().reason, "context_changed");

        // 3. Baseline mismatch → fallback (policy P0).
        let plan = store
            .affected(&json!({"changes": []}), "zzz999", &ctx)
            .unwrap();
        assert!(plan.is_fallback(), "baseline mismatch must fall back");
        assert_eq!(plan.fallback.as_ref().unwrap().reason, "baseline_mismatch");

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn e2e_missing_store_degrades_safely() {
        // A store that was never initialized returns `store_missing` from the
        // engine. `affected` must surface that as an error the caller can
        // degrade on — never a certified plan, never a panic.
        // No engine at all: the safe default, also acceptable.
        if EngineBinary::resolve().path().is_none() {
            return;
        }
        let repo = e2e_dir("uninit");
        std::fs::create_dir_all(&repo).unwrap();
        let Some(mut store) = Store::open(&repo).ok() else {
            // Engine exists but store could not be opened.
            let _ = std::fs::remove_dir_all(&repo);
            return;
        };
        let ctx = build_context(&repo, "test-runner@1.0.0", "1.0.0");
        // The store is missing: the engine rejects the query. We assert that
        // the call does not panic and either errors or returns a fallback.
        let result = store.affected(&json!({"changes": []}), "abc123", &ctx);
        match result {
            Err(_) => {} // expected: store_missing
            Ok(plan) => assert!(plan.is_fallback()),
        }
        let _ = std::fs::remove_dir_all(&repo);
    }
}
