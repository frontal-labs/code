//! Typed client for the `Frontal Code` integrations HTTP API.
//!
//! The client talks to the `Frontal Code` server (the one the CLI wraps) and lets
//! SDK consumers discover registered integrations and invoke their capabilities
//! with full type safety. All types are self-contained; they mirror the server's
//! wire shape and do not depend on the server crate.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Summary of a registered integration (as returned by `GET /v1/integrations`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IntegrationSummary {
  pub id: String,
  pub version: String,
  pub display_name: String,
  pub description: String,
  pub capability_count: usize,
  pub auth_required: bool,
  pub event_count: usize,
}

/// Summary of a capability (as returned by `GET /v1/integrations/{id}/capabilities`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CapabilitySummary {
  pub name: String,
  pub description: String,
  pub permission_mode: String,
  pub idempotent: bool,
}

/// A webhook event summary (as returned by `GET /v1/integrations/{id}/events`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EventSummary {
  pub name: String,
  pub description: String,
  pub signature_required: bool,
}

/// Authentication requirements declared by an integration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthSpec {
  /// The auth kind: `"oauth"`, `"api_key"`, or `"none"`.
  pub r#type: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub client_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub token_url: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub authorize_url: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub scopes: Option<Vec<String>>,
}

/// The full integration manifest as returned by `GET /v1/integrations/{id}`.
///
/// Fields beyond the well-known ones are preserved in [`Self::extra`] so the
/// client never fails on a newer server shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IntegrationManifest {
  pub id: String,
  pub version: String,
  pub display_name: String,
  pub description: String,
  pub transport: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub transport_config: Option<Value>,
  #[serde(default)]
  pub capabilities: Vec<CapabilitySummary>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub auth: Option<AuthSpec>,
  #[serde(default)]
  pub events: Vec<EventSummary>,
  #[serde(flatten)]
  pub extra: std::collections::BTreeMap<String, Value>,
}

/// Errors surfaced by the [`IntegrationClient`].
#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
  #[error("http request failed: {0}")]
  Http(#[from] reqwest::Error),
  #[error("server returned status {status}: {body}")]
  Server { status: u16, body: String },
  #[error("failed to parse response: {0}")]
  Parse(String),
}

impl IntegrationError {
  /// The HTTP status code, when the error came from a non-2xx server response.
  pub fn status(&self) -> Option<u16> {
    match self {
      Self::Server { status, .. } => Some(*status),
      _ => None,
    }
  }

  /// The raw response body, when the error came from a non-2xx server response.
  pub fn body(&self) -> Option<&str> {
    match self {
      Self::Server { body, .. } => Some(body),
      _ => None,
    }
  }
}

/// Typed client for the integrations API.
#[derive(Clone)]
pub struct IntegrationClient {
  base_url: String,
  http: reqwest::Client,
}

impl std::fmt::Debug for IntegrationClient {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("IntegrationClient")
      .field("base_url", &self.base_url)
      .finish_non_exhaustive()
  }
}

impl IntegrationClient {
  /// Create a client pointing at the given server URL.
  ///
  /// A trailing slash is normalized away.
  pub fn new(base_url: &str) -> Self {
    Self {
      base_url: base_url.trim_end_matches('/').to_string(),
      http: reqwest::Client::new(),
    }
  }

  /// Create a client with a caller-supplied HTTP client (useful for tests and
  /// custom TLS configuration). The base URL is normalized the same way as in
  /// [`Self::new`].
  pub fn with_client(base_url: &str, http: reqwest::Client) -> Self {
    Self {
      base_url: base_url.trim_end_matches('/').to_string(),
      http,
    }
  }

  /// The server URL this client is bound to, without a trailing slash.
  pub fn base_url(&self) -> &str {
    &self.base_url
  }

  /// List all registered integrations.
  pub async fn list(&self) -> Result<Vec<IntegrationSummary>, IntegrationError> {
    let response = self.http.get(self.url("/v1/integrations")).send().await?;
    let value = parse_json(response).await?;
    serde_json::from_value(value).map_err(|e| IntegrationError::Parse(e.to_string()))
  }

  /// Get the full manifest for one integration.
  pub async fn get(&self, id: &str) -> Result<IntegrationManifest, IntegrationError> {
    let response = self
      .http
      .get(self.url(&format!("/v1/integrations/{id}")))
      .send()
      .await?;
    let value = parse_json(response).await?;
    serde_json::from_value(value).map_err(|e| IntegrationError::Parse(e.to_string()))
  }

  /// List capabilities for an integration.
  pub async fn capabilities(
    &self,
    integration_id: &str,
  ) -> Result<Vec<CapabilitySummary>, IntegrationError> {
    let response = self
      .http
      .get(self.url(&format!("/v1/integrations/{integration_id}/capabilities")))
      .send()
      .await?;
    let value = parse_json(response).await?;
    serde_json::from_value(value).map_err(|e| IntegrationError::Parse(e.to_string()))
  }

