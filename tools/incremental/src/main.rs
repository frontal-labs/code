//! `tools-incremental` — dev-pipeline CLI for the Incremental Compute Engine.
//!
//! Wraps the `code-incremental` crate so the repo's build/test pipeline can
//! observe task outcomes, advance the baseline, and ask which tasks are
//! affected by a changeset. Fallbacks ("run everything") are the safe
//! default and never a hard error.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use code_incremental::{build_context, target_triple, EngineBinary, Store};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Parser)]
#[command(
    name = "code-incremental",
    about = "Incremental Compute Engine dev CLI"
)]
struct Cli {
    /// Repository root the store is rooted at (defaults to the current directory).
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text, global = true)]
    format: Format,

    /// Suppress the fallback-safe exit code (always exit 0 on success).
    #[arg(long, global = true)]
    no_fail_on_fallback: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[value(rename_all = "lowercase")]
enum Format {
    Text,
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Resolve the engine binary and print its path.
    Doctor,
    /// The engine `hello` handshake (protocol, engine, capabilities, store).
    Hello,
    /// Ping the engine.
    Ping,
    /// Store statistics (tasks, observations, baseline).
    Stats,
    /// Query the current baseline.
    Baseline,
    /// Advance the baseline to a new sha.
    Advance {
        /// New baseline sha to advance to.
        to: String,
        /// Expected current baseline; omit (or use `--expected null`) for the first advance.
        #[arg(long, default_value = "null")]
        expected: String,
    },
    /// Record a single observation (JSON file or `-` for stdin).
    Observe {
        /// Path to a JSON observation file, or `-` for stdin.
        input: PathBuf,
    },
    /// Record multiple observations from a JSON array file (or `-` for stdin).
    ObserveMany {
        /// Path to a JSON array of observations, or `-` for stdin.
        input: PathBuf,
    },
    /// Explain why a task is or is not invalid.
    Explain { task: String },
    /// Drop a task's observations.
    Forget { task: String },
    /// Compute the affected plan for a changeset.
    Affected {
        /// Baseline to compare against.
        baseline: String,
        /// Path to a changeset JSON file (`{"changes": [{"path": ..., "status": ...}]}`),
        /// or `-` for stdin. When omitted, a git diff against the baseline is used.
        #[arg(long)]
        changes: Option<PathBuf>,
        /// Runner identifier for the observation context (default: `frontal-code-cli`).
        #[arg(long, default_value = "frontal-code-cli")]
        runner: String,
        /// Runtime node version for the observation context.
        #[arg(long, default_value = "0.0.0")]
        node: String,
        /// Repo-relative path to the lockfile (default: auto-detect `Cargo.lock`).
        #[arg(long)]
        lockfile: Option<String>,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)]
fn run() -> Result<()> {
    let cli = Cli::parse();
    let repo_root = canonicalize(&cli.root)?;

    if matches!(cli.command, Command::Doctor) {
        let triple = target_triple();
        let engine = EngineBinary::resolve();
        let path = engine.path().map(Path::display);
        let report = json!({
            "platform": triple,
            "engine_found": engine.is_available(),
            "engine_path": path.map(|p| p.to_string()),
            "store_path": store_display_path(&repo_root),
        });
        print_result(cli.format, &report, &doctor_text(&report));
        return Ok(());
    }

    let Some(mut store) = open_store(&repo_root)? else {
        // No engine binary available: degrade to the safe default.
        eprintln!("incremental engine unavailable; assuming everything is affected");
        if !cli.no_fail_on_fallback {
            std::process::exit(EXIT_FALLBACK);
        }
        return Ok(());
    };

    match &cli.command {
        Command::Doctor => unreachable!("handled above"),
        Command::Hello => {
            print_raw(cli.format, &store.hello().context("hello")?);
        }
        Command::Ping => {
            print_raw(cli.format, &store.ping().context("ping")?);
        }
        Command::Stats => {
            print_raw(cli.format, &store.stats().context("stats")?);
        }
        Command::Baseline => {
            print_raw(cli.format, &store.baseline().context("baseline")?);
        }
        Command::Advance { to, expected } => {
            let expected_opt = (expected != "null").then_some(expected.as_str());
            print_raw(
                cli.format,
                &store.advance(to, expected_opt).context("advance")?,
            );
        }
        Command::Observe { input } => {
            let observation = read_json(input).context("read observation")?;
            print_raw(cli.format, &store.observe(&observation).context("observe")?);
        }
        Command::ObserveMany { input } => {
            let value = read_json(input).context("read observations")?;
            let arr = value
                .as_array()
                .context("input must be a JSON array of observations")?;
            print_raw(
                cli.format,
                &store.observe_many(arr).context("observe_many")?,
            );
        }
        Command::Explain { task } => {
            print_raw(cli.format, &store.explain(task).context("explain")?);
        }
        Command::Forget { task } => {
            print_raw(cli.format, &store.forget(task).context("forget")?);
        }
        Command::Affected {
            baseline,
            changes,
            runner,
            node,
            lockfile,
        } => {
            let changeset = match changes {
                Some(path) => read_json(path).context("read changeset")?,
                None => git_changeset(baseline, &repo_root)?,
            };
            // Lockfile override: the digest is embedded in the context;
            // an explicit `--lockfile` path is honored by re-deriving
            // the digest for that specific file.
            let context = if let Some(lock) = lockfile {
                context_with_lockfile(&repo_root, lock, runner, node)
            } else {
                build_context(&repo_root, runner, node)
            };
            let plan = store
                .affected(&changeset, baseline, &context)
                .with_context(|| format!("affected against baseline {baseline}"))?;

            let report = json!({
                "baseline": baseline,
                "fallback": plan.is_fallback(),
                "fallback_reason": plan.fallback.as_ref().map(|f| &f.reason),
                "tasks": plan.tasks,
                "skipped": plan.skipped,
                "unobserved": plan.unobserved,
                "confidence": plan.confidence,
                "graph_version": plan.graph_version,
            });
            print_result(cli.format, &report, &affected_text(&report));
            if plan.is_fallback() && !cli.no_fail_on_fallback {
                std::process::exit(EXIT_FALLBACK);
            }
        }
    }

    Ok(())
}

