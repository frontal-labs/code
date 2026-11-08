//! Credentials for hosted worker containers.
//!
//! A worker must never hold a provider API key or the control-plane key. Instead:
//!
//! - Each task gets a random token. The token authorizes only that task's worker
//!   endpoints (read the task, read its runtime, read and write its GitHub
//!   integration, and post completion).
//! - Model calls go through `/v1/model-proxy/{provider}`. The proxy checks the
//!   token, drops the caller's auth headers, adds the real provider key from the
//!   server's environment, and streams the upstream response back.
//! - The token is revoked when the task completes or is cancelled. Tokens are stored
//!   hashed in Postgres (see `worker_tokens.rs`), so they survive a server restart and
//!   are never held in server memory.

use crate::worker_tokens::WorkerTokenStore;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::Response;

/// Largest request body the proxy forwards. Prompts with large context fit well below this.
pub(crate) const MODEL_PROXY_MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// Request headers the proxy forwards. Authorization headers are deliberately absent:
/// the caller's token must never reach the provider.
const FORWARDED_REQUEST_HEADERS: &[&str] = &[
    "content-type",
    "accept",
    "anthropic-version",
    "anthropic-beta",
    "user-agent",
];

/// Task-scoped worker tokens, backed by Postgres.
#[derive(Debug, Clone)]
pub(crate) struct WorkerAccess {
    store: Option<WorkerTokenStore>,
    ttl_secs: u64,
}

impl WorkerAccess {
    pub(crate) fn new(store: Option<WorkerTokenStore>, ttl_secs: u64) -> Self {
        Self { store, ttl_secs }
    }

    /// Issues a token for `task_id`, replacing the task's earlier token. Synchronous,
    /// because the lane transport is synchronous.
    pub(crate) fn issue(&self, task_id: &str) -> Result<String, String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "no worker token store is configured".to_string())?;
        store.issue_blocking(task_id, self.ttl_secs)
    }

    /// Returns the task a live token belongs to. A store error counts as "no access".
    pub(crate) async fn task_for_token(&self, token: &str) -> Option<String> {
        let store = self.store.as_ref()?;
        match store.validate(token).await {
            Ok(task) => task,
            Err(error) => {
                eprintln!("worker token check failed: {error}");
                None
            }
        }
    }

    /// Revokes the task's token from synchronous code.
    pub(crate) fn revoke(&self, task_id: &str) {
        if let Some(store) = &self.store {
            if let Err(error) = store.revoke_blocking(task_id) {
                eprintln!("worker token revoke failed for {task_id}: {error}");
            }
        }
    }

    /// Revokes the task's token from async code.
    pub(crate) async fn revoke_async(&self, task_id: &str) {
        if let Some(store) = &self.store {
            if let Err(error) = store.revoke(task_id).await {
                eprintln!("worker token revoke failed for {task_id}: {error}");
            }
        }
    }
}

/// Returns the task id when `method` and `path` are one of the endpoints a worker
/// calls for its own task. Any other path returns `None`, so a worker token cannot
/// reach the rest of the control plane.
pub(crate) fn worker_endpoint_task_id(method: &Method, path: &str) -> Option<String> {
    let rest = path.strip_prefix("/v1/tasks/")?;
    let segments: Vec<&str> = rest.split('/').collect();
    let task_id = segments.first().copied().filter(|id| !id.is_empty())?;
    let allowed = match segments.as_slice() {
        [_] | [_, "runtime"] => *method == Method::GET,
        [_, "integration", "github"] => *method == Method::GET || *method == Method::POST,
        [_, "complete" | "publish"] => *method == Method::POST,
        _ => false,
    };
    allowed.then(|| task_id.to_string())
}

