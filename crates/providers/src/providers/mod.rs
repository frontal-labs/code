#![allow(clippy::cast_possible_truncation)]
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::prompt_cache::PromptCache;
use crate::types::{MessageRequest, MessageResponse};

pub mod anthropic;
pub mod azure;
pub mod bedrock;
pub mod frontal;
pub mod ollama;
pub mod openai;
pub mod openai_responses;
pub mod xai;

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

pub trait Provider {
    type Stream;

    fn send_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, MessageResponse>;

    fn stream_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, Self::Stream>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    Xai,
    OpenAi,
    Frontal,
    Bedrock,
    Azure,
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub provider: ProviderKind,
    pub auth_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelTokenLimit {
    pub max_output_tokens: u32,
    pub context_window_tokens: u32,
}

const MODEL_REGISTRY: &[(&str, ProviderMetadata)] = &[
    (
        "opus",
        ProviderMetadata {
            provider: ProviderKind::Anthropic,
            auth_env: "FRONTAL_API_KEY",
            base_url_env: "FRONTAL_BASE_URL",
            default_base_url: anthropic::DEFAULT_BASE_URL,
        },
    ),
    (
        "sonnet",
        ProviderMetadata {
            provider: ProviderKind::Anthropic,
            auth_env: "FRONTAL_API_KEY",
            base_url_env: "FRONTAL_BASE_URL",
            default_base_url: anthropic::DEFAULT_BASE_URL,
        },
    ),
    (
        "haiku",
        ProviderMetadata {
            provider: ProviderKind::Anthropic,
            auth_env: "FRONTAL_API_KEY",
            base_url_env: "FRONTAL_BASE_URL",
            default_base_url: anthropic::DEFAULT_BASE_URL,
        },
    ),
    (
        "frontal",
        ProviderMetadata {
            provider: ProviderKind::Frontal,
            auth_env: "FRONTAL_API_KEY",
            base_url_env: "FRONTAL_BASE_URL",
            default_base_url: frontal::DEFAULT_BASE_URL,
        },
    ),
    (
        "bedrock",
        ProviderMetadata {
            provider: ProviderKind::Bedrock,
            auth_env: "BEDROCK_API_KEY",
            base_url_env: "BEDROCK_BASE_URL",
            default_base_url: bedrock::DEFAULT_BASE_URL,
        },
    ),
    (
        "azure",
        ProviderMetadata {
            provider: ProviderKind::Azure,
            auth_env: "AZURE_OPENAI_API_KEY",
            base_url_env: "AZURE_OPENAI_BASE_URL",
            default_base_url: azure::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-3",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-mini",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-3-mini",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-2",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
];

#[must_use]
pub fn resolve_model_alias(model: &str) -> String {
    let trimmed = model.trim();
    let lower = trimmed.to_ascii_lowercase();
    MODEL_REGISTRY
        .iter()
        .find_map(|(alias, metadata)| {
            (*alias == lower).then_some(match metadata.provider {
                ProviderKind::Anthropic => match *alias {
                    "opus" => "claude-opus-5",
                    "sonnet" => "claude-sonnet-5",
                    "haiku" => "claude-haiku-4-5",
                    _ => trimmed,
                },
                ProviderKind::Xai => match *alias {
                    "grok" | "grok-3" => "grok-3",
                    "grok-mini" | "grok-3-mini" => "grok-3-mini",
                    "grok-2" => "grok-2",
                    _ => trimmed,
                },
                ProviderKind::Frontal
                | ProviderKind::Bedrock
                | ProviderKind::Azure
                | ProviderKind::OpenAi
                | ProviderKind::Ollama => trimmed,
            })
        })
        .map_or_else(|| trimmed.to_string(), ToOwned::to_owned)
}

#[must_use]
pub fn metadata_for_model(model: &str) -> Option<ProviderMetadata> {
    let canonical = resolve_model_alias(model);
    if canonical.starts_with("claude") {
        return Some(ProviderMetadata {
            provider: ProviderKind::Anthropic,
            auth_env: "FRONTAL_API_KEY",
            base_url_env: "FRONTAL_BASE_URL",
            default_base_url: anthropic::DEFAULT_BASE_URL,
        });
    }
    if canonical.starts_with("grok") {
        return Some(ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        });
    }
    if canonical.starts_with("frontal/") || canonical.starts_with("frontal:") {
        return Some(ProviderMetadata {
            provider: ProviderKind::Frontal,
            auth_env: "FRONTAL_API_KEY",
            base_url_env: "FRONTAL_BASE_URL",
            default_base_url: frontal::DEFAULT_BASE_URL,
        });
    }
    if canonical.starts_with("bedrock/") || canonical.starts_with("bedrock:") {
        return Some(ProviderMetadata {
            provider: ProviderKind::Bedrock,
            auth_env: "BEDROCK_API_KEY",
            base_url_env: "BEDROCK_BASE_URL",
            default_base_url: bedrock::DEFAULT_BASE_URL,
        });
    }
    if canonical.starts_with("azure/") || canonical.starts_with("azure:") {
        return Some(ProviderMetadata {
            provider: ProviderKind::Azure,
            auth_env: "AZURE_OPENAI_API_KEY",
            base_url_env: "AZURE_OPENAI_BASE_URL",
            default_base_url: azure::DEFAULT_BASE_URL,
        });
    }
    // Codex-family models are OpenAI models served by the Responses API.
    if openai_responses::model_requires_responses(&canonical) {
        return Some(ProviderMetadata {
            provider: ProviderKind::OpenAi,
            auth_env: "OPENAI_API_KEY",
            base_url_env: "OPENAI_BASE_URL",
            default_base_url: openai::DEFAULT_BASE_URL,
        });
    }
    None
}

#[must_use]
pub fn detect_provider_kind(model: &str) -> ProviderKind {
    if let Some(metadata) = metadata_for_model(model) {
        return metadata.provider;
    }
    if openai::has_api_key("FRONTAL_API_KEY") || openai::has_api_key("FRONTAL_AUTH_TOKEN") {
        return ProviderKind::Anthropic;
    }
    if openai::has_api_key("OPENAI_API_KEY") || has_saved_oauth("openai") {
        return ProviderKind::OpenAi;
    }
    if openai::has_api_key("XAI_API_KEY") {
        return ProviderKind::Xai;
    }
    if openai::has_api_key("BEDROCK_API_KEY") {
        return ProviderKind::Bedrock;
    }
    if openai::has_api_key("AZURE_OPENAI_API_KEY") {
        return ProviderKind::Azure;
    }
    // Default to Ollama if no other auth is available
    ProviderKind::Ollama
}

/// Token-endpoint config for refreshing `provider`'s saved login.
///
/// Built only from the endpoint recorded when the login happened. Project or user
/// settings are never consulted here, so a repository cannot redirect a refresh.
pub(crate) fn stored_refresh_config(
    provider: &str,
) -> Result<Option<frontal_code_runtime::OAuthConfig>, ApiError> {
    let Some(endpoint) = frontal_code_runtime::load_oauth_endpoint_for(
        frontal_code_runtime::credentials_key_for_provider(provider),
    )
    .map_err(ApiError::from)?
    else {
        return Ok(None);
    };
    validate_oauth_endpoint(&endpoint.token_url)?;
    Ok(Some(frontal_code_runtime::OAuthConfig {
        client_id: endpoint.client_id,
        authorize_url: String::new(),
        token_url: endpoint.token_url,
        callback_port: None,
        manual_redirect_url: None,
        scopes: Vec::new(),
    }))
}

/// Rejects token endpoints that would send credentials in clear text. Plain HTTP is
/// allowed only for the loopback host, which local test servers use. The host is
/// compared exactly, so `http://127.0.0.1.example.com` is refused.
pub fn validate_oauth_endpoint(url: &str) -> Result<(), ApiError> {
    if url.starts_with("https://") {
        return Ok(());
    }
    let loopback_host = url
        .strip_prefix("http://")
        .and_then(|rest| rest.split(['/', ':', '?']).next());
    match loopback_host {
        Some("127.0.0.1" | "localhost") => Ok(()),
        _ => Err(ApiError::Auth(format!(
            "OAuth endpoint must use https (or loopback http for testing): {url}"
        ))),
    }
}

/// HTTP client for OAuth token requests. Redirects are not followed, so the refresh
/// token and authorization code are never re-sent to a host the endpoint redirected to.
pub(crate) fn oauth_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("OAuth HTTP client with redirects disabled should build")
}

/// Serializes every test that mutates process env vars or the credentials home,
/// across all provider modules, so parallel tests cannot observe each other's state.
#[cfg(test)]
pub(crate) fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Name a provider goes by in the project policy (`allowedProviders`).
#[must_use]
pub fn policy_provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAi => "openai",
        ProviderKind::Xai => "xai",
        ProviderKind::Frontal => "frontal",
        ProviderKind::Bedrock => "bedrock",
        ProviderKind::Azure => "azure",
        ProviderKind::Ollama => "ollama",
    }
}

