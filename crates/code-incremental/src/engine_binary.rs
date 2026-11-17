//! Engine binary resolution and spawn.
//!
//! Resolves the `incremental` engine binary from (in order):
//! 1. `FRONTAL_CODE_INCREMENTAL_ENGINE` env var (explicit path).
//! 2. `INCREMENTAL_ENGINE` env var (the binding's own default).
//! 3. A path baked in at build time (relative to the workspace root).
//! 4. `incremental` on `PATH`.
//!
//! If no binary is found, the integration degrades to "always run everything"
//! (a safe fallback). It never returns a hard error for a missing binary.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Result of attempting to locate the engine binary.
#[derive(Debug, Clone)]
pub enum EngineBinary {
    /// The binary was found at the given path.
    Found(PathBuf),
    /// No binary is available. The integration should degrade to "always run everything."
    Missing,
}

impl EngineBinary {
    /// Whether a usable binary was found.
    #[must_use]
    pub fn is_available(&self) -> bool {
        matches!(self, EngineBinary::Found(_))
    }

    /// The resolved path, if found.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            EngineBinary::Found(path) => Some(path),
            EngineBinary::Missing => None,
        }
    }

    /// Resolve the engine binary from env vars and build-time paths.
    #[must_use]
    pub fn resolve() -> Self {
        resolve()
    }
}

/// Resolve the engine binary from env vars and build-time paths.
#[must_use]
pub fn resolve() -> EngineBinary {
    // 1. Explicit override.
    if let Some(path) = env::var_os("FRONTAL_CODE_INCREMENTAL_ENGINE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return EngineBinary::Found(path);
        }
    }

    // 2. The binding's own env var.
    if let Some(path) = env::var_os("INCREMENTAL_ENGINE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return EngineBinary::Found(path);
        }
    }

    // 3. Build-time baked path (workspace root).
    if let Some(baked) = baked_path() {
        if baked.is_file() {
            return EngineBinary::Found(baked);
        }
    }

    // 4. PATH lookup.
    if which_binary("incremental") {
        return EngineBinary::Found(PathBuf::from("incremental"));
    }

    EngineBinary::Missing
}

/// Build-time-baked path to the engine binary, computed relative to the
/// workspace root (which is two directories above `crates/code-incremental`).
///
/// `CARGO_MANIFEST_DIR` is the crate directory itself
/// (`<root>/crates/code-incremental`), so exactly two `parent()` calls reach
/// the workspace root: one off the crate dir, one off `crates`.
fn baked_path() -> Option<PathBuf> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates
        .and_then(|p| p.parent()); // workspace root

    let triple = target_triple();
    workspace_root.map(|root| {
        root.join(format!(
            "third_party/incremental/engine-binaries/{triple}/incremental"
        ))
    })
}

/// Check whether a binary is on the PATH.
fn which_binary(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The target triple for this platform (e.g. `darwin-arm64`, `linux-x86_64`).
///
/// The OS component is normalized to the directory names used under
/// `third_party/incremental/engine-binaries/`: macOS reports as `macos` to
/// `env::consts::OS`, but the layout uses `darwin`.
#[must_use]
pub fn target_triple() -> String {
    let os = match env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = env::consts::ARCH;
    // Normalize arch names to match the directory layout.
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        "x86" => "x86",
        other => other,
    };
    format!("{os}-{arch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_triple_is_well_formed() {
        let triple = target_triple();
        assert!(
            triple.contains('-'),
            "triple should contain a dash: {triple}"
        );
    }

    #[test]
    fn baked_path_resolves_within_workspace() {
        // The baked path must point at the vendored engine location relative
        // to the workspace root. A wrong parent depth or a raw `macos` triple
        // would point at a non-existent layout and the integration would
        // silently degrade to "run everything."
        //
        // Only the path shape is asserted (not file existence): the engine
        // binary is a checked-in artifact that is not present in the Bazel
        // sandbox, where the crate is tested from a copied workspace that
        // carries the Rust sources but not the prebuilt binaries.
        let Some(path) = baked_path() else {
            panic!("baked path could not be computed from CARGO_MANIFEST_DIR");
        };
        let s = path.display().to_string();
        assert!(
            s.contains("third_party/incremental/engine-binaries/"),
            "baked path missing the engine-binaries layout: {s}"
        );
        assert!(
            !s.contains("macos-"),
            "raw `macos` triple not normalized to `darwin`: {s}"
        );
        assert!(
            s.ends_with("/incremental"),
            "baked path must end in the engine binary name: {s}"
        );
        // The path must resolve relative to the crate's own workspace, i.e.
        // walking up from the baked path must reach the crate manifest dir.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut p = path.clone();
        for _ in 0..7 {
            // third_party/incremental/engine-binaries/<triple>/incremental
            if !p.pop() {
                break;
            }
        }
        assert!(
            manifest.starts_with(&p),
            "baked path does not sit under the same workspace as the crate: {} vs {}",
            path.display(),
            p.display()
        );
    }

    #[test]
    fn missing_engine_reports_unavailable() {
        // We can't guarantee a missing engine in all environments, so just
        // check the API shape.
        let missing = EngineBinary::Missing;
        assert!(!missing.is_available());
        assert!(missing.path().is_none());
    }

    #[test]
    fn found_engine_reports_available() {
        let found = EngineBinary::Found(PathBuf::from("/bin/true"));
        assert!(found.is_available());
        assert!(found.path().is_some());
    }
}