/// Extracts a token from `x-api-key` or `Authorization: Bearer`.
pub(crate) fn token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        })
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamAuth {
    Bearer,
    AnthropicKey,
    ApiKeyHeader,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Upstream {
    pub base_url: String,
    pub key: Option<String>,
    pub auth: UpstreamAuth,
}

/// Canonical worker provider names, in the form the CLI accepts for `--provider`.
const WORKER_PROVIDERS: &[&str] = &["anthropic", "frontal", "openai", "xai", "azure", "ollama"];

/// Chooses the provider a task will use. This mirrors the CLI: an explicit provider
/// wins, codex models run on `openai`, and everything else uses the configured default
/// (`frontal`). Bedrock is refused because its requests are signed per request and
/// cannot be proxied with a static key.
pub(crate) fn worker_provider_name(
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<&'static str, String> {
    if let Some(provider) = provider.map(|value| value.trim().to_ascii_lowercase()) {
        return match provider.as_str() {
            "bedrock" => Err(bedrock_unsupported()),
            other => WORKER_PROVIDERS
                .iter()
                .copied()
                .find(|known| *known == other)
                .ok_or_else(|| format!("unsupported provider `{other}` for a worker lane")),
        };
    }
    if model.is_some_and(|model| model.to_ascii_lowercase().contains("codex")) {
        return Ok("openai");
    }
    Ok("frontal")
}

fn bedrock_unsupported() -> String {
    "bedrock is not supported in worker lanes: its requests are signed per request and cannot \
     go through the model proxy"
        .to_string()
}

/// Environment for a worker container. It holds the proxy URL and the task token.
/// It never holds an upstream provider key.
pub(crate) fn worker_provider_env(
    provider_name: &str,
    proxy_base_url: &str,
    token: &str,
) -> Vec<(String, String)> {
    let base = format!(
        "{}/v1/model-proxy/{provider_name}",
        proxy_base_url.trim_end_matches('/')
    );
    match provider_name {
        "openai" => vec![
            ("OPENAI_API_KEY".to_string(), token.to_string()),
            ("OPENAI_BASE_URL".to_string(), base),
        ],
        "xai" => vec![
            ("XAI_API_KEY".to_string(), token.to_string()),
            ("XAI_BASE_URL".to_string(), base),
        ],
        "frontal" | "anthropic" => vec![
            ("FRONTAL_API_KEY".to_string(), token.to_string()),
            ("FRONTAL_BASE_URL".to_string(), base),
        ],
        "azure" => vec![
            ("AZURE_OPENAI_API_KEY".to_string(), token.to_string()),
            ("AZURE_OPENAI_BASE_URL".to_string(), base),
        ],
        "ollama" => vec![("OLLAMA_BASE_URL".to_string(), base)],
        _ => Vec::new(),
    }
}

/// Where the proxy sends a provider's requests, and which key and header it uses.
/// `lookup` reads the server's environment; it is a parameter so tests can supply values.
pub(crate) fn upstream_for(
    provider: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Upstream, (StatusCode, String)> {
    let configured = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());
    let (key_env, base_env, default_base, auth) = match provider {
        "openai" => (
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            "https://api.openai.com/v1",
            UpstreamAuth::Bearer,
        ),
        "xai" => (
            "XAI_API_KEY",
            "XAI_BASE_URL",
            "https://api.x.ai/v1",
            UpstreamAuth::Bearer,
        ),
        "frontal" => (
            "FRONTAL_API_KEY",
            "FRONTAL_BASE_URL",
            "https://ai.frontal.dev/v1",
            UpstreamAuth::Bearer,
        ),
        // The CLI reads FRONTAL_BASE_URL for both `frontal` and `anthropic`, so the proxy
        // does the same. Each task only receives the variable for its own provider.
        "anthropic" => (
            "FRONTAL_API_KEY",
            "FRONTAL_BASE_URL",
            "https://api.anthropic.com",
            UpstreamAuth::AnthropicKey,
        ),
        "azure" => (
            "AZURE_OPENAI_API_KEY",
            "AZURE_OPENAI_BASE_URL",
            "",
            UpstreamAuth::ApiKeyHeader,
        ),
        "ollama" => (
            "",
            "OLLAMA_BASE_URL",
            "http://localhost:11434",
            UpstreamAuth::None,
        ),
        "bedrock" => return Err((StatusCode::BAD_REQUEST, bedrock_unsupported())),
        _ => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("unknown model provider `{provider}`"),
            ))
        }
    };

    let key = if key_env.is_empty() {
        None
    } else {
        Some(configured(key_env).ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("the server has no {key_env} configured for provider `{provider}`"),
            )
        })?)
    };
    let base_url = configured(base_env).unwrap_or_else(|| default_base.to_string());
    if base_url.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            format!("the server has no {base_env} configured for provider `{provider}`"),
        ));
    }
    Ok(Upstream {
        base_url,
        key,
        auth,
    })
}

