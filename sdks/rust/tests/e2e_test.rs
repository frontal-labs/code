//! End-to-end checks against the real `code` binary and a live model.
//!
//! Skipped unless `FRONTAL_CODE_E2E_BIN` is set, so normal test runs stay offline. The default
//! provider is a local Ollama server (`OLLAMA_BASE_URL`, default `http://localhost:11434`) and
//! model `qwen2.5:1.5b`. For a cloud OpenAI-compatible provider set
//! `FRONTAL_CODE_E2E_PROVIDER=openai` with `OPENAI_API_KEY` (and `OPENAI_BASE_URL`).
//! The mock-based tests cover protocol handling; these prove the wire contract.

use std::collections::HashMap;
use std::path::PathBuf;

use frontal_code_sdk::{
  FrontalCode, FrontalCodeEvent, FrontalCodeOptions, ThreadInput, ThreadOptions, ThreadRunOptions,
};

const DEFAULT_MODEL: &str = "qwen2.5:1.5b";
const DEFAULT_PROVIDER: &str = "ollama";

fn e2e_bin() -> Option<String> {
  let bin = std::env::var("FRONTAL_CODE_E2E_BIN").ok()?;
  let provider =
    std::env::var("FRONTAL_CODE_E2E_PROVIDER").unwrap_or_else(|_| DEFAULT_PROVIDER.into());
  if provider == "openai" {
    std::env::var("OPENAI_API_KEY").ok()?;
  }
  Some(bin)
}

fn client(bin: String) -> FrontalCode {
  let mut env = HashMap::new();
  for key in ["OPENAI_API_KEY", "OPENAI_BASE_URL", "OLLAMA_BASE_URL"] {
    if let Ok(value) = std::env::var(key) {
      env.insert(key.to_string(), value);
    }
  }
  env.insert(
    "FRONTAL_CODE_MAX_OUTPUT_TOKENS".to_string(),
    "1024".to_string(),
  );
  FrontalCode::new(FrontalCodeOptions {
    command: Some(bin),
    env: Some(env),
    ..Default::default()
  })
}

fn options(workdir: PathBuf) -> ThreadOptions {
  ThreadOptions {
    working_directory: Some(workdir),
    provider: Some(
      std::env::var("FRONTAL_CODE_E2E_PROVIDER").unwrap_or_else(|_| DEFAULT_PROVIDER.into()),
    ),
    model: Some(std::env::var("FRONTAL_CODE_E2E_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into())),
    permission_mode: Some("read-only".to_string()),
    ..Default::default()
  }
}

#[tokio::test]
async fn run_returns_a_session_id_and_text() {
  let Some(bin) = e2e_bin() else { return };
  let workdir = tempfile::tempdir().expect("tempdir");
  let thread = client(bin).start_thread(options(workdir.path().to_path_buf()));
  let turn = thread
    .run(
      &ThreadInput::Text("Reply with the single word: pong".into()),
      &ThreadRunOptions::default(),
    )
    .await
    .expect("turn should succeed");
  assert_ne!(turn.final_response.trim(), "");
  assert!(thread.id().await.is_some());
}

#[tokio::test]
async fn streamed_turn_emits_turn_started_then_completed() {
  let Some(bin) = e2e_bin() else { return };
  let workdir = tempfile::tempdir().expect("tempdir");
  let thread = client(bin).start_thread(options(workdir.path().to_path_buf()));
  let mut streamed = thread
    .run_streamed(
      &ThreadInput::Text("Reply with the single word: pong".into()),
      &ThreadRunOptions::default(),
    )
    .await
    .expect("streamed turn should start");

  let mut events = Vec::new();
  while let Some(event) = streamed.events.recv().await {
    events.push(event);
  }
  assert!(matches!(
    events.first(),
    Some(FrontalCodeEvent::TurnStarted)
  ));
  assert!(matches!(
    events.last(),
    Some(FrontalCodeEvent::TurnCompleted { .. })
  ));
}

#[tokio::test]
async fn resumed_thread_keeps_earlier_context() {
  let Some(bin) = e2e_bin() else { return };
  let workdir = tempfile::tempdir().expect("tempdir");
  let frontal = client(bin);
  let first = frontal.start_thread(options(workdir.path().to_path_buf()));
  first
    .run(
      &ThreadInput::Text("My favorite color is teal. Acknowledge in one short sentence.".into()),
      &ThreadRunOptions::default(),
    )
    .await
    .expect("first turn");
  let session_id = first.id().await.expect("session id after first turn");

  let resumed = frontal.resume_thread(session_id, options(workdir.path().to_path_buf()));
  let turn = resumed
    .run(
      &ThreadInput::Text("What is my favorite color? Answer with one word.".into()),
      &ThreadRunOptions::default(),
    )
    .await
    .expect("resumed turn");
  assert!(
    turn.final_response.to_lowercase().contains("teal"),
    "got: {}",
    turn.final_response
  );
}
