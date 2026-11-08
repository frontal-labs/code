//! Ollama, through its OpenAI-compatible endpoint at `<root>/v1`.
//!
//! Ollama serves chat, tool calls, streaming, and token usage on that endpoint, so this
//! provider reuses the OpenAI-compatible client rather than keeping a second wire format.
//! No API key is needed; the client sends a placeholder that Ollama ignores.

use super::openai::{AuthScheme, OpenAiCompatClient, OpenAiCompatConfig};

/// Where a local Ollama server listens by default. This is the server root, not `/v1`.
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

#[must_use]
pub const fn config() -> OpenAiCompatConfig {
    OpenAiCompatConfig {
        provider_name: "Ollama",
        api_key_env: "OLLAMA_API_KEY",
        credential_env_vars: &[],
        base_url_env: "OLLAMA_BASE_URL",
        default_base_url: DEFAULT_BASE_URL,
        auth_scheme: AuthScheme::Bearer,
        requires_base_url: false,
    }
}

/// Sent when `OLLAMA_API_KEY` is unset. Ollama ignores the value.
const PLACEHOLDER_KEY: &str = "ollama";

/// A client for the server named by `OLLAMA_BASE_URL`, or the local default. It sends
/// `OLLAMA_API_KEY` as the bearer when that is set. Hosted workers rely on this: their
/// token travels in that variable so the server's model proxy can identify the task.
#[must_use]
pub fn client_from_env() -> OpenAiCompatClient {
    let root = std::env::var("OLLAMA_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let key = std::env::var("OLLAMA_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| PLACEHOLDER_KEY.to_string());
    with_key(&key, &root)
}

fn with_key(key: &str, root: &str) -> OpenAiCompatClient {
    OpenAiCompatClient::new(key, config())
        .with_base_url(format!("{}/v1", root.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_gets_the_v1_suffix_once() {
        assert_eq!(
            with_key(PLACEHOLDER_KEY, "http://127.0.0.1:11434/").base_url_for_tests(),
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            with_key(PLACEHOLDER_KEY, "http://127.0.0.1:11434").base_url_for_tests(),
            "http://127.0.0.1:11434/v1"
        );
    }
}