  /// Invoke a capability with the given input.
  pub async fn invoke(
    &self,
    integration_id: &str,
    capability: &str,
    input: &Value,
  ) -> Result<Value, IntegrationError> {
    let response = self
      .http
      .post(self.url(&format!(
        "/v1/integrations/{integration_id}/capabilities/{capability}/invoke"
      )))
      .json(input)
      .send()
      .await?;
    let status = response.status().as_u16();
    let body = response.text().await?;
    if !(200..300).contains(&status) {
      return Err(IntegrationError::Server { status, body });
    }
    serde_json::from_str(&body).map_err(|e| IntegrationError::Parse(e.to_string()))
  }

  /// List webhook events for an integration.
  pub async fn events(&self, integration_id: &str) -> Result<Vec<EventSummary>, IntegrationError> {
    let response = self
      .http
      .get(self.url(&format!("/v1/integrations/{integration_id}/events")))
      .send()
      .await?;
    let value = parse_json(response).await?;
    serde_json::from_value(value).map_err(|e| IntegrationError::Parse(e.to_string()))
  }

  fn url(&self, path: &str) -> String {
    format!("{}{path}", self.base_url)
  }
}

/// Send a response and, on success, parse the body as JSON. Non-2xx responses
/// become [`IntegrationError::Server`] with the raw body captured.
async fn parse_json(response: reqwest::Response) -> Result<Value, IntegrationError> {
  let status = response.status().as_u16();
  let body = response.text().await?;
  if !(200..300).contains(&status) {
    return Err(IntegrationError::Server { status, body });
  }
  serde_json::from_str(&body).map_err(|e| IntegrationError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;

  const SUMMARY_JSON: &str = r#"{
    "id": "github",
    "version": "1.2.0",
    "display_name": "GitHub",
    "description": "Issues, PRs, and repos",
    "capability_count": 4,
    "auth_required": true,
    "event_count": 2
  }"#;

  #[test]
  fn integration_summary_deserializes_from_sample_json() {
    let summary: IntegrationSummary = serde_json::from_str(SUMMARY_JSON).expect("parses");
    assert_eq!(summary.id, "github");
    assert_eq!(summary.version, "1.2.0");
    assert_eq!(summary.display_name, "GitHub");
    assert_eq!(summary.description, "Issues, PRs, and repos");
    assert_eq!(summary.capability_count, 4);
    assert!(summary.auth_required);
    assert_eq!(summary.event_count, 2);
  }

  #[test]
  fn capability_summary_deserializes_from_sample_json() {
    let json = r#"{
      "name": "create_issue",
      "description": "Open an issue",
      "permission_mode": "workspace_write",
      "idempotent": false
    }"#;
    let cap: CapabilitySummary = serde_json::from_str(json).expect("parses");
    assert_eq!(cap.name, "create_issue");
    assert_eq!(cap.permission_mode, "workspace_write");
    assert!(!cap.idempotent);
  }

  #[test]
  fn manifest_preserves_unknown_fields_in_extra() {
    let json = r#"{
      "id": "github",
      "version": "1.2.0",
      "display_name": "GitHub",
      "description": "Issues, PRs, and repos",
      "transport": "stdio",
      "capabilities": [],
      "events": [],
      "context_fields": [{"name": "owner"}]
    }"#;
    let manifest: IntegrationManifest = serde_json::from_str(json).expect("parses");
    assert_eq!(manifest.id, "github");
    assert_eq!(manifest.transport, "stdio");
    assert!(manifest.auth.is_none());
    assert_eq!(
      manifest
        .extra
        .get("context_fields")
        .and_then(Value::as_array)
        .map(Vec::len),
      Some(1)
    );
  }

  #[test]
  fn client_new_strips_trailing_slash_and_exposes_base_url() {
    let client = IntegrationClient::new("http://localhost:8790/");
    assert_eq!(client.base_url(), "http://localhost:8790");
  }

  #[test]
  fn client_new_keeps_base_url_without_trailing_slash() {
    let client = IntegrationClient::new("http://localhost:8790");
    assert_eq!(client.base_url(), "http://localhost:8790");
  }

  #[test]
  fn server_error_exposes_status_and_body() {
    let error = IntegrationError::Server {
      status: 404,
      body: "not found".to_string(),
    };
    assert_eq!(error.status(), Some(404));
    assert_eq!(error.body(), Some("not found"));
    assert_eq!(error.to_string(), "server returned status 404: not found");
  }

  #[test]
  fn parse_error_exposes_detail() {
    let error = IntegrationError::Parse("bad json".to_string());
    assert_eq!(error.status(), None);
    assert_eq!(error.to_string(), "failed to parse response: bad json");
  }
}
