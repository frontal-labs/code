//! Context builder for the Incremental protocol.
//!
//! Every observation and every `affected` query carries a `context` object —
//! the set of global facts that change what a task's result means. The context
//! must be identical between observations and queries, or the engine falls back
//! to running everything (policy P2: `context_changed`).

use serde_json::{json, Value};
use std::path::Path;

/// Build the context object for this repo.
///
/// # Arguments
///
/// * `repo_root` - The repository root directory.
/// * `runner` - The runner identifier (e.g. `frontal-code-sdk@0.3.0` or `cargo@1.85`).
/// * `node` - The runtime node version (e.g. `1.85.0` for Rust, `22.11.0` for Node).
///
/// The context includes:
/// - `lockfile`: digest of `Cargo.lock` (Rust) or `package-lock.json` (TS), if present.
/// - `runner`: the runner identifier.
/// - `node`: the runtime version.
/// - `fmt`: the engine's format version (1).
/// - `caps`: capability flags. We only claim capabilities the adapter really records.
#[must_use]
pub fn build_context(repo_root: &Path, runner: &str, node: &str) -> Value {
    let lockfile = lockfile_digest(repo_root);
    let mut ctx = json!({
        "lockfile": lockfile,
        "runner": runner,
        "node": node,
        "fmt": 1,
    });
    if lockfile.is_none() {
        ctx["lockfile"] = Value::Null;
    }
    // Capability flags: we only claim what the adapter actually records.
    ctx["caps"] = json!({
        "reads": true,
        "lists": false,
        "probes": false,
        "writes": true,
        "env": false,
        "config": false,
        "declared_tasks": false,
    });
    ctx
}

/// Build a minimal context for tests (no real lockfile, fixed runner/node).
#[must_use]
pub fn test_context() -> Value {
    build_context(Path::new("/nonexistent"), "test-runner@1.0.0", "1.0.0")
}

/// Compute the lockfile digest.
///
/// Reads `Cargo.lock` or `package-lock.json` from `repo_root` and returns a
/// deterministic 32-byte digest as a hex string, or `None` if no lockfile is
/// present or the read fails.
fn lockfile_digest(repo_root: &Path) -> Option<String> {
    // Try Rust lockfile first, then Node.
    for name in ["Cargo.lock", "package-lock.json"] {
        let path = repo_root.join(name);
        if path.is_file() {
            if let Ok(content) = std::fs::read(&path) {
                return Some(hex(&sha256_bytes(&content)));
            }
        }
    }
    None
}

/// Compute a deterministic 32-byte digest of a byte slice.
///
/// This is a small inline implementation to avoid pulling in the `sha2` crate
/// as a dependency of `code-incremental`. The lockfile digest only needs to be
/// stable and collision-resistant enough for the engine to compare it — it is
/// not cryptographically secure. Uses FNV-1a 64-bit extended to 32 bytes by
/// hashing with 4 different offset mixings.
#[must_use]
fn sha256_bytes(data: &[u8]) -> [u8; 32] {
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

/// Encode a byte slice as a lowercase hex string.
#[must_use]
fn hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for &byte in data {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_has_required_fields() {
        let ctx = test_context();
        assert!(ctx.get("lockfile").is_some());
        assert!(ctx.get("runner").is_some());
        assert!(ctx.get("node").is_some());
        assert_eq!(ctx["fmt"], 1);
        let caps = &ctx["caps"];
        assert!(caps["reads"].is_boolean());
        assert!(caps["writes"].is_boolean());
    }

    #[test]
    fn build_context_in_empty_dir() {
        let dir = std::env::temp_dir().join(format!(
            "code-inc-ctx-empty-{}-{}",
            std::process::id(),
            nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = build_context(&dir, "runner", "1.0");
        assert!(ctx["lockfile"].is_null(), "no lockfile in empty dir");
        assert_eq!(ctx["runner"], "runner");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_context_with_cargo_lock() {
        let dir = std::env::temp_dir().join(format!(
            "code-inc-ctx-lock-{}-{}",
            std::process::id(),
            nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.lock"), "[package]\nname = \"x\"\n").unwrap();
        let ctx = build_context(&dir, "runner", "1.0");
        assert!(
            ctx["lockfile"].is_string(),
            "lockfile should be a hex string"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fnv_hash_is_deterministic() {
        let a = sha256_bytes(b"hello world");
        let b = sha256_bytes(b"hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn fnv_hash_differs_for_different_input() {
        let a = sha256_bytes(b"hello");
        let b = sha256_bytes(b"world");
        assert_ne!(a, b);
    }

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    }
}
