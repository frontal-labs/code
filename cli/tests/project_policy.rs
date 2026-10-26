//! Project policy enforced by the real binary.
//!
//! Each case here is refused before any request leaves the process, so the tests run
//! offline.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Workspace {
    root: PathBuf,
    project: PathBuf,
    config_home: PathBuf,
}

impl Workspace {
    fn new(name: &str, project_settings: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("project-policy-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        let config_home = root.join("config");
        fs::create_dir_all(project.join(".frontal-code")).expect("project settings dir");
        fs::create_dir_all(&config_home).expect("config home");
        fs::write(
            project.join(".frontal-code/settings.json"),
            project_settings,
        )
        .expect("write project settings");
        Self {
            root,
            project,
            config_home,
        }
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_code"));
        command
            .args(args)
            .current_dir(&self.project)
            .env("FRONTAL_CODE_CONFIG_HOME", &self.config_home)
            .env_remove("FRONTAL_API_KEY")
            .env_remove("FRONTAL_AUTH_TOKEN")
            .env_remove("OPENAI_API_KEY")
            .env_remove("RUSTY_CLAUDE_PERMISSION_MODE");
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().expect("run frontal-code")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn project_allowed_providers_refuse_other_providers_before_any_request() {
    let workspace = Workspace::new(
        "providers",
        r#"{"policy":{"allowedProviders":["anthropic"]}}"#,
    );
    let output = workspace.run(
        &[
            "--provider",
            "openai",
            "--model",
            "gpt-5-codex",
            "prompt",
            "hi",
        ],
        &[("OPENAI_API_KEY", "test-key")],
    );

    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(
        text.contains("provider `openai` is not allowed by project settings"),
        "{text}"
    );
}

#[test]
fn project_allowed_models_refuse_models_outside_the_list() {
    let workspace = Workspace::new(
        "models",
        r#"{"policy":{"allowedModels":["claude-sonnet-*"]}}"#,
    );
    let output = workspace.run(
        &[
            "--provider",
            "openai",
            "--model",
            "gpt-5-codex",
            "prompt",
            "hi",
        ],
        &[("OPENAI_API_KEY", "test-key")],
    );

    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(
        text.contains("model `gpt-5-codex` is not allowed by project settings"),
        "{text}"
    );
}

#[test]
fn auth_method_rule_refuses_a_static_key_when_only_oauth_is_allowed() {
    let workspace = Workspace::new(
        "auth",
        r#"{"policy":{"authMethods":{"anthropic":["oauth"]}}}"#,
    );
    let output = workspace.run(
        &[
            "--provider",
            "anthropic",
            "--model",
            "claude-sonnet-4-6",
            "prompt",
            "hi",
        ],
        &[("FRONTAL_API_KEY", "test-key")],
    );

    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(
        text.contains("apiKey authentication for `anthropic` is not allowed by project settings"),
        "{text}"
    );
}

#[test]
fn project_allow_rules_are_reported_as_ignored() {
    let workspace = Workspace::new("ignored", r#"{"permissions":{"allow":["bash"]}}"#);
    // The provider check passes (no policy restricts it); the run then fails on the
    // closed endpoint, which is fine: the warning is what this test checks.
    let output = workspace.run(
        &["--provider", "openai", "--model", "gpt-4.1", "prompt", "hi"],
        &[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", "http://127.0.0.1:9/v1"),
        ],
    );

    let text = stderr(&output);
    assert!(
        text.contains(
            "permissions.allow is ignored; project and local settings can only restrict access"
        ),
        "{text}"
    );
}

#[test]
fn policy_in_user_settings_applies_even_without_project_policy() {
    let workspace = Workspace::new("user-policy", "{}");
    fs::write(
        workspace.config_home.join("settings.json"),
        r#"{"policy":{"allowedProviders":["anthropic"]}}"#,
    )
    .expect("write user settings");

    let output = workspace.run(
        &[
            "--provider",
            "openai",
            "--model",
            "gpt-5-codex",
            "prompt",
            "hi",
        ],
        &[("OPENAI_API_KEY", "test-key")],
    );

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("provider `openai` is not allowed by user settings"),
        "{}",
        stderr(&output)
    );
}
