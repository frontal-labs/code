//! Flag-gated observation bridge between the runtime task lifecycle and the
//! Incremental engine store.
//!
//! This module is the Phase-2 hook surface:
//! - **Observe**: task status transitions are recorded as protocol-v1
//!   observations so that a future `affected` query can certify skips.
//! - **Advance**: on terminal worker transitions, the store baseline is
//!   advanced so subsequent queries measure against the latest completed
//!   state.
//!
//! The bridge is **flag-gated** on `incremental.enabled` in
//! `.frontal-code/settings.json` and **best-effort**: every method swallows
//! errors so a missing engine binary or store directory never disrupts task
//! execution. The safe default — run everything — is preserved because
//! `affected` falls back to a full plan when the engine is unavailable.
//!
//! Task IDs follow the convention `<worktree_rel_path>#<task_kind>` (see the
//! integration plan). Callers are responsible for building a stable ID; this
//! module provides a helper for the common case.

use std::path::Path;

use code_incremental::{build_context, Store};
use serde_json::{json, Value};

/// The runner identifier embedded in every observation context.
const RUNNER: &str = "frontal-code-runtime";

/// The runtime node version embedded in the context. Bump when the
/// runtime changes in a way that affects task outcomes.
const NODE: &str = "0.3.0";

/// Build a stable task ID for a sub-agent task.
///
/// `rel_path` is the repo-relative path of the primary file the task
/// operates on; `kind` is the task kind (`test`, `build`, `run`, etc.).
#[must_use]
pub fn task_id(rel_path: &str, kind: &str) -> String {
    format!("{rel_path}#{kind}")
}

/// Map a terminal task status to the protocol-v1 outcome string.
#[must_use]
pub fn outcome_for(status: &str) -> &'static str {
    match status {
        "completed" => "pass",
        "failed" => "fail",
        "stopped" => "error",
        _ => "none",
    }
}

/// Lazily opened store. Re-opened on every call for now; the engine process
/// is short-lived and the cost is dominated by the JSONL append, not process
/// spawn. A process-pooling optimization is a follow-up.
///
/// Ensures the store is initialized (idempotent) so the first observation
/// on a fresh worktree does not hit `store_missing`.
fn open_store(repo_root: &Path) -> Option<Store> {
    let store = Store::open(repo_root).ok()?;
    store.ensure_initialized();
    Some(store)
}

/// Record a task status transition as a protocol-v1 observation.
///
/// # Arguments
///
/// * `repo_root` - repository root (used to locate the store).
/// * `id` - stable task ID (`<rel_path>#<kind>`).
/// * `inputs` - the protocol-v1 input list (files, dirs, probes) the task
///   consumed. Pass an empty slice when unknown; the engine will treat the
///   observation as non-certifiable but not dirty.
/// * `status` - the terminal task status (`completed`, `failed`, `stopped`).
/// * `dirty` - set true if the run crashed before inputs could be verified.
pub fn observe_task(repo_root: &Path, id: &str, inputs: &[Value], status: &str, dirty: bool) {
    let Some(mut store) = open_store(repo_root) else {
        return;
    };
    let context = build_context(repo_root, RUNNER, NODE);
    // The engine requires the observation's baseline to match the store
    // baseline for the observation to count as a certificate (policy P0).
    let obs_baseline = store
        .baseline()
        .ok()
        .and_then(|v| {
            v.get("baseline")
                .and_then(serde_json::Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default();
    let obs_baseline = if obs_baseline.is_empty() {
        "unknown".to_string()
    } else {
        obs_baseline
    };
    let observation = json!({
        "task": id,
        "baseline": obs_baseline,
        "inputs": inputs,
        "outputs": [],
        "outcome": outcome_for(status),
        "dirty": dirty,
        "context": context,
    });
    let _ = store.observe(&observation);
}

/// Record a batch of observations (e.g. all task transitions for a batch).
pub fn observe_many(repo_root: &Path, observations: &[Value]) {
    let Some(mut store) = open_store(repo_root) else {
        return;
    };
    let _ = store.observe_many(observations);
}

/// Advance the store baseline to `to`. Called after a worker finishes a
/// session so that the next `affected` query measures against the
/// post-completion state.
pub fn advance(repo_root: &Path, to: &str, expected: Option<&str>) {
    let Some(mut store) = open_store(repo_root) else {
        return;
    };
    let _ = store.advance(to, expected);
}

/// Build the observation context for this repo (useful for constructing
/// `affected` queries from the runtime).
#[must_use]
pub fn context(repo_root: &Path) -> Value {
    build_context(repo_root, RUNNER, NODE)
}

/// Query the store for tasks affected by the given changeset relative to
/// `baseline`. Returns `None` if the store is unavailable or the query
/// falls back (caller should then run everything).
#[must_use]
pub fn affected(repo_root: &Path, baseline: &str, changes: &Value) -> Option<Vec<String>> {
    let mut store = open_store(repo_root)?;
    // The engine requires a context object on every `affected` query to
    // certify skips (policy P0b). Build it the same way observations do.
    let ctx = build_context(repo_root, RUNNER, NODE);
    let plan = store.affected(changes, baseline, &ctx).ok()?;
    if plan.is_fallback() {
        return None;
    }
    Some(plan.tasks.iter().map(|t| t.task_id.clone()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_is_stable() {
        assert_eq!(task_id("crates/foo.rs", "test"), "crates/foo.rs#test");
    }

    #[test]
    fn outcome_mapping() {
        assert_eq!(outcome_for("completed"), "pass");
        assert_eq!(outcome_for("failed"), "fail");
        assert_eq!(outcome_for("stopped"), "error");
        assert_eq!(outcome_for("created"), "none");
    }

    #[test]
    fn context_is_deterministic() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let a = context(root);
        let b = context(root);
        assert_eq!(a, b);
    }

    #[test]
    fn observe_task_does_not_panic_without_store() {
        // No store exists at this path; observe_task must swallow the error.
        let root = Path::new("/nonexistent-repo-root-for-test");
        observe_task(root, "task_1", &[], "completed", false);
    }

    #[test]
    fn affected_returns_none_without_store() {
        let root = Path::new("/nonexistent-repo-root-for-test");
        let changes = json!({"changes": []});
        assert!(affected(root, "HEAD", &changes).is_none());
    }
}
