//! Process-wide analytics for the CLI.
//!
//! The client is built once from the environment and the user's settings, and
//! shared by every session the process runs. This module is the only place the
//! CLI reads analytics environment variables. The library itself reads none.

use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use code_analytics::{AnalyticsClient, Consent, DisabledReason, Environment, Event, ProviderKind};
use code_api::{detect_provider_kind, ProviderKind as ProviderFamily};
use code_runtime::{default_config_home, ConfigLoader};
use serde_json::{Map, Value};

const ENV_ENABLED: &str = "FRONTAL_CODE_ANALYTICS_ENABLED";
const ENV_DISABLED: &str = "FRONTAL_CODE_ANALYTICS_DISABLED";
const ENV_API_KEY: &str = "FRONTAL_CODE_POSTHOG_KEY";

static CLIENT: OnceLock<AnalyticsClient> = OnceLock::new();
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Records that the user stopped the session with Ctrl-C.
pub fn mark_interrupted() {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

/// Whether the user stopped the session with Ctrl-C.
pub fn was_interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

/// Builds the process client. Later calls are no-ops, so the first call wins.
pub fn init(no_analytics_flag: bool) {
    let _ = CLIENT.get_or_init(|| build(no_analytics_flag));
}

fn build(no_analytics_flag: bool) -> AnalyticsClient {
    let user_settings = env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(cwd).load().ok());
    let analytics_settings = user_settings
        .as_ref()
        .map(code_runtime::RuntimeConfig::analytics);

    let api_key = env::var(ENV_API_KEY)
        .ok()
        .or_else(|| option_env!("FRONTAL_CODE_POSTHOG_KEY").map(str::to_string));

    let mut builder = AnalyticsClient::builder()
        .api_key(api_key)
        .app_version(env!("CARGO_PKG_VERSION"))
        .environment(if cfg!(debug_assertions) {
            Environment::Development
        } else {
            Environment::Production
        })
        .settings_enabled(
            analytics_settings.and_then(code_runtime::RuntimeAnalyticsConfig::enabled),
        )
        .env_enabled(code_analytics::is_truthy(
            env::var(ENV_ENABLED).ok().as_deref(),
        ))
        .env_disabled(code_analytics::is_truthy(
            env::var(ENV_DISABLED).ok().as_deref(),
        ))
        .do_not_track(code_analytics::is_truthy(
            env::var("DO_NOT_TRACK").ok().as_deref(),
        ))
        .ci(env::var_os("CI").is_some_and(|value| !value.is_empty()))
        .cli_opt_out(no_analytics_flag);

    if let Some(host) = analytics_settings.and_then(|settings| settings.host()) {
        builder = builder.host(host);
    }
    builder = builder.config_home(code_runtime::default_config_home());
    builder.build()
}

/// The process client. Before [`init`] runs, this is a disabled client.
pub fn client() -> AnalyticsClient {
    CLIENT
        .get()
        .cloned()
        .unwrap_or_else(|| AnalyticsClient::disabled(DisabledReason::NotEnabled))
}

/// Records an event on the process client.
pub fn capture(event: &Event) {
    client().capture(event);
}

/// Flushes and shuts down the process client. Safe to call more than once.
pub fn shutdown() {
    if let Some(client) = CLIENT.get() {
        client.shutdown();
    }
}

/// Shuts analytics down, then exits. Use this instead of `std::process::exit`
/// so buffered events are not lost.
pub fn exit(code: i32) -> ! {
    shutdown();
    std::process::exit(code)
}

/// Human-readable status for `/telemetry`.
pub fn status_line() -> String {
    match client().consent() {
        Consent::Enabled => "enabled (anonymous; see docs/ANALYTICS.md)".to_string(),
        Consent::Disabled(reason) => format!("disabled: {reason}"),
    }
}

