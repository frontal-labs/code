//! `/v1/integrations` control-plane routes.
//!
//! Serves the typed integration manifests from [`code_integrations`]: listing
//! integrations, inspecting a single manifest, its capabilities, and its
//! events. Capability invocation is a stub for now; real MCP dispatch lands
//! in Phase 3.

use std::sync::Mutex;

use axum::extract::Path;
use axum::extract::State;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use code_integrations::mcp::tools::PermissionMode;
use code_integrations::{AuthSpec, IntegrationManifest, ManifestRegistry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::AppError;
use crate::ServerState;

/// Environment variable holding a JSON array of [`IntegrationManifest`] values.
pub const INTEGRATIONS_CONFIG_ENV: &str = "FRONTAL_CODE_INTEGRATIONS_CONFIG";

/// Server state for the integrations routes. The registry is validated on
/// load; manifests that fail validation are skipped with a warning.
#[derive(Debug, Default)]
pub struct IntegrationsState {
    pub registry: Mutex<ManifestRegistry>,
}

impl IntegrationsState {
    /// Create an empty state with no registered integrations.
    #[cfg(test)]
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: Mutex::new(ManifestRegistry::new()),
        }
    }

    /// Create a state seeded with the built-in provider manifests (GitHub,
    /// Linear, `PostHog`, and Sentry). These are the first-party defaults;
    /// project-level configuration can still override any of them by id.
    #[cfg(test)]
    #[must_use]
    pub fn with_builtins() -> Self {
        Self {
            registry: Mutex::new(
                code_integrations::builtin_providers()
                    .expect("built-in provider manifests are valid"),
            ),
        }
    }

    /// Load integrations from the `FRONTAL_CODE_INTEGRATIONS_CONFIG`
    /// environment variable, a JSON array of manifests, on top of the
    /// built-in providers. The env var can add new integrations or override a
    /// built-in by id. Missing or invalid entries are skipped with a warning
    /// so one bad manifest cannot take the whole registry down.
    #[must_use]
    pub fn load_from_env() -> Self {
        Self::from_json(
            &std::env::var(INTEGRATIONS_CONFIG_ENV).unwrap_or_default(),
            code_integrations::builtin_providers().expect("built-in provider manifests are valid"),
        )
    }

    /// Build a state from a raw JSON array of manifests, layered on top of a
    /// base registry (typically the built-in providers). An empty, invalid,
    /// or malformed string yields the base registry with a warning;
    /// individual manifests that fail validation are skipped with a warning
    /// so one bad manifest cannot take the whole registry down. A manifest
    /// whose id collides with a base entry replaces it, letting project
    /// configuration override a built-in.
    #[must_use]
    pub fn from_json(raw: &str, base: ManifestRegistry) -> Self {
        let mut registry = base;
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let Ok(manifests) = serde_json::from_str::<Vec<IntegrationManifest>>(trimmed) else {
                eprintln!("warning: {INTEGRATIONS_CONFIG_ENV} is not a JSON array of manifests; ignoring it");
                return Self {
                    registry: Mutex::new(registry),
                };
            };
            for manifest in manifests {
                let id = manifest.id.clone();
                if registry.get(&id).is_some() {
                    // A same-id manifest from configuration overrides the
                    // built-in: drop the existing entry, then register the new one.
                    registry.remove(&id);
                }
                if let Err(issues) = registry.register(manifest) {
                    let reason = issues
                        .iter()
                        .map(|issue| format!("{}: {}", issue.field, issue.message))
                        .collect::<Vec<_>>()
                        .join("; ");
                    eprintln!("warning: skipping integration manifest `{id}`: {reason}");
                }
            }
        }
        Self {
            registry: Mutex::new(registry),
        }
    }
}

/// One row of the integration list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrationSummary {
    pub id: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub capability_count: usize,
    pub auth_required: bool,
    pub event_count: usize,
}

impl From<&IntegrationManifest> for IntegrationSummary {
    fn from(manifest: &IntegrationManifest) -> Self {
        Self {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            display_name: manifest.display_name.clone(),
            description: manifest.description.clone(),
            capability_count: manifest.capabilities.len(),
            auth_required: manifest
                .auth
                .as_ref()
                .is_some_and(|auth| !matches!(auth, AuthSpec::None)),
            event_count: manifest.events.len(),
        }
    }
}

/// One capability of an integration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilitySummary {
    pub name: String,
    pub description: String,
    pub permission_mode: String,
    pub idempotent: bool,
}

