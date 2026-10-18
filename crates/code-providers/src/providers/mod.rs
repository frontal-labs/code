#![allow(clippy::cast_possible_truncation)]
use std::future::Future;
use std::pin::Pin;

use serde::Serialize;

use crate::error::ApiError;
use crate::prompt_cache::PromptCache;
use crate::types::{MessageRequest, MessageResponse};

pub mod anthropic;
pub mod azure;
pub mod bedrock;
pub mod frontal;
pub mod ollama;
pub mod openai;
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
        "fable",
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
        "grok-code",
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
        "grok-build",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-4.7",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-4.6",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-4.5",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-4.3",
        ProviderMetadata {
            provider: ProviderKind::Xai,
            auth_env: "XAI_API_KEY",
            base_url_env: "XAI_BASE_URL",
            default_base_url: xai::DEFAULT_BASE_URL,
        },
    ),
    (
        "grok-build-0.1",
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
                    "opus" => "claude-opus-5-5",
                    "sonnet" => "claude-sonnet-5-5",
                    "haiku" => "claude-haiku-5-5",
                    "fable" => "claude-fable-5-1",
                    _ => trimmed,
                },
                ProviderKind::Xai => match *alias {
                    "grok" | "grok-4.7" => "grok-4.7",
                    "grok-4.6" => "grok-4.6",
                    "grok-4.5" => "grok-4.5",
                    "grok-mini" | "grok-4.3" => "grok-4.3",
                    "grok-code" | "grok-build" | "grok-build-0.1" => "grok-build-0.1",
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
    if canonical.starts_with("gpt")
        || canonical.starts_with("o1")
        || canonical.starts_with("o3")
        || canonical.starts_with("o4")
    {
        return Some(ProviderMetadata {
            provider: ProviderKind::OpenAi,
            auth_env: "OPENAI_API_KEY",
            base_url_env: "OPENAI_BASE_URL",
            default_base_url: openai::DEFAULT_BASE_URL,
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
    if openai::has_api_key("OPENAI_API_KEY") {
        return ProviderKind::OpenAi;
    }
    if openai::has_api_key("XAI_API_KEY") {
        return ProviderKind::Xai;
    }
    if openai::has_api_key("FRONTAL_API_KEY") {
        return ProviderKind::Frontal;
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

#[must_use = "the constructed provider client must be used to send requests"]
pub fn create_provider_client(
    provider: &str,
    model: String,
) -> Result<crate::client::ProviderClient, ApiError> {
    match provider.to_lowercase().as_str() {
        "anthropic" => Ok(crate::client::ProviderClient::Anthropic(
            anthropic::AnthropicClient::from_env()?
                .with_base_url(anthropic::read_base_url())
                .with_prompt_cache(PromptCache::new("default")),
        )),
        "openai" => Ok(crate::client::ProviderClient::OpenAi(
            openai::OpenAiCompatClient::from_env(openai::config())?,
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
            if canonical.contains("gpt-4.1") {
                32_768
            } else if canonical.contains("o1")
                || canonical.contains("o3")
                || canonical.contains("o4")
            {
                100_000
            } else if canonical.contains("opus")
                || canonical.contains("sonnet")
                || canonical.contains("haiku")
                || canonical.contains("fable")
                || canonical.contains("mythos")
                || canonical.contains("gpt")
                || canonical.contains("grok")
            {
                128_000
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
        // Current Anthropic lineup (Oct 2026): 1M-token context window and a
        // 128K output ceiling on the synchronous Messages API.
        "claude-opus-5-5" | "claude-sonnet-5-5" | "claude-haiku-5-5" | "claude-fable-5-1"
        // Previous generation, still served.
        | "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6"
        | "claude-opus-4-5" | "claude-sonnet-5" | "claude-sonnet-4-6" | "claude-sonnet-4-5" => {
            Some(ModelTokenLimit {
                max_output_tokens: 128_000,
                context_window_tokens: 1_000_000,
            })
        }
        "claude-haiku-4-5" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 200_000,
        }),
        // Current OpenAI lineup (Oct 2026): GPT-6 / GPT-5.6 / GPT-5.5 ship a
        // ~1.05M context window with 128K max output.
        "gpt-6-astra" | "gpt-6.1-sol" | "gpt-6-luna" | "gpt-5.6-sol" | "gpt-5.6-terra"
        | "gpt-5.6-luna" | "gpt-5.5" | "gpt-5.4" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 1_050_000,
        }),
        // GPT-5 / 5.1 / 5.3-codex generation: 400K context, 128K max output.
        "gpt-5.3-codex" | "gpt-5.1" | "gpt-5" | "gpt-5-mini" | "gpt-5-nano"
        | "gpt-5.4-mini" | "gpt-5.4-nano" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 400_000,
        }),
        "gpt-4.1" | "gpt-4.1-mini" | "gpt-4.1-nano" => Some(ModelTokenLimit {
            max_output_tokens: 32_768,
            context_window_tokens: 1_047_576,
        }),
        "o1" | "o3" | "o3-mini" | "o4-mini" => Some(ModelTokenLimit {
            max_output_tokens: 100_000,
            context_window_tokens: 200_000,
        }),
        // Current xAI lineup (Oct 2026). Max output defaults to 128K when
        // `max_completion_tokens` is unset.
        "grok-4.7" | "grok-4.6" | "grok-4.5" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 500_000,
        }),
        "grok-4.3" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 1_000_000,
        }),
        "grok-build-0.1" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 256_000,
        }),
        // Retired xAI generation, kept for existing configs.
        "grok-3" | "grok-3-mini" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 131_072,
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
    fn resolves_grok_aliases() {
        assert_eq!(resolve_model_alias("grok"), "grok-4.7");
        assert_eq!(resolve_model_alias("grok-4.7"), "grok-4.7");
        assert_eq!(resolve_model_alias("grok-4.6"), "grok-4.6");
        assert_eq!(resolve_model_alias("grok-4.5"), "grok-4.5");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-4.3");
        assert_eq!(resolve_model_alias("grok-4.3"), "grok-4.3");
        assert_eq!(resolve_model_alias("grok-code"), "grok-build-0.1");
        assert_eq!(resolve_model_alias("grok-build"), "grok-build-0.1");
        assert_eq!(resolve_model_alias("opus"), "claude-opus-5-5");
        assert_eq!(resolve_model_alias("sonnet"), "claude-sonnet-5-5");
        assert_eq!(resolve_model_alias("haiku"), "claude-haiku-5-5");
        assert_eq!(resolve_model_alias("fable"), "claude-fable-5-1");
        assert_eq!(resolve_model_alias("frontal"), "frontal");
        assert_eq!(resolve_model_alias("bedrock"), "bedrock");
        assert_eq!(resolve_model_alias("azure"), "azure");
    }

    #[test]
    fn detects_provider_from_model_name_first() {
        assert_eq!(detect_provider_kind("grok"), ProviderKind::Xai);
        assert_eq!(detect_provider_kind("grok-4.7"), ProviderKind::Xai);
        assert_eq!(detect_provider_kind("grok-build-0.1"), ProviderKind::Xai);
        assert_eq!(detect_provider_kind("gpt-6.1-sol"), ProviderKind::OpenAi);
        assert_eq!(detect_provider_kind("gpt-5.3-codex"), ProviderKind::OpenAi);
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
            detect_provider_kind("claude-sonnet-5-5"),
            ProviderKind::Anthropic
        );
    }

    #[test]
    fn keeps_existing_max_token_heuristic() {
        assert_eq!(max_tokens_for_model("opus"), 128_000);
        assert_eq!(max_tokens_for_model("sonnet"), 128_000);
        assert_eq!(max_tokens_for_model("grok-4.7"), 128_000);
        assert_eq!(max_tokens_for_model("gpt-6.1-sol"), 128_000);
        assert_eq!(max_tokens_for_model("gpt-4.1"), 32_768);
    }

    #[test]
    fn returns_context_window_metadata_for_supported_models() {
        assert_eq!(
            model_token_limit("claude-sonnet-5-5")
                .expect("claude-sonnet-5-5 should be registered")
                .context_window_tokens,
            1_000_000
        );
        assert_eq!(
            model_token_limit("opus")
                .expect("opus should resolve to a registered model")
                .context_window_tokens,
            1_000_000
        );
        assert_eq!(
            model_token_limit("haiku")
                .expect("haiku should resolve to a registered model")
                .context_window_tokens,
            1_000_000
        );
        assert_eq!(
            model_token_limit("claude-haiku-4-5")
                .expect("legacy haiku should stay registered")
                .context_window_tokens,
            200_000
        );
        assert_eq!(
            model_token_limit("fable")
                .expect("fable should resolve to a registered model")
                .context_window_tokens,
            1_000_000
        );
        assert_eq!(
            model_token_limit("grok-mini")
                .expect("grok-mini should resolve to a registered model")
                .context_window_tokens,
            1_000_000
        );
        assert_eq!(
            model_token_limit("grok-code")
                .expect("grok-code should resolve to the code model")
                .context_window_tokens,
            256_000
        );
        assert_eq!(
            model_token_limit("gpt-6.1-sol")
                .expect("gpt-6.1-sol should be registered")
                .context_window_tokens,
            1_050_000
        );
        assert_eq!(
            model_token_limit("gpt-5.3-codex")
                .expect("gpt-5.3-codex should be registered")
                .context_window_tokens,
            400_000
        );
    }

    #[test]
    fn preflight_blocks_requests_that_exceed_the_model_context_window() {
        let request = MessageRequest {
            model: "claude-sonnet-5-5".to_string(),
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
                assert_eq!(model, "claude-sonnet-5-5");
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