/// Exit code for a fallback result: the caller should run everything.
const EXIT_FALLBACK: i32 = 3;

fn open_store(repo_root: &Path) -> Result<Option<Store>> {
    let engine = EngineBinary::resolve();
    if engine.path().is_none() {
        return Ok(None);
    }
    Store::open(repo_root)
        .map(Some)
        .context("open incremental store")
}

fn canonicalize(path: &Path) -> Result<PathBuf> {
    let canonical =
        fs::canonicalize(path).with_context(|| format!("resolve {}", path.display()))?;
    if !canonical.is_dir() {
        bail!("{} is not a directory", canonical.display());
    }
    Ok(canonical)
}

fn store_display_path(repo_root: &Path) -> String {
    std::env::var_os("FRONTAL_CODE_INCREMENTAL_STORE")
        .or_else(|| std::env::var_os("INCREMENTAL_STORE"))
        .map_or_else(
            || repo_root.join(".incremental").display().to_string(),
            |p| p.to_string_lossy().into_owned(),
        )
}

fn read_json(path: &Path) -> Result<Value> {
    let raw = if path == Path::new("-") {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).context("read stdin")?;
        buf
    } else {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    };
    let value: Value = serde_json::from_str(&raw).with_context(|| "parse JSON")?;
    Ok(value)
}

/// Build a changeset from `git diff --name-status <baseline>` when no file is given.
fn git_changeset(baseline: &str, repo_root: &Path) -> Result<Value> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-status", baseline])
        .current_dir(repo_root)
        .output()
        .context("run git diff")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // A bad baseline (unknown commit) is a fallback trigger, not a crash.
        bail!(
            "git diff failed: {stderr}\n\
             (pass --changes with an explicit changeset, or use a valid baseline sha)"
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut changes: Vec<Value> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.splitn(2, '\t');
            let status = parts.next().unwrap_or_default();
            let path = parts.next().unwrap_or_default();
            json!({"path": path, "status": map_git_status(status)})
        })
        .collect();
    // `git diff --name-status` never reports untracked files, and the
    // baseline is a commit, not the working tree: a newly added file that
    // has not been staged is invisible to the diff above. Without it, tasks
    // that read such a file would be certified as unaffected. Append the
    // untracked set so the changeset covers the full working tree.
    if let Ok(untracked) = std::process::Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(repo_root)
        .output()
    {
        if untracked.status.success() {
            let list = String::from_utf8_lossy(&untracked.stdout);
            for line in list.lines() {
                let path = line.trim();
                if path.is_empty() {
                    continue;
                }
                changes.push(json!({"path": path, "status": "A"}));
            }
        }
    }
    Ok(json!({"changes": changes}))
}

/// Map a Git `--name-status` code to the change-status vocabulary the
/// engine understands.
///
/// The engine treats a change as "file is different" when the status is
/// anything other than `A` (added). A raw Git status of `D` (deleted) or
/// `R`/`C` (renamed/copied, with the second path in a second column) would
/// pass through verbatim and be misread. Normalize deletions, renames and
/// copies to `M` (modified) so they invalidate the tasks that read the
/// path; `A` and `M` already match the engine vocabulary.
fn map_git_status(status: &str) -> &'static str {
    match status {
        "A" => "A",
        _ => "M",
    }
}