/// The effective project policy for the current directory (user, project, and local
/// settings combined).
pub fn load_project_policy() -> Result<frontal_code_runtime::ProjectPolicy, ApiError> {
    let cwd = std::env::current_dir().map_err(|error| ApiError::Auth(error.to_string()))?;
    frontal_code_runtime::ConfigLoader::default_for(cwd)
        .load()
        .map(|config| config.policy().clone())
        .map_err(|error| ApiError::Auth(error.to_string()))
}

/// Refuses a provider, model, or auth method the project policy does not allow.
/// Call before any request is sent or any client is built for that provider.
pub fn enforce_provider_policy(provider: &str, model: &str) -> Result<(), ApiError> {
    let policy = load_project_policy()?;
    let model = resolve_model_alias(model);
    policy
        .check_request(Some(provider), Some(&model), auth_method_for(provider))
        .map_err(|violation| ApiError::Auth(violation.to_string()))
}

/// Policy check for a model when the provider is inferred from the model name.
pub fn enforce_policy_for_model(model: &str) -> Result<(), ApiError> {
    let kind = detect_provider_kind(&resolve_model_alias(model));
    enforce_provider_policy(policy_provider_name(kind), model)
}

/// How `provider` is currently authenticated: a static key or a saved login. `None`
/// when neither is set, in which case the client constructor reports the error.
fn auth_method_for(provider: &str) -> Option<frontal_code_runtime::AuthMethod> {
    let env_keys: &[&str] = match provider {
        "anthropic" => &["FRONTAL_API_KEY", "FRONTAL_AUTH_TOKEN"],
        "openai" => &["OPENAI_API_KEY"],
        "xai" => &["XAI_API_KEY"],
        "frontal" => &["FRONTAL_API_KEY"],
        "bedrock" => &["BEDROCK_API_KEY"],
        "azure" => &["AZURE_OPENAI_API_KEY"],
        _ => &[],
    };
    if env_keys.iter().any(|key| openai::has_api_key(key)) {
        return Some(frontal_code_runtime::AuthMethod::ApiKey);
    }
    if has_saved_oauth(provider) {
        return Some(frontal_code_runtime::AuthMethod::OAuth);
    }
    None
}

