mod client;
mod error;
mod prompt_cache;
mod providers;
mod sse;
mod types;

pub use client::{
    oauth_token_is_expired, read_azure_base_url, read_base_url, read_bedrock_base_url,
    read_frontal_base_url, read_xai_base_url, resolve_saved_oauth_token,
    resolve_startup_auth_source, MessageStream, OAuthTokenSet, ProviderClient,
};
pub use error::ApiError;
pub use prompt_cache::{
    CacheBreakEvent, PromptCache, PromptCacheConfig, PromptCachePaths, PromptCacheRecord,
    PromptCacheStats,
};
pub use providers::anthropic::{AnthropicClient, AnthropicClient as ApiClient, AuthSource};
pub use providers::openai::{OpenAiCompatClient, OpenAiCompatConfig};
pub use providers::openai_responses::model_requires_responses;
pub use providers::{
    complete_oauth_login, create_provider_client, detect_provider_kind, enforce_policy_for_model,
    enforce_provider_policy, has_saved_oauth, load_project_policy, max_tokens_for_model,
    policy_provider_name, resolve_model_alias, validate_oauth_endpoint, ProviderKind,
};
pub use sse::{parse_frame, SseParser};
pub use types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent,
    InputContentBlock, InputMessage, MessageDelta, MessageDeltaEvent, MessageRequest,
    MessageResponse, MessageStartEvent, MessageStopEvent, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition, ToolResultContentBlock, Usage,
};

pub use code_telemetry::{
    AnalyticsEvent, AnthropicRequestProfile, JsonlTelemetrySink, MemoryTelemetrySink,
    SessionTraceRecord, SessionTracer, TelemetryEvent, TelemetrySink,
};
pub use code_types::{
    ClientIdentity, DEFAULT_APP_NAME, DEFAULT_FRONTAL_CODE_VERSION, DEFAULT_RUNTIME,
};
