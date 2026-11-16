//! Built-in provider manifests.
//!
//! These are the four first-party integrations (GitHub, Linear, `PostHog`,
//! and Sentry) expressed as config-driven [`IntegrationManifest`] values instead
//! of hardcoded provider code. They are the defaults the server and CLI load
//! when no project-level overrides are configured.
//!
//! The legacy `IntegrationRegistry::call_*` methods (see
//! [`crate::mcp::integration`]) remain as deprecated adapters over the
//! corresponding capabilities below for one release.

use std::collections::BTreeMap;

use serde_json::json;

use crate::manifest::{
    AuthSpec, CapabilitySpec, IntegrationManifest, ManifestRegistry, TransportConfig,
};
use crate::mcp::config::McpTransport;
use crate::mcp::tools::PermissionMode;

/// The four built-in provider integration IDs, in stable (sorted) order.
pub const BUILTIN_PROVIDER_IDS: [&str; 4] = ["github", "linear", "posthog", "sentry"];

/// Returns the four built-in provider manifests, ready to register.
///
/// Each manifest is validated before being returned; a built-in manifest that
/// fails validation is a bug, so this returns a `Result` that callers (and
/// tests) can assert on.
pub fn builtin_providers() -> Result<ManifestRegistry, Vec<String>> {
    let mut registry = ManifestRegistry::new();
    let mut failures = Vec::new();
    for manifest in [
        github_manifest(),
        linear_manifest(),
        posthog_manifest(),
        sentry_manifest(),
    ] {
        if let Err(issues) = registry.register(manifest) {
            failures.extend(issues.into_iter().map(|issue| issue.message));
        }
    }
    if failures.is_empty() {
        Ok(registry)
    } else {
        Err(failures)
    }
}

/// The GitHub integration: PR creation, issue comments, and check runs against
/// the GitHub REST/MCP surface.
fn github_manifest() -> IntegrationManifest {
    IntegrationManifest {
        id: "github".into(),
        version: "1.0.0".into(),
        display_name: "GitHub".into(),
        description: "Create pull requests, comment on issues, and report check runs.".into(),
        transport: McpTransport::Http,
        transport_config: TransportConfig::Http {
            url: "https://api.github.com".into(),
            headers: header_map(&[("Authorization", "${GITHUB_TOKEN}")]),
        },
        capabilities: vec![
            CapabilitySpec {
                name: "create_pr".into(),
                description: "Open a pull request on a repository.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string" },
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "head": { "type": "string" },
                        "base": { "type": "string" }
                    },
                    "required": ["repo", "title", "head", "base"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": { "number": { "type": "integer" }, "url": { "type": "string" } }
                })),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: false,
            },
            CapabilitySpec {
                name: "create_issue_comment".into(),
                description: "Add a comment to an issue or pull request.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string" },
                        "issue_number": { "type": "integer" },
                        "body": { "type": "string" }
                    },
                    "required": ["repo", "issue_number", "body"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": { "id": { "type": "integer" }, "url": { "type": "string" } }
                })),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: false,
            },
            CapabilitySpec {
                name: "create_check_run".into(),
                description: "Create a check run for a commit.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string" },
                        "name": { "type": "string" },
                        "head_sha": { "type": "string" },
                        "status": { "type": "string" },
                        "conclusion": { "type": "string" }
                    },
                    "required": ["repo", "name", "head_sha"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": { "id": { "type": "integer" }, "url": { "type": "string" } }
                })),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: false,
            },
        ],
        auth: Some(AuthSpec::ApiKey {
            env_var: "GITHUB_TOKEN".into(),
            header: Some("Authorization".into()),
        }),
        events: Vec::new(),
        context_fields: Vec::new(),
    }
}

/// The Linear integration: issue comments used by the tracker reporter.
fn linear_manifest() -> IntegrationManifest {
    IntegrationManifest {
        id: "linear".into(),
        version: "1.0.0".into(),
        display_name: "Linear".into(),
        description: "Comment on Linear issues for tracker reporting.".into(),
        transport: McpTransport::Http,
        transport_config: TransportConfig::Http {
            url: "https://api.linear.app/graphql".into(),
            headers: header_map(&[("Authorization", "${LINEAR_API_KEY}")]),
        },
        capabilities: vec![CapabilitySpec {
            name: "create_issue_comment".into(),
            description: "Add a comment to a Linear issue.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "issue_id": { "type": "string" },
                    "body": { "type": "string" }
                },
                "required": ["issue_id", "body"]
            }),
            output_schema: Some(json!({
                "type": "object",
                "properties": { "id": { "type": "string" }, "url": { "type": "string" } }
            })),
            permission_mode: PermissionMode::WorkspaceWrite,
            idempotent: false,
        }],
        auth: Some(AuthSpec::ApiKey {
            env_var: "LINEAR_API_KEY".into(),
            header: Some("Authorization".into()),
        }),
        events: Vec::new(),
        context_fields: Vec::new(),
    }
}