/// Asks once whether to turn analytics on. Only runs in an interactive terminal
/// and only when nothing else has decided the question. The answer is saved to
/// the user settings file, so it is never asked again. The default is no.
pub fn prompt_first_run(no_analytics_flag: bool) {
    if no_analytics_flag || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return;
    }
    let decided_by_env = [ENV_ENABLED, ENV_DISABLED, "DO_NOT_TRACK", "CI"]
        .iter()
        .any(|name| env::var_os(name).is_some());
    if decided_by_env {
        return;
    }

    let settings_path = default_config_home().join("settings.json");
    let mut root = match read_user_settings(&settings_path) {
        Some(root) => root,
        // Missing file: start from an empty object. Unreadable or non-object file: leave it alone.
        None if !settings_path.exists() => Map::new(),
        None => return,
    };
    let already_decided = root
        .get("analytics")
        .and_then(Value::as_object)
        .is_some_and(|analytics| analytics.contains_key("enabled"));
    if already_decided {
        return;
    }

    println!("Frontal Code can send anonymous usage events (which commands and tools run, and where errors happen).");
    println!("No prompts, file paths, or code are included. Details: docs/ANALYTICS.md");
    print!("Share anonymous usage events? [y/N] ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    if io::stdin().lock().read_line(&mut answer).is_err() {
        return;
    }
    let enabled = matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes");

    let analytics = root
        .entry("analytics")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(analytics) = analytics.as_object_mut() {
        analytics.insert("enabled".to_string(), Value::Bool(enabled));
    }
    let _ = write_user_settings(&settings_path, &root);
    println!(
        "{}",
        if enabled {
            "Thanks. Analytics is on. Turn it off any time with FRONTAL_CODE_ANALYTICS_DISABLED=1 or in settings."
        } else {
            "Analytics stays off. You can turn it on later in settings."
        }
    );
}

fn read_user_settings(path: &Path) -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn write_user_settings(path: &Path, root: &Map<String, Value>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text =
        serde_json::to_string_pretty(&Value::Object(root.clone())).map_err(io::Error::other)?;
    std::fs::write(path, format!("{text}\n"))
}

/// Deletes the anonymous install ID. A new one is created on the next event.
pub fn reset_install_id() -> io::Result<()> {
    code_analytics::reset_install_id(&default_config_home())
}

/// Analytics provider label for a session. An explicit `--provider` wins over the model name.
pub fn provider_for_session(model: &str, provider_override: Option<&str>) -> ProviderKind {
    provider_override.map_or_else(|| provider_for_model(model), provider_for_name)
}

/// Maps a model name to the provider family reported in analytics.
pub fn provider_for_model(model: &str) -> ProviderKind {
    match detect_provider_kind(model) {
        ProviderFamily::Anthropic => ProviderKind::Anthropic,
        ProviderFamily::OpenAi => ProviderKind::OpenAi,
        ProviderFamily::Xai => ProviderKind::Xai,
        ProviderFamily::Frontal => ProviderKind::Frontal,
        _ => ProviderKind::Other,
    }
}

/// Maps a `login` provider name to the provider family reported in analytics.
pub fn provider_for_name(name: &str) -> ProviderKind {
    match name.trim().to_ascii_lowercase().as_str() {
        "anthropic" => ProviderKind::Anthropic,
        "openai" => ProviderKind::OpenAi,
        "xai" => ProviderKind::Xai,
        "frontal" => ProviderKind::Frontal,
        _ => ProviderKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_analytics::DisabledReason;

    #[test]
    fn provider_names_map_to_known_families() {
        assert_eq!(provider_for_name("Anthropic"), ProviderKind::Anthropic);
        assert_eq!(provider_for_name(" openai "), ProviderKind::OpenAi);
        assert_eq!(
            provider_for_name("some-custom-gateway"),
            ProviderKind::Other
        );
    }

    #[test]
    fn claude_models_report_as_anthropic() {
        assert_eq!(
            provider_for_model("claude-opus-5-5"),
            ProviderKind::Anthropic
        );
    }

    #[test]
    fn explicit_provider_overrides_model_name() {
        assert_eq!(
            provider_for_session("claude-opus-5-5", Some("openai")),
            ProviderKind::OpenAi
        );
        assert_eq!(
            provider_for_session("claude-opus-5-5", None),
            ProviderKind::Anthropic
        );
    }

    #[test]
    fn client_before_init_is_disabled() {
        assert_eq!(
            client().consent(),
            Consent::Disabled(DisabledReason::NotEnabled)
        );
    }
}
