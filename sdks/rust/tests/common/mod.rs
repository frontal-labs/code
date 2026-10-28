//! Shared test helper: a hermetic stand-in for the `frontal-code` CLI.
//!
//! The fake binary records the argv/cwd it was invoked with (NUL-separated)
//! and echoes a configurable JSONL payload, so SDK tests need no real binary,
//! API key, or network access.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::OnceLock;

use tempfile::TempDir;

/// The fake `frontal-code` binary, shared by every mock in this test process.
///
/// Writing a fresh executable per test races with other tests' `fork`s: a child
/// forked while the script is still open for writing keeps that descriptor until
/// it execs, so running the script fails with `ETXTBSY` ("Text file busy"). One
/// binary is written, closed, and made executable before any test can spawn it,
/// and each test's response, exit code, and capture paths come from its environment.
fn shared_bin() -> &'static PathBuf {
  static BIN: OnceLock<PathBuf> = OnceLock::new();
  BIN.get_or_init(|| {
    let dir = tempfile::Builder::new()
      .prefix("frontal-code-mock")
      .tempdir()
      .expect("tempdir");
    let bin_path = dir.path().join("frontal-code");
    let script = "#!/usr/bin/env bash\n\
             CAP=\"${FRONTAL_CODE_MOCK_CAPTURE:-/dev/null}\"\n\
             CWD=\"${FRONTAL_CODE_MOCK_CWD:-/dev/null}\"\n\
             RESP=\"${FRONTAL_CODE_MOCK_RESPONSE:-/dev/null}\"\n\
             EXIT=\"${FRONTAL_CODE_MOCK_EXIT:-0}\"\n\
             pwd > \"$CWD\"\n\
             printf '%s\\0' \"$@\" > \"$CAP\"\n\
             cat \"$RESP\"\n\
             exit \"$EXIT\"\n";
    fs::write(&bin_path, script).expect("write script");
    let mut perms = fs::metadata(&bin_path).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&bin_path, perms).expect("chmod");
    // Keep the directory for the whole process; the binary must outlive every test.
    std::mem::forget(dir);
    bin_path
  })
}

#[allow(dead_code)]
pub struct MockFrontalCode {
  #[allow(dead_code)]
  pub dir: TempDir,
  pub bin_path: PathBuf,
  pub capture_path: PathBuf,
  pub cwd_path: PathBuf,
  pub response_path: PathBuf,
  exit_code: i32,
}

impl MockFrontalCode {
  pub fn new(response: &str, exit_code: i32) -> Self {
    let dir = tempfile::tempdir().expect("tempdir");
    let capture_path = dir.path().join("capture.bin");
    let cwd_path = dir.path().join("cwd.txt");
    let response_path = dir.path().join("response.jsonl");
    fs::write(&response_path, response).expect("write response");

    Self {
      dir,
      bin_path: shared_bin().clone(),
      capture_path,
      cwd_path,
      response_path,
      exit_code,
    }
  }

  /// Environment routing a spawned CLI to this mock.
  pub fn env(&self) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert(
      "FRONTAL_CODE_MOCK_CAPTURE".into(),
      self.capture_path.to_string_lossy().into_owned(),
    );
    map.insert(
      "FRONTAL_CODE_MOCK_CWD".into(),
      self.cwd_path.to_string_lossy().into_owned(),
    );
    map.insert(
      "FRONTAL_CODE_MOCK_RESPONSE".into(),
      self.response_path.to_string_lossy().into_owned(),
    );
    map.insert("FRONTAL_CODE_MOCK_EXIT".into(), self.exit_code.to_string());
    map.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    map
  }

  /// The argv the mock captured, one element per entry.
  pub fn captured_args(&self) -> Vec<String> {
    let raw = fs::read(&self.capture_path).unwrap_or_default();
    let text = String::from_utf8_lossy(&raw);
    text
      .split('\0')
      .filter(|part| !part.is_empty())
      .map(str::to_string)
      .collect()
  }

  /// The working directory the mock recorded.
  pub fn captured_cwd(&self) -> String {
    fs::read_to_string(&self.cwd_path)
      .unwrap_or_default()
      .trim()
      .to_string()
  }
}