impl From<&code_integrations::CapabilitySpec> for CapabilitySummary {
    fn from(spec: &code_integrations::CapabilitySpec) -> Self {
        Self {
            name: spec.name.clone(),
            description: spec.description.clone(),
            permission_mode: permission_mode_label(spec.permission_mode),
            idempotent: spec.idempotent,
        }
    }
}

/// One webhook event an integration can emit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventSummary {
    pub name: String,
    pub description: String,
    pub signature_required: bool,
}

impl From<&code_integrations::EventSpec> for EventSummary {
    fn from(event: &code_integrations::EventSpec) -> Self {
        Self {
            name: event.name.clone(),
            description: event.description.clone(),
            signature_required: event.signature.is_some(),
        }
    }
}

/// Input for invoking a capability. The JSON value is the MCP tool input.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InvokeRequest {
    pub input: Value,
}

fn permission_mode_label(mode: PermissionMode) -> String {
    match mode {
        PermissionMode::ReadOnly => "read_only",
        PermissionMode::WorkspaceWrite => "workspace_write",
        PermissionMode::DangerFullAccess => "danger_full_access",
    }
    .to_string()
}

pub(crate) async fn list_integrations(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<IntegrationSummary>>, AppError> {
    let registry = state
        .integrations
        .registry
        .lock()
        .map_err(|_| AppError::internal("integrations registry lock poisoned"))?;
    Ok(Json(
        registry
            .list()
            .into_iter()
            .map(IntegrationSummary::from)
            .collect(),
    ))
}

pub(crate) async fn get_integration(
    Path(id): Path<String>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<IntegrationManifest>, AppError> {
    let registry = state
        .integrations
        .registry
        .lock()
        .map_err(|_| AppError::internal("integrations registry lock poisoned"))?;
    let manifest = registry
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("integration not found: {id}")))?;
    Ok(Json(manifest.clone()))
}

pub(crate) async fn list_capabilities(
    Path(id): Path<String>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<CapabilitySummary>>, AppError> {
    let registry = state
        .integrations
        .registry
        .lock()
        .map_err(|_| AppError::internal("integrations registry lock poisoned"))?;
    let manifest = registry
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("integration not found: {id}")))?;
    Ok(Json(
        manifest
            .capabilities
            .iter()
            .map(CapabilitySummary::from)
            .collect(),
    ))
}

/// Invoke a capability. Stub: MCP dispatch is Phase 3.
pub(crate) async fn invoke_capability(
    Path((id, name)): Path<(String, String)>,
    State(state): State<Arc<ServerState>>,
    Json(request): Json<InvokeRequest>,
) -> Result<(), AppError> {
    let registry = state
        .integrations
        .registry
        .lock()
        .map_err(|_| AppError::internal("integrations registry lock poisoned"))?;
    let Some(_capability) = registry.capability(&id, &name) else {
        return Err(AppError::not_found(format!(
            "capability '{name}' not found on integration '{id}'"
        )));
    };
    let input = request.input;
    drop(registry);
    Err(AppError::not_implemented(format!(
        "invoking capability '{name}' on integration '{id}' is not implemented yet; \
         MCP dispatch lands in Phase 3 (input: {input})"
    )))
}

pub(crate) async fn list_events(
    Path(id): Path<String>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<EventSummary>>, AppError> {
    let registry = state
        .integrations
        .registry
        .lock()
        .map_err(|_| AppError::internal("integrations registry lock poisoned"))?;
    let manifest = registry
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("integration not found: {id}")))?;
    Ok(Json(
        manifest.events.iter().map(EventSummary::from).collect(),
    ))
}