/// True when credentials were saved for `provider` by `frontal-code login`.
#[must_use]
pub fn has_saved_oauth(provider: &str) -> bool {
    frontal_code_runtime::load_oauth_credentials_for(
        frontal_code_runtime::credentials_key_for_provider(provider),
    )
    .ok()
    .flatten()
    .is_some()
}

#[derive(Deserialize)]
struct RefreshedTokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    #[serde(default)]
    scopes: Vec<String>,
}

/// Returns a bearer token from the credentials saved for `provider`, refreshing it
/// first when it has expired. Returns `None` when nothing is saved for the provider.
///
/// Unlike the Anthropic-only helpers, this reads and writes the per-provider entry,
/// so refreshing one provider's token never touches another's.
pub(crate) fn saved_oauth_bearer(provider: &str) -> Result<Option<String>, ApiError> {
    let key = frontal_code_runtime::credentials_key_for_provider(provider);
    let Some(saved) = load_saved_login(key)? else {
        return Ok(None);
    };
    if !is_expired(&saved) {
        return Ok(Some(saved.access_token));
    }
    // Refresh tokens can rotate and are usually valid once. Hold the lock while
    // refreshing, then re-read: another process may have refreshed while we waited.
    let _lock = lock_credentials_for_refresh()?;
    let Some(saved) = load_saved_login(key)? else {
        return Ok(None);
    };
    if !is_expired(&saved) {
        return Ok(Some(saved.access_token));
    }
    refresh_saved_login(provider, key, saved).map(Some)
}

