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

/// A client for the Ollama server at `root` (for example `http://localhost:11434`).
#[must_use]
pub fn client(root: &str) -> OpenAiCompatClient {
    OpenAiCompatClient::new("ollama", config())
        .with_base_url(format!("{}/v1", root.trim_end_matches('/')))
}

/// A client for the server named by `OLLAMA_BASE_URL`, or the local default.
#[must_use]
pub fn client_from_env() -> OpenAiCompatClient {
    let root = std::env::var("OLLAMA_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    client(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_gets_the_v1_suffix_once() {
        assert_eq!(client("http://127.0.0.1:11434/").base_url_for_tests(), "http://127.0.0.1:11434/v1");
        assert_eq!(client("http://127.0.0.1:11434").base_url_for_tests(), "http://127.0.0.1:11434/v1");
    }
}