/// Build a router over the integrations routes, for callers that do not want
/// the full control-plane app.
pub(crate) fn integrations_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/v1/integrations", get(list_integrations))
        .route("/v1/integrations/{id}", get(get_integration))
        .route("/v1/integrations/{id}/capabilities", get(list_capabilities))
        .route(
            "/v1/integrations/{id}/capabilities/{name}/invoke",
            post(invoke_capability),
        )
        .route("/v1/integrations/{id}/events", get(list_events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_integrations::mcp::config::McpTransport;
    use code_integrations::{ContextFieldSpec, EventSpec, SignatureSpec, TransportConfig};
    use serde_json::json;

    fn sample_manifest() -> IntegrationManifest {
        IntegrationManifest {
            id: "github".to_string(),
            version: "1.0.0".to_string(),
            display_name: "GitHub".to_string(),
            description: "GitHub integration".to_string(),
            transport: McpTransport::Stdio,
            transport_config: TransportConfig::Stdio {
                command: "gh-mcp".to_string(),
                args: vec!["serve".to_string()],
                env: std::collections::BTreeMap::new(),
            },
            capabilities: vec![code_integrations::CapabilitySpec {
                name: "create_issue".to_string(),
                description: "Create an issue".to_string(),
                input_schema: json!({"type": "object"}),
                output_schema: Some(json!({"type": "object"})),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: false,
            }],
            auth: Some(AuthSpec::ApiKey {
                env_var: "GITHUB_TOKEN".to_string(),
                header: None,
            }),
            events: vec![EventSpec {
                name: "issue.opened".to_string(),
                description: "An issue was opened".to_string(),
                payload_schema: json!({"type": "object"}),
                signature: Some(SignatureSpec::HmacSha256 {
                    secret_env: "GITHUB_WEBHOOK_SECRET".to_string(),
                }),
            }],
            context_fields: vec![ContextFieldSpec {
                name: "github.issue".to_string(),
                description: "The linked GitHub issue".to_string(),
                schema: json!({"type": "object"}),
            }],
        }
    }

    #[test]
    fn new_creates_an_empty_registry() {
        let state = IntegrationsState::new();
        let registry = state.registry.lock().expect("registry is not poisoned");
        assert_eq!(registry.list().len(), 0);
    }

    #[test]
    fn from_json_reads_the_sample_json_over_builtins() {
        let sample = sample_manifest();
        let raw = serde_json::to_string(&vec![sample]).expect("serialize sample");
        let base = code_integrations::builtin_providers().expect("built-ins validate");
        let state = IntegrationsState::from_json(&raw, base);
        let registry = state.registry.lock().expect("registry is not poisoned");
        // The sample manifest overrides the built-in github (same id), and the
        // other built-ins remain.
        assert!(registry.get("github").is_some());
        assert_eq!(registry.list().len(), 4);
    }

    #[test]
    fn from_json_over_builtins_allows_a_custom_id_to_be_added() {
        let custom = sample_manifest();
        let custom_raw = serde_json::to_string(&vec![custom]).expect("serialize");
        let base = code_integrations::builtin_providers().expect("built-ins validate");
        let state = IntegrationsState::from_json(&custom_raw, base);
        let registry = state.registry.lock().expect("registry is not poisoned");
        assert_eq!(registry.list().len(), 4);
        assert!(registry.get("sentry").is_some());
    }

    #[test]
    fn from_json_with_empty_raw_keeps_only_builtins() {
        let base = code_integrations::builtin_providers().expect("built-ins validate");
        let state = IntegrationsState::from_json("", base);
        let registry = state.registry.lock().expect("registry is not poisoned");
        assert_eq!(registry.list().len(), 4);
        for id in code_integrations::BUILTIN_PROVIDER_IDS {
            assert!(registry.get(id).is_some(), "missing `{id}`");
        }
    }

    static INTEGRATIONS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Build a state seeded with the built-in providers, wrapped in an
    /// `Arc<ServerState>` for the integrations routes.
    fn state_with_builtins() -> Arc<ServerState> {
        Arc::new(crate::ServerState {
            integrations: std::sync::Arc::new(IntegrationsState::with_builtins()),
            ..crate::ServerState::default()
        })
    }

    /// Fire a single request at a stateless router and return the response
    /// status and body for assertions. The integrations router is built
    /// per-call with the given state so each request sees the same snapshot.
    async fn oneshot(
        state: Arc<ServerState>,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> (axum::http::StatusCode, Vec<u8>) {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let router = integrations_routes().with_state(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, bytes)
    }

    #[test]
    fn load_from_env_reads_the_sample_json() {
        let _guard = INTEGRATIONS_ENV_LOCK.lock().expect("env lock poisoned");
        let sample = sample_manifest();
        let raw = serde_json::to_string(&vec![sample]).expect("serialize sample");
        std::env::set_var(INTEGRATIONS_CONFIG_ENV, raw);
        let state = IntegrationsState::load_from_env();
        std::env::remove_var(INTEGRATIONS_CONFIG_ENV);
        let registry = state.registry.lock().expect("registry is not poisoned");
        // The sample manifest overrides the built-in github (same id), and the
        // other built-ins remain.
        assert!(registry.get("github").is_some());
        assert_eq!(registry.list().len(), 4);
    }

    #[test]
    fn load_from_env_skips_invalid_manifests() {
        let _guard = INTEGRATIONS_ENV_LOCK.lock().expect("env lock poisoned");
        let raw = r#"[
            {"id": "Bad Id!", "version": "1.0.0", "display_name": "x", "description": "y",
             "transport": "stdio", "transport_config": {"kind": "stdio", "command": "c", "args": [], "env": {}},
             "capabilities": [], "auth": null, "events": [], "context_fields": []},
            {"id": "good-one", "version": "2.0.0", "display_name": "Good", "description": "z",
             "transport": "http", "transport_config": {"kind": "http", "url": "https://example.com", "headers": {}},
             "capabilities": [{"name": "ping", "description": "p", "input_schema": {"type": "object"},
                               "permission_mode": "read_only", "idempotent": true}],
             "auth": null, "events": [], "context_fields": []}
        ]"#;
        let base = code_integrations::builtin_providers().expect("built-ins validate");
        let state = IntegrationsState::from_json(raw, base);
        let registry = state.registry.lock().expect("registry is not poisoned");
        // The bad manifest is skipped; the good one is added; built-ins remain.
        assert!(registry.get("Bad Id!").is_none());
        assert!(registry.get("good-one").is_some());
        assert_eq!(registry.list().len(), 5);
    }

    #[test]
    fn with_builtins_seeds_all_four_providers() {
        let state = IntegrationsState::with_builtins();
        let registry = state.registry.lock().expect("registry is not poisoned");
        assert_eq!(registry.list().len(), 4);
        for id in code_integrations::BUILTIN_PROVIDER_IDS {
            assert!(registry.get(id).is_some(), "missing `{id}`");
        }
    }

    #[test]
    fn summary_dto_serializes_with_expected_fields() {
        let summary = IntegrationSummary::from(&sample_manifest());
        let value = serde_json::to_value(&summary).expect("serialize summary");
        assert_eq!(value["id"], "github");
        assert_eq!(value["version"], "1.0.0");
        assert_eq!(value["display_name"], "GitHub");
        assert_eq!(value["capability_count"], 1);
        assert_eq!(value["auth_required"], true);
        assert_eq!(value["event_count"], 1);
    }

    #[test]
    fn capability_dto_uses_snake_case_permission_labels() {
        let spec = &sample_manifest().capabilities[0];
        let summary = CapabilitySummary::from(spec);
        let value = serde_json::to_value(&summary).expect("serialize capability");
        assert_eq!(value["name"], "create_issue");
        assert_eq!(value["permission_mode"], "workspace_write");
        assert_eq!(value["idempotent"], false);

        let mut read_only = spec.clone();
        read_only.permission_mode = PermissionMode::ReadOnly;
        assert_eq!(
            CapabilitySummary::from(&read_only).permission_mode,
            "read_only"
        );
        let mut danger = spec.clone();
        danger.permission_mode = PermissionMode::DangerFullAccess;
        assert_eq!(
            CapabilitySummary::from(&danger).permission_mode,
            "danger_full_access"
        );
    }

    #[test]
    fn event_dto_reports_signature_requirements() {
        let event = &sample_manifest().events[0];
        let summary = EventSummary::from(event);
        assert_eq!(summary.name, "issue.opened");
        assert!(summary.signature_required);

        let mut unsigned = event.clone();
        unsigned.signature = None;
        assert!(!EventSummary::from(&unsigned).signature_required);
    }

    #[test]
    fn invoke_request_deserializes_an_input_value() {
        let request: InvokeRequest =
            serde_json::from_value(json!({"input": {"title": "bug", "body": "broken"}}))
                .expect("deserialize invoke request");
        assert_eq!(request.input["title"], "bug");
    }

    #[tokio::test]
    async fn routes_serve_the_manifest_surface() {
        use axum::http::StatusCode;

        // The state is seeded with the built-in providers, so the list is
        // non-empty and a built-in id (github) is present.
        let state = state_with_builtins();
        let (status, bytes) = oneshot(state.clone(), "GET", "/v1/integrations", json!(null)).await;
        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.as_array().expect("array").len(), 4);

        let (status, _) =
            oneshot(state.clone(), "GET", "/v1/integrations/github", json!(null)).await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = oneshot(
            state.clone(),
            "GET",
            "/v1/integrations/github/capabilities",
            json!(null),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = oneshot(
            state.clone(),
            "GET",
            "/v1/integrations/github/events",
            json!(null),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // An unknown integration id still 404s.
        let (status, _) = oneshot(
            state.clone(),
            "GET",
            "/v1/integrations/does-not-exist",
            json!(null),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // ...and a known capability returns the 501 stub.
        let (status, _) = oneshot(
            state.clone(),
            "POST",
            "/v1/integrations/github/capabilities/create_pr/invoke",
            json!({"input": {"title": "bug"}}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);

        // A capability that does not exist on the integration 404s.
        let (status, _) = oneshot(
            state.clone(),
            "POST",
            "/v1/integrations/github/capabilities/missing/invoke",
            json!({"input": {}}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