/// Forwards one model request. The caller must present a live worker token.
pub(crate) async fn proxy_model_request(
    access: &WorkerAccess,
    client: &reqwest::Client,
    provider: &str,
    rest: &str,
    request: Request,
    server_env: impl Fn(&str) -> Option<String>,
) -> Response {
    let Some(token) = token_from_headers(request.headers()) else {
        return error_response(StatusCode::UNAUTHORIZED, "missing worker token");
    };
    if access.task_for_token(&token).await.is_none() {
        return error_response(StatusCode::UNAUTHORIZED, "invalid or revoked worker token");
    }

    let upstream = match upstream_for(provider, server_env) {
        Ok(upstream) => upstream,
        Err((status, message)) => return error_response(status, &message),
    };

    let query = request
        .uri()
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let url = format!(
        "{}/{}{}",
        upstream.base_url.trim_end_matches('/'),
        rest.trim_start_matches('/'),
        query
    );

    let mut headers = reqwest::header::HeaderMap::new();
    for name in FORWARDED_REQUEST_HEADERS {
        if let Some(value) = request.headers().get(*name) {
            if let Ok(name) = HeaderName::from_bytes(name.as_bytes()) {
                headers.insert(name, value.clone());
            }
        }
    }
    if let Some(key) = upstream.key.as_deref() {
        let injected = match upstream.auth {
            UpstreamAuth::Bearer => HeaderValue::from_str(&format!("Bearer {key}"))
                .map(|value| (header::AUTHORIZATION, value)),
            UpstreamAuth::AnthropicKey => HeaderValue::from_str(key)
                .map(|value| (HeaderName::from_static("x-api-key"), value)),
            UpstreamAuth::ApiKeyHeader => {
                HeaderValue::from_str(key).map(|value| (HeaderName::from_static("api-key"), value))
            }
            UpstreamAuth::None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "invalid upstream configuration",
                )
            }
        };
        match injected {
            Ok((name, value)) => {
                headers.insert(name, value);
            }
            Err(_) => {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, "invalid upstream key")
            }
        }
    }

    let method = request.method().clone();
    let Ok(body) = axum::body::to_bytes(request.into_body(), MODEL_PROXY_MAX_BODY_BYTES).await
    else {
        return error_response(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");
    };

    let upstream_response = match client
        .request(method, &url)
        .headers(headers)
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            // Do not include the URL or headers: they can carry configuration details.
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("model provider request failed: {}", error.without_url()),
            );
        }
    };

    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned();
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    builder
        .body(Body::from_stream(upstream_response.bytes_stream()))
        .unwrap_or_else(|_| {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "response build failed")
        })
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "error": message }).to_string();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("static error response is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_token_reaches_only_its_own_task_endpoints() {
        assert_eq!(
            worker_endpoint_task_id(&Method::GET, "/v1/tasks/t1").as_deref(),
            Some("t1")
        );
        assert_eq!(
            worker_endpoint_task_id(&Method::GET, "/v1/tasks/t1/runtime").as_deref(),
            Some("t1")
        );
        assert_eq!(
            worker_endpoint_task_id(&Method::POST, "/v1/tasks/t1/complete").as_deref(),
            Some("t1")
        );
        assert_eq!(
            worker_endpoint_task_id(&Method::POST, "/v1/tasks/t1/integration/github").as_deref(),
            Some("t1")
        );

        // Everything else is outside the worker's reach.
        assert!(worker_endpoint_task_id(&Method::POST, "/v1/tasks").is_none());
        assert!(worker_endpoint_task_id(&Method::POST, "/v1/tasks/t1/cancel").is_none());
        assert!(worker_endpoint_task_id(&Method::POST, "/v1/tasks/t1/approval").is_none());
        assert!(worker_endpoint_task_id(&Method::GET, "/v1/tasks/t1/complete").is_none());
        assert!(worker_endpoint_task_id(&Method::GET, "/v1/events/ws").is_none());
        assert!(worker_endpoint_task_id(&Method::GET, "/v1/tasks/").is_none());
    }

    #[test]
    fn provider_resolution_matches_the_cli_default() {
        assert_eq!(worker_provider_name(Some("OpenAI"), None), Ok("openai"));
        assert_eq!(worker_provider_name(None, None), Ok("frontal"));
        assert_eq!(
            worker_provider_name(None, Some("gpt-5-codex")),
            Ok("openai")
        );
        assert!(worker_provider_name(Some("bedrock"), None).is_err());
        assert!(worker_provider_name(Some("mystery"), None).is_err());
    }

    #[test]
    fn worker_env_contains_only_the_proxy_and_the_token_for_one_provider() {
        let env = worker_provider_env("openai", "http://host.docker.internal:8790/", "tok");
        assert_eq!(
            env,
            vec![
                ("OPENAI_API_KEY".to_string(), "tok".to_string()),
                (
                    "OPENAI_BASE_URL".to_string(),
                    "http://host.docker.internal:8790/v1/model-proxy/openai".to_string()
                ),
            ]
        );
        // No other provider's variables are present.
        assert!(env
            .iter()
            .all(|(key, _)| !key.starts_with("XAI") && !key.starts_with("FRONTAL")));
    }

    #[test]
    fn upstream_uses_server_key_and_refuses_missing_or_bedrock() {
        let keys = |name: &str| match name {
            "OPENAI_API_KEY" => Some("sk-real".to_string()),
            "OPENAI_BASE_URL" => Some("https://openrouter.ai/api/v1".to_string()),
            _ => None,
        };
        let upstream = upstream_for("openai", keys).expect("openai upstream");
        assert_eq!(upstream.key.as_deref(), Some("sk-real"));
        assert_eq!(upstream.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(upstream.auth, UpstreamAuth::Bearer);

        let missing = upstream_for("xai", |_| None).expect_err("no xai key");
        assert_eq!(missing.0, StatusCode::SERVICE_UNAVAILABLE);
        assert!(upstream_for("bedrock", |_| None).is_err());
        assert!(upstream_for("unknown", |_| None).is_err());
    }

    #[test]
    fn ollama_upstream_needs_no_key() {
        let upstream = upstream_for("ollama", |_| None).expect("ollama");
        assert_eq!(upstream.key, None);
        assert_eq!(upstream.auth, UpstreamAuth::None);
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A one-shot upstream. It records the raw request head it received and replies with
    /// a fixed JSON body.
    fn upstream_once() -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
        let address = listener.local_addr().expect("addr");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut head_bytes = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                head_bytes.extend_from_slice(&buffer[..read]);
                if head_bytes.windows(4).any(|window| window == b"\r\n\r\n") || read == 0 {
                    break;
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&head_bytes).into_owned());
            let body = r#"{"echo":"ok"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });
        (format!("http://{address}/api"), receiver)
    }

    fn worker_request(token: Option<&str>) -> Request {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/model-proxy/openai/chat/completions?stream=false")
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder
            .body(axum::body::Body::from(r#"{"model":"x"}"#))
            .expect("request")
    }

    #[tokio::test]
    async fn proxy_injects_the_server_key_and_strips_the_worker_token() {
        let Some(store) = crate::worker_tokens::tests::test_store() else {
            return;
        };
        let access = WorkerAccess::new(Some(store), 3600);
        let token = access
            .issue(&crate::worker_tokens::tests::unique_task("proxy"))
            .expect("token");
        let (base, received) = upstream_once();
        let client = reqwest::Client::new();

        let response = proxy_model_request(
            &access,
            &client,
            "openai",
            "chat/completions",
            worker_request(Some(&token)),
            |name| match name {
                "OPENAI_API_KEY" => Some("sk-server-only".to_string()),
                "OPENAI_BASE_URL" => Some(base.clone()),
                _ => None,
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let head = received.recv().expect("upstream saw a request");
        assert!(
            head.starts_with("POST /api/chat/completions?stream=false "),
            "{head}"
        );
        assert!(
            head.to_ascii_lowercase()
                .contains("authorization: bearer sk-server-only"),
            "{head}"
        );
        assert!(!head.contains(&token), "worker token leaked upstream");
    }

    #[tokio::test]
    async fn proxy_refuses_missing_or_revoked_tokens_without_contacting_the_provider() {
        let Some(store) = crate::worker_tokens::tests::test_store() else {
            return;
        };
        let access = WorkerAccess::new(Some(store), 3600);
        let task = crate::worker_tokens::tests::unique_task("revoked");
        let client = reqwest::Client::new();
        let server_env =
            |name: &str| (name == "OPENAI_API_KEY").then(|| "sk-server-only".to_string());

        let missing = proxy_model_request(
            &access,
            &client,
            "openai",
            "chat/completions",
            worker_request(None),
            server_env,
        )
        .await;
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

        let token = access.issue(&task).expect("token");
        access.revoke(&task);
        let revoked = proxy_model_request(
            &access,
            &client,
            "openai",
            "chat/completions",
            worker_request(Some(&token)),
            server_env,
        )
        .await;
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn proxy_reports_a_missing_server_key_as_unavailable() {
        let Some(store) = crate::worker_tokens::tests::test_store() else {
            return;
        };
        let access = WorkerAccess::new(Some(store), 3600);
        let token = access
            .issue(&crate::worker_tokens::tests::unique_task("nokey"))
            .expect("token");
        let response = proxy_model_request(
            &access,
            &reqwest::Client::new(),
            "xai",
            "chat/completions",
            worker_request(Some(&token)),
            |_| None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
