//! The top-level [`Frontal Code`] client.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use crate::protocol::{FrontalCodeOptions, ThreadOptions};
use crate::thread::Thread;

const REQUIRED_ENV_VARS: &[&str] = &["CODEX_API_KEY"];

/// The two names every install channel provides for the same CLI.
const COMMAND_NAMES: [&str; 2] = ["frontal-code", "code"];

/// The CLI command to run when none is given: `frontal-code` if it is on `PATH`, otherwise `code`.
/// Both are the same program, so either works; the lookup only chooses which name to spawn.
pub fn default_command() -> String {
  default_command_on(std::env::var_os("PATH").as_deref())
}

fn default_command_on(path: Option<&OsStr>) -> String {
  for name in COMMAND_NAMES {
    if is_on_path(name, path) {
      return name.to_string();
    }
  }
  COMMAND_NAMES[0].to_string()
}

fn is_on_path(name: &str, path: Option<&OsStr>) -> bool {
  let Some(path) = path else { return false };
  std::env::split_paths(path).any(|dir| is_executable(&dir.join(name)))
}

#[cfg(unix)]
fn is_executable(candidate: &Path) -> bool {
  use std::os::unix::fs::PermissionsExt;
  std::fs::metadata(candidate)
    .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(candidate: &Path) -> bool {
  candidate.is_file()
}

/// The top-level `FrontalCode` client. Wraps the `frontal-code` CLI and is used to start and
/// resume conversation [`Thread`]s.
#[derive(Debug, Clone)]
pub struct FrontalCode {
  pub options: FrontalCodeOptions,
  pub command: String,
}

impl FrontalCode {
  /// Create a client from the given options.
  pub fn new(options: FrontalCodeOptions) -> Self {
    let command = options.command.clone().unwrap_or_else(default_command);
    Self { options, command }
  }

  /// Start a fresh conversation thread.
  pub fn start_thread(&self, options: ThreadOptions) -> Thread {
    Thread::new(Arc::new(self.clone()), options, None)
  }

  /// Reconstruct a thread from a previously persisted session id.
  pub fn resume_thread(&self, session_id: String, options: ThreadOptions) -> Thread {
    Thread::new(Arc::new(self.clone()), options, Some(session_id))
  }

  /// Build the environment for a spawned CLI. Starts from the ambient
  /// environment, applies the user-provided `env`, then injects any required
  /// variables (such as `CODEX_API_KEY`) if still missing.
  pub fn build_env(&self) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    if let Some(user_env) = &self.options.env {
      for (key, value) in user_env {
        env.insert(key.clone(), value.clone());
      }
    }
    for var in REQUIRED_ENV_VARS {
      if !env.contains_key(*var) {
        if let Ok(value) = std::env::var(var) {
          env.insert((*var).to_string(), value);
        }
      }
    }
    env
  }
}

#[cfg(test)]
mod command_tests {
  use super::*;

  fn bin_dir(names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in names {
      std::fs::write(
        dir.path().join(name),
        "#!/bin/sh
",
      )
      .expect("write");
    }
    dir
  }

  #[cfg(unix)]
  fn make_executable(dir: &tempfile::TempDir, name: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join(name);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
  }

  #[cfg(unix)]
  #[test]
  fn prefers_frontal_code_when_both_names_are_on_path() {
    let dir = bin_dir(&["frontal-code", "code"]);
    make_executable(&dir, "frontal-code");
    make_executable(&dir, "code");
    assert_eq!(
      default_command_on(Some(dir.path().as_os_str())),
      "frontal-code"
    );
  }

  #[cfg(unix)]
  #[test]
  fn falls_back_to_code_when_only_code_is_on_path() {
    let dir = bin_dir(&["code"]);
    make_executable(&dir, "code");
    assert_eq!(default_command_on(Some(dir.path().as_os_str())), "code");
  }

  #[cfg(unix)]
  #[test]
  fn ignores_a_file_that_is_not_executable() {
    let dir = bin_dir(&["code"]);
    assert_eq!(
      default_command_on(Some(dir.path().as_os_str())),
      "frontal-code"
    );
  }

  #[test]
  fn defaults_to_frontal_code_when_path_is_unset() {
    assert_eq!(default_command_on(None), "frontal-code");
  }
}