/// The `PostHog` integration: capture product and telemetry events.
fn posthog_manifest() -> IntegrationManifest {
    IntegrationManifest {
        id: "posthog".into(),
        version: "1.0.0".into(),
        display_name: "PostHog".into(),
        description: "Capture analytics events in PostHog.".into(),
        transport: McpTransport::Http,
        transport_config: TransportConfig::Http {
            url: "https://us.i.posthog.com/capture/".into(),
            headers: BTreeMap::new(),
        },
        capabilities: vec![
            CapabilitySpec {
                name: "capture_event".into(),
                description: "Capture a single analytics event.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "event": { "type": "string" },
                        "distinct_id": { "type": "string" },
                        "properties": { "type": "object" }
                    },
                    "required": ["event", "distinct_id"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": { "status": { "type": "string" } }
                })),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: true,
            },
            CapabilitySpec {
                name: "capture_events_batch".into(),
                description: "Capture a batch of analytics events.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "batch": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "event": { "type": "string" },
                                    "distinct_id": { "type": "string" },
                                    "properties": { "type": "object" }
                                },
                                "required": ["event", "distinct_id"]
                            }
                        }
                    },
                    "required": ["batch"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": { "status": { "type": "string" } }
                })),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: true,
            },
        ],
        auth: Some(AuthSpec::ApiKey {
            env_var: "POSTHOG_API_KEY".into(),
            header: Some("Authorization".into()),
        }),
        events: Vec::new(),
        context_fields: Vec::new(),
    }
}

/// The Sentry integration: create issues and comments for error reporting.
fn sentry_manifest() -> IntegrationManifest {
    IntegrationManifest {
        id: "sentry".into(),
        version: "1.0.0".into(),
        display_name: "Sentry".into(),
        description: "Create Sentry issues and comments for error reporting.".into(),
        transport: McpTransport::Http,
        transport_config: TransportConfig::Http {
            url: "https://sentry.io/api/0".into(),
            headers: header_map(&[("Authorization", "Token ${SENTRY_AUTH_TOKEN}")]),
        },
        capabilities: vec![
            CapabilitySpec {
                name: "create_issue".into(),
                description: "Create a new Sentry issue.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "title": { "type": "string" },
                        "message": { "type": "string" }
                    },
                    "required": ["project", "title", "message"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": { "id": { "type": "string" }, "url": { "type": "string" } }
                })),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: false,
            },
            CapabilitySpec {
                name: "create_issue_comment".into(),
                description: "Add a comment to a Sentry issue.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "issue_id": { "type": "string" },
                        "body": { "type": "string" }
                    },
                    "required": ["issue_id", "body"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": { "id": { "type": "string" }, "url": { "type": "string" } }
                })),
                permission_mode: PermissionMode::WorkspaceWrite,
                idempotent: false,
            },
        ],
        auth: Some(AuthSpec::ApiKey {
            env_var: "SENTRY_AUTH_TOKEN".into(),
            header: Some("Authorization".into()),
        }),
        events: Vec::new(),
        context_fields: Vec::new(),
    }
}

fn header_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_providers_all_validate() {
        let registry = builtin_providers().expect("all built-in manifests must validate");
        assert_eq!(registry.list().len(), 4);
        for id in BUILTIN_PROVIDER_IDS {
            assert!(registry.get(id).is_some(), "missing built-in `{id}`");
        }
    }

    #[test]
    fn builtin_provider_ids_are_sorted_and_unique() {
        let ids: Vec<&str> = BUILTIN_PROVIDER_IDS.to_vec();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "BUILTIN_PROVIDER_IDS must be sorted");
        assert_eq!(
            ids.len(),
            ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
            "BUILTIN_PROVIDER_IDS must be unique"
        );
    }

    #[test]
    fn each_builtin_manifest_roundtrips_through_serde_json() {
        for manifest in [
            github_manifest(),
            linear_manifest(),
            posthog_manifest(),
            sentry_manifest(),
        ] {
            let original_id = manifest.id.clone();
            let json = serde_json::to_string(&manifest).expect("serialize");
            let parsed: IntegrationManifest = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, manifest, "roundtrip mismatch for {original_id}");
            assert!(parsed.validate().is_ok(), "{original_id} must validate");
        }
    }

    #[test]
    fn github_manifest_exposes_expected_capabilities() {
        let manifest = github_manifest();
        let names: Vec<&str> = manifest
            .capabilities
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["create_pr", "create_issue_comment", "create_check_run"]
        );
    }

    #[test]
    fn linear_manifest_targets_issue_comments() {
        let manifest = linear_manifest();
        let capability = manifest
            .capabilities
            .iter()
            .find(|c| c.name == "create_issue_comment")
            .expect("linear must expose create_issue_comment");
        assert_eq!(capability.permission_mode, PermissionMode::WorkspaceWrite);
        assert!(!capability.idempotent);
    }

    #[test]
    fn posthog_capabilities_are_idempotent_writes() {
        let manifest = posthog_manifest();
        assert!(
            !manifest.capabilities.is_empty(),
            "posthog must expose at least one capability"
        );
        for capability in &manifest.capabilities {
            assert!(
                capability.idempotent,
                "posthog capability `{}` must be idempotent",
                capability.name
            );
            assert_eq!(capability.permission_mode, PermissionMode::WorkspaceWrite);
        }
    }

    #[test]
    fn all_builtins_use_http_transport_with_api_key_auth() {
        for manifest in [
            github_manifest(),
            linear_manifest(),
            posthog_manifest(),
            sentry_manifest(),
        ] {
            assert!(
                matches!(manifest.transport, McpTransport::Http),
                "{} must use http transport",
                manifest.id
            );
            match manifest.auth {
                Some(AuthSpec::ApiKey { env_var, .. }) => {
                    assert!(!env_var.is_empty(), "{} env_var must be set", manifest.id);
                }
                other => panic!("{} must use api_key auth, got {other:?}", manifest.id),
            }
        }
    }
}