/// Like [`build_context`] but with an explicit lockfile path override.
fn context_with_lockfile(repo_root: &Path, lockfile: &str, runner: &str, node: &str) -> Value {
    let mut ctx = build_context(repo_root, runner, node);
    let path = Path::new(lockfile);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    let digest = match fs::read(&abs) {
        Ok(bytes) => {
            // Must match the digest scheme in `code_incremental::build_context`
            // exactly (a 32-byte digest hex-encoded to 64 chars). The engine
            // compares context lockfile digests across observations and
            // queries; a different scheme or length here would mismatch and
            // trigger `context_changed` fallback. The reference is a homegrown
            // FNV-based digest in `context.rs` (not the `sha2` crate), so
            // replicate it here to keep both sides byte-identical.
            Some(Value::String(hex(&digest_32(&bytes))))
        }
        Err(_) => None,
    };
    if let Some(obj) = ctx.as_object_mut() {
        obj.insert("lockfile".to_string(), digest.unwrap_or(Value::Null));
    }
    ctx
}

/// The 32-byte lockfile digest scheme used by `code_incremental::build_context`.
///
/// Replicated verbatim from `crates/code-incremental/src/context.rs` so the CLI
/// produces the same digest as the crate for identical lockfile bytes. The
/// engine compares lockfile digests across observations and queries; a
/// mismatch (including different length) triggers `context_changed` fallback.
#[must_use]
fn digest_32(data: &[u8]) -> [u8; 32] {
    fn fnv1a_64(data: &[u8], offset: u64) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325u64 ^ offset;
        for &byte in data {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x1000_0000_01b3u64);
        }
        hash
    }

    let mut out = [0u8; 32];
    for (i, &offset) in [0u64, 1, 2, 3].iter().enumerate() {
        out[i * 8..(i + 1) * 8].copy_from_slice(&fnv1a_64(data, offset).to_le_bytes());
    }
    out
}

/// Encode a byte slice as a lowercase hex string (same scheme as `context.rs`).
#[must_use]
fn hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for &byte in data {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

fn print_raw(format: Format, value: &Value) {
    print_result(format, value, &value.to_string());
}

fn print_result(format: Format, value: &Value, text: &str) {
    match format {
        Format::Text => println!("{text}"),
        Format::Json => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
            println!("{pretty}");
        }
    }
}

fn doctor_text(report: &Value) -> String {
    let triple = report["platform"].as_str().unwrap_or("?");
    if report["engine_found"].as_bool().unwrap_or(false) {
        let path = report["engine_path"].as_str().unwrap_or("?");
        format!(
            "platform: {triple}\nengine: {path}\nstore: {}",
            report["store_path"].as_str().unwrap_or("?")
        )
    } else {
        format!(
            "platform: {triple}\nengine: NOT FOUND (set FRONTAL_CODE_INCREMENTAL_ENGINE or INCREMENTAL_ENGINE)\nstore: {}",
            report["store_path"].as_str().unwrap_or("?")
        )
    }
}

fn affected_text(report: &Value) -> String {
    if report["fallback"].as_bool().unwrap_or(false) {
        let reason = report["fallback_reason"].as_str().unwrap_or("unknown");
        return format!("FALLBACK ({reason}): run everything");
    }
    let mut lines = Vec::new();
    if let Some(tasks) = report["tasks"].as_array() {
        for task in tasks {
            let id = task["task_id"].as_str().unwrap_or("?");
            let reasons = task["reasons"]
                .as_array()
                .map(|r| {
                    r.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            lines.push(format!("{id}: {reasons}"));
        }
    }
    lines.push(format!(
        "skipped={} unobserved={} confidence={}",
        report["skipped"].as_u64().unwrap_or(0),
        report["unobserved"].as_array().map_or(0, Vec::len),
        report["confidence"].as_str().unwrap_or("?")
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affected_text_reports_fallback() {
        let report = json!({"fallback": true, "fallback_reason": "no_baseline"});
        let text = affected_text(&report);
        assert!(text.contains("FALLBACK"));
        assert!(text.contains("no_baseline"));
    }

    #[test]
    fn affected_text_lists_tasks() {
        let report = json!({
            "fallback": false,
            "tasks": [{"task_id": "a.rs#test", "reasons": ["reads:a.rs"]}],
            "skipped": 2,
            "unobserved": ["b.rs#test"],
            "confidence": "medium"
        });
        let text = affected_text(&report);
        assert!(text.contains("a.rs#test: reads:a.rs"));
        assert!(text.contains("skipped=2"));
        assert!(text.contains("confidence=medium"));
    }
}