fn load_saved_login(key: &str) -> Result<Option<frontal_code_runtime::OAuthTokenSet>, ApiError> {
    frontal_code_runtime::load_oauth_credentials_for(key).map_err(ApiError::from)
}

fn is_expired(saved: &frontal_code_runtime::OAuthTokenSet) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    saved.expires_at.is_some_and(|expires_at| expires_at <= now)
}

/// Exclusive lock over `credentials.json` for the duration of a refresh. Held by
/// dropping the returned file. Only taken when a refresh is needed.
pub(crate) fn lock_credentials_for_refresh() -> Result<std::fs::File, ApiError> {
    let path = frontal_code_runtime::credentials_path()
        .map_err(ApiError::from)?
        .with_extension("json.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ApiError::from)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(ApiError::from)?;
    file.lock().map_err(ApiError::from)?;
    Ok(file)
}

/// Refreshes an expired saved login and stores the result under `key`. The caller
/// must hold the refresh lock.
fn refresh_saved_login(
    provider: &str,
    key: &str,
    saved: frontal_code_runtime::OAuthTokenSet,
) -> Result<String, ApiError> {
    let Some(refresh_token) = saved.refresh_token.clone() else {
        return Err(ApiError::ExpiredOAuthToken);
    };
    let config = stored_refresh_config(provider)?.ok_or_else(|| {
        ApiError::Auth(
            "saved OAuth token is expired and has no recorded token endpoint; run `frontal-code login` again"
                .to_string(),
        )
    })?;
    let refreshed = anthropic::client_runtime_block_on(refresh_oauth_grant(
        &config,
        refresh_token.clone(),
        saved.scopes.clone(),
    ))?;
    let resolved = frontal_code_runtime::OAuthTokenSet {
        access_token: refreshed.access_token.clone(),
        // Providers may omit a rotated refresh token; keep the one we already have.
        refresh_token: refreshed.refresh_token.or(Some(refresh_token)),
        expires_at: refreshed.expires_at,
        scopes: if refreshed.scopes.is_empty() {
            saved.scopes
        } else {
            refreshed.scopes
        },
    };
    frontal_code_runtime::save_oauth_credentials_for(&resolved, key).map_err(ApiError::from)?;
    Ok(refreshed.access_token)
}

async fn refresh_oauth_grant(
    config: &frontal_code_runtime::OAuthConfig,
    refresh_token: String,
    scopes: Vec<String>,
) -> Result<RefreshedTokens, ApiError> {
    let request =
        frontal_code_runtime::OAuthRefreshRequest::from_config(config, refresh_token, Some(scopes));
    post_token_request(config, &request.form_params()).await
}

/// POSTs a form-encoded grant (authorization code or refresh) to the token endpoint.
async fn post_token_request(
    config: &frontal_code_runtime::OAuthConfig,
    form: &std::collections::BTreeMap<&str, String>,
) -> Result<RefreshedTokens, ApiError> {
    validate_oauth_endpoint(&config.token_url)?;
    let response = oauth_http_client()
        .post(&config.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .form(form)
        .send()
        .await
        .map_err(ApiError::from)?;
    if !response.status().is_success() {
        return Err(ApiError::Auth(format!(
            "OAuth token request failed with status {}",
            response.status()
        )));
    }
    response
        .json::<RefreshedTokens>()
        .await
        .map_err(ApiError::from)
}

/// Exchanges an authorization code for tokens and saves them under `provider`.
/// Called by `frontal-code login` once the browser redirect has been received.
pub fn complete_oauth_login(
    provider: &str,
    config: &frontal_code_runtime::OAuthConfig,
    request: &frontal_code_runtime::OAuthTokenExchangeRequest,
) -> Result<(), ApiError> {
    let tokens =
        anthropic::client_runtime_block_on(post_token_request(config, &request.form_params()))?;
    // Record the endpoint these tokens came from, so refreshes go only there.
    let endpoint = frontal_code_runtime::OAuthEndpoint {
        client_id: config.client_id.clone(),
        token_url: config.token_url.clone(),
    };
    frontal_code_runtime::save_oauth_login_for(
        &frontal_code_runtime::OAuthTokenSet {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            scopes: tokens.scopes,
        },
        frontal_code_runtime::credentials_key_for_provider(provider),
        &endpoint,
    )
    .map_err(ApiError::from)
}

#[must_use = "the constructed provider client must be used to send requests"]
pub fn create_provider_client(
    provider: &str,
    model: String,
) -> Result<crate::client::ProviderClient, ApiError> {
    enforce_provider_policy(&provider.to_ascii_lowercase(), &model)?;
    match provider.to_lowercase().as_str() {
        "anthropic" => {
            enforce_provider_policy("anthropic", model.as_str())?;
            let auth = anthropic::resolve_startup_auth_source()?;
            Ok(crate::client::ProviderClient::Anthropic(
                anthropic::AnthropicClient::from_auth(auth)
                    .with_base_url(anthropic::read_base_url())
                    .with_prompt_cache(PromptCache::new("default")),
            ))
        }
        "openai" => Ok(crate::client::ProviderClient::OpenAi(
            openai::OpenAiCompatClient::from_env_or_saved(openai::config(), "openai")?,
        )),
        "xai" => Ok(crate::client::ProviderClient::Xai(
            openai::OpenAiCompatClient::from_env(xai::config())?,
        )),
        "frontal" => Ok(crate::client::ProviderClient::Frontal(
            openai::OpenAiCompatClient::from_env(frontal::config())?,
        )),
        "bedrock" => Ok(crate::client::ProviderClient::Bedrock(
            openai::OpenAiCompatClient::from_env(bedrock::config())?,
        )),
        "azure" => Ok(crate::client::ProviderClient::Azure(
            openai::OpenAiCompatClient::from_env(azure::config())?,
        )),
        "ollama" => Ok(crate::client::ProviderClient::Ollama(
            ollama::OllamaClient::new(
                std::env::var("OLLAMA_BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:11434".to_string()),
                model,
            ),
        )),
        _ => Err(ApiError::Auth(format!("Unknown provider: {provider}"))),
    }
}

#[must_use]
pub fn max_tokens_for_model(model: &str) -> u32 {
    model_token_limit(model).map_or_else(
        || {
            let canonical = resolve_model_alias(model);
            if canonical.contains("opus") {
                32_000
            } else {
                64_000
            }
        },
        |limit| limit.max_output_tokens,
    )
}

#[must_use]
pub fn model_token_limit(model: &str) -> Option<ModelTokenLimit> {
    let canonical = resolve_model_alias(model);
    match canonical.as_str() {
        // The Claude 5 family and the 4.6/4.7/4.8 Opus and Sonnet tiers all ship
        // a 1M-token context window and a 128K output ceiling.
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6"
        | "claude-sonnet-5" | "claude-sonnet-4-6" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 1_000_000,
        }),
        "claude-haiku-4-5" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 200_000,
        }),
        "grok-3" | "grok-3-mini" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 131_072,
        }),
        // OpenAI model reference: 400K context window, 128K max output.
        "gpt-5-codex" | "gpt-5.3-codex" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 400_000,
        }),
        _ => None,
    }
}

pub fn preflight_message_request(request: &MessageRequest) -> Result<(), ApiError> {
    let Some(limit) = model_token_limit(&request.model) else {
        return Ok(());
    };

    let estimated_input_tokens = estimate_message_request_input_tokens(request);
    let estimated_total_tokens = estimated_input_tokens.saturating_add(request.max_tokens);
    if estimated_total_tokens > limit.context_window_tokens {
        return Err(ApiError::ContextWindowExceeded {
            model: resolve_model_alias(&request.model),
            estimated_input_tokens,
            requested_output_tokens: request.max_tokens,
            estimated_total_tokens,
            context_window_tokens: limit.context_window_tokens,
        });
    }

    Ok(())
}

fn estimate_message_request_input_tokens(request: &MessageRequest) -> u32 {
    let mut estimate = estimate_serialized_tokens(&request.messages);
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.system));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tools));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tool_choice));
    estimate
}

fn estimate_serialized_tokens<T: Serialize>(value: &T) -> u32 {
    serde_json::to_vec(value)
        .ok()
        .map_or(0, |bytes| (bytes.len() / 4 + 1) as u32)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::error::ApiError;
    use crate::types::{
        InputContentBlock, InputMessage, MessageRequest, ToolChoice, ToolDefinition,
    };

    use super::{
        detect_provider_kind, max_tokens_for_model, model_token_limit, preflight_message_request,
        resolve_model_alias, ProviderKind,
    };

    #[test]
    fn codex_models_resolve_to_openai_with_documented_limits() {
        for model in ["gpt-5-codex", "gpt-5.3-codex"] {
            let metadata = super::metadata_for_model(model).expect("codex model metadata");
            assert_eq!(metadata.provider, ProviderKind::OpenAi);
            assert_eq!(metadata.auth_env, "OPENAI_API_KEY");
            assert_eq!(super::max_tokens_for_model(model), 128_000);
            assert_eq!(
                super::model_token_limit(model).map(|limit| limit.context_window_tokens),
                Some(400_000)
            );
        }
        // Ordinary OpenAI chat models keep the default path.
        assert!(super::metadata_for_model("gpt-4.1").is_none());
    }

    #[test]
    fn resolves_grok_aliases() {
        assert_eq!(resolve_model_alias("grok"), "grok-3");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-3-mini");
        assert_eq!(resolve_model_alias("grok-2"), "grok-2");
        assert_eq!(resolve_model_alias("frontal"), "frontal");
        assert_eq!(resolve_model_alias("bedrock"), "bedrock");
        assert_eq!(resolve_model_alias("azure"), "azure");
    }

    #[test]
    fn detects_provider_from_model_name_first() {
        assert_eq!(detect_provider_kind("grok"), ProviderKind::Xai);
        assert_eq!(
            detect_provider_kind("frontal/gpt-4.1"),
            ProviderKind::Frontal
        );
        assert_eq!(
            detect_provider_kind("bedrock/anthropic.claude-3-5-sonnet"),
            ProviderKind::Bedrock
        );
        assert_eq!(detect_provider_kind("azure/gpt-4.1"), ProviderKind::Azure);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::Anthropic
        );
    }

    #[test]
    fn keeps_existing_max_token_heuristic() {
        assert_eq!(max_tokens_for_model("opus"), 128_000);
        assert_eq!(max_tokens_for_model("grok-3"), 64_000);
    }

    #[test]
    fn returns_context_window_metadata_for_supported_models() {
        assert_eq!(
            model_token_limit("claude-sonnet-4-6")
                .expect("claude-sonnet-4-6 should be registered")
                .context_window_tokens,
            1_000_000
        );
        assert_eq!(
            model_token_limit("haiku")
                .expect("haiku should resolve to a registered model")
                .context_window_tokens,
            200_000
        );
        assert_eq!(
            model_token_limit("grok-mini")
                .expect("grok-mini should resolve to a registered model")
                .context_window_tokens,
            131_072
        );
    }

    #[test]
    fn preflight_blocks_requests_that_exceed_the_model_context_window() {
        let request = MessageRequest {
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(6_000_000),
                }],
            }],
            system: Some("Keep the answer short.".to_string()),
            tools: Some(vec![ToolDefinition {
                name: "weather".to_string(),
                description: Some("Fetches weather".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": { "city": { "type": "string" } },
                }),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            stream: true,
        };

        let error = preflight_message_request(&request)
            .expect_err("oversized request should be rejected before the provider call");

        match error {
            ApiError::ContextWindowExceeded {
                model,
                estimated_input_tokens,
                requested_output_tokens,
                estimated_total_tokens,
                context_window_tokens,
            } => {
                assert_eq!(model, "claude-sonnet-4-6");
                assert!(estimated_input_tokens > 1_000_000);
                assert_eq!(requested_output_tokens, 64_000);
                assert!(estimated_total_tokens > context_window_tokens);
                assert_eq!(context_window_tokens, 1_000_000);
            }
            other => panic!("expected context-window preflight failure, got {other:?}"),
        }
    }

    #[test]
    fn preflight_skips_unknown_models() {
        let request = MessageRequest {
            model: "unknown-model".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(6_000_000),
                }],
            }],
            system: None,
            tools: None,
            tool_choice: None,
            stream: false,
        };

        preflight_message_request(&request)
            .expect("models without context metadata should skip the guarded preflight");
    }
}
