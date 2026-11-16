//! Typed, declarative integration manifests.
//!
//! An [`IntegrationManifest`] is the plugin contract for `FrontalCode` integrations:
//! the server, CLI, and SDKs all consume this shape. Manifests declare how to reach
//! an integration (transport + transport config), the capabilities (tools/endpoints)
//! it exposes, its auth requirements, the webhook events it can emit, and the fields
//! it writes into task context.

use crate::mcp::config::McpTransport;
use crate::mcp::tools::PermissionMode;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A fully typed, declarative definition of an integration.
/// This is the plugin contract: the server, CLI, and SDKs all consume this shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationManifest {
    /// Unique identifier (e.g., "github", "linear", "my-custom-api")
    pub id: String,
    /// Semver version of this manifest
    pub version: String,
    pub display_name: String,
    pub description: String,
    /// How to reach the integration. Reuses the existing MCP transport enum.
    pub transport: McpTransport,
    /// Transport-specific config (command/args/env for stdio, url for http, etc.)
    pub transport_config: TransportConfig,
    /// The capabilities (tools/endpoints) this integration exposes
    pub capabilities: Vec<CapabilitySpec>,
    /// Auth requirements, if any
    pub auth: Option<AuthSpec>,
    /// Webhook events this integration can emit (for webhook-capable integrations)
    pub events: Vec<EventSpec>,
    /// Fields this integration writes into task context (prefix-based)
    pub context_fields: Vec<ContextFieldSpec>,
}

/// Transport-specific configuration. Tagged enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
    },
    Sse {
        url: String,
        headers: BTreeMap<String, String>,
    },
    WebSocket {
        url: String,
    },
    Sdk {
        module: String,
    },
    ManagedProxy {
        proxy_url: String,
        target: String,
    },
}

/// A single capability (tool/endpoint) exposed by an integration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for input validation (industry standard, matches MCP)
    pub input_schema: serde_json::Value,
    /// JSON Schema for output validation
    pub output_schema: Option<serde_json::Value>,
    pub permission_mode: PermissionMode,
    /// If true, safe to retry on failure
    pub idempotent: bool,
}

/// Auth specification for an integration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthSpec {
    None,
    OAuth {
        client_id_env: String,
        client_secret_env: Option<String>,
        auth_url: String,
        token_url: String,
        scopes: Vec<String>,
        redirect_path: Option<String>,
    },
    ApiKey {
        env_var: String,
        header: Option<String>,
    },
}

/// A webhook event this integration can emit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the event payload
    pub payload_schema: serde_json::Value,
    /// Signature verification method
    pub signature: Option<SignatureSpec>,
}

/// Signature verification method for webhook events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignatureSpec {
    HmacSha256 { secret_env: String },
    None,
}

/// A field this integration writes into task context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFieldSpec {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

/// A single validation issue found while checking a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Dot-separated path to the offending field (e.g., "capabilities[2].name").
    pub field: String,
    /// Human-readable description of the problem.
    pub message: String,
}

impl IntegrationManifest {
    /// Validate this manifest, returning any issues found.
    ///
    /// Returns `Ok(())` when the manifest is well-formed.
    pub fn validate(&self) -> Result<(), Vec<ValidationIssue>> {
        let mut issues: Vec<ValidationIssue> = Vec::new();
        self.validate_id(&mut issues);
        self.validate_version(&mut issues);
        self.validate_capabilities(&mut issues);
        self.validate_auth(&mut issues);
        self.validate_events(&mut issues);
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }

    fn validate_id(&self, issues: &mut Vec<ValidationIssue>) {
        // `id`: non-empty, lowercase alphanumeric + hyphens, max 64 chars.
        if self.id.is_empty() {
            issues.push(ValidationIssue {
                field: "id".to_string(),
                message: "id must be non-empty".to_string(),
            });
        } else if self.id.len() > 64 {
            issues.push(ValidationIssue {
                field: "id".to_string(),
                message: format!("id must be at most 64 characters, got {}", self.id.len()),
            });
        } else if !id_re().is_match(&self.id) {
            issues.push(ValidationIssue {
                field: "id".to_string(),
                message: format!(
                    "id `{}` must be lowercase alphanumeric with hyphens only",
                    self.id
                ),
            });
        }

    }

    fn validate_version(&self, issues: &mut Vec<ValidationIssue>) {
        // `version`: must parse as semver.
        if !semver_re().is_match(&self.version) {
            issues.push(ValidationIssue {
                field: "version".to_string(),
                message: format!(
                    "version `{}` is not valid semver (expected e.g. `1.2.3`)",
                    self.version
                ),
            });
        }

    }

    fn validate_capabilities(&self, issues: &mut Vec<ValidationIssue>) {
        // `capabilities`: must be non-empty, names unique, schemas valid JSON.
        if self.capabilities.is_empty() {
            issues.push(ValidationIssue {
                field: "capabilities".to_string(),
                message: "capabilities must be non-empty".to_string(),
            });
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for (index, capability) in self.capabilities.iter().enumerate() {
            let field = format!("capabilities[{index}]");
            if !seen.insert(&capability.name) {
                issues.push(ValidationIssue {
                    field: format!("{field}.name"),
                    message: format!("duplicate capability name `{}`", capability.name),
                });
            }
            if capability.input_schema.is_null() {
                issues.push(ValidationIssue {
                    field: format!("{field}.input_schema"),
                    message: "input_schema must be valid JSON".to_string(),
                });
            }
            if let Some(output_schema) = &capability.output_schema {
                if output_schema.is_null() {
                    issues.push(ValidationIssue {
                        field: format!("{field}.output_schema"),
                        message: "output_schema must be valid JSON".to_string(),
                    });
                }
            }
        }

    }

    fn validate_auth(&self, issues: &mut Vec<ValidationIssue>) {
        // `auth`: env var names must be non-empty strings.
        match &self.auth {
            Some(AuthSpec::OAuth {
                client_id_env,
                client_secret_env,
                ..
            }) => {
                if client_id_env.is_empty() {
                    issues.push(ValidationIssue {
                        field: "auth.client_id_env".to_string(),
                        message: "auth.client_id_env must be a non-empty string".to_string(),
                    });
                }
                if let Some(secret) = client_secret_env {
                    if secret.is_empty() {
                        issues.push(ValidationIssue {
                            field: "auth.client_secret_env".to_string(),
                            message: "auth.client_secret_env must be a non-empty string"
                                .to_string(),
                        });
                    }
                }
            }
            Some(AuthSpec::ApiKey { env_var, .. }) => {
                if env_var.is_empty() {
                    issues.push(ValidationIssue {
                        field: "auth.env_var".to_string(),
                        message: "auth.env_var must be a non-empty string".to_string(),
                    });
                }
            }
            Some(AuthSpec::None) | None => {}
        }

    }

    fn validate_events(&self, issues: &mut Vec<ValidationIssue>) {
        // `events`: names must be unique.
        let mut seen_events: BTreeSet<&str> = BTreeSet::new();
        for (index, event) in self.events.iter().enumerate() {
            let field = format!("events[{index}]");
            if !seen_events.insert(&event.name) {
                issues.push(ValidationIssue {
                    field: format!("{field}.name"),
                    message: format!("duplicate event name `{}`", event.name),
                });
            }
        }
    }
}

/// Registry of integration manifests, keyed by manifest id.
#[derive(Debug, Clone, Default)]
pub struct ManifestRegistry {
    manifests: BTreeMap<String, IntegrationManifest>,
}

impl ManifestRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a manifest, rejecting it if it fails validation
    /// or if a manifest with the same id is already registered.
    pub fn register(&mut self, manifest: IntegrationManifest) -> Result<(), Vec<ValidationIssue>> {
        manifest.validate()?;
        if self.manifests.contains_key(&manifest.id) {
            return Err(vec![ValidationIssue {
                field: "id".to_string(),
                message: format!("manifest with id `{}` is already registered", manifest.id),
            }]);
        }
        self.manifests.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    /// Remove a manifest by id, returning it if it was present.
    pub fn remove(&mut self, id: &str) -> Option<IntegrationManifest> {
        self.manifests.remove(id)
    }

    /// Look up a manifest by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&IntegrationManifest> {
        self.manifests.get(id)
    }

    /// List all registered manifests in id order.
    #[must_use]
    pub fn list(&self) -> Vec<&IntegrationManifest> {
        self.manifests.values().collect()
    }

    /// Look up a single capability on a registered manifest.
    #[must_use]
    pub fn capability(
        &self,
        integration_id: &str,
        capability: &str,
    ) -> Option<&CapabilitySpec> {
        self.manifests
            .get(integration_id)?
            .capabilities
            .iter()
            .find(|spec| spec.name == capability)
    }
}

static ID_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
static SEMVER_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn id_re() -> &'static Regex {
    ID_RE.get_or_init(|| Regex::new("^[a-z0-9]+(-[a-z0-9]+)*$").expect("static regex is valid"))
}

fn semver_re() -> &'static Regex {
    SEMVER_RE.get_or_init(|| {
        Regex::new(
            r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$",
        )
        .expect("static regex is valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> IntegrationManifest {
        IntegrationManifest {
            id: "github".to_string(),
            version: "1.0.0".to_string(),
            display_name: "GitHub".to_string(),
            description: "GitHub integration".to_string(),
            transport: McpTransport::Stdio,
            transport_config: TransportConfig::Stdio {
                command: "gh-mcp".to_string(),
                args: vec!["serve".to_string()],
                env: BTreeMap::new(),
            },
            capabilities: vec![CapabilitySpec {
                name: "create_issue".to_string(),
                description: "Create an issue".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: Some(serde_json::json!({"type": "object"})),
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
                payload_schema: serde_json::json!({"type": "object"}),
                signature: None,
            }],
            context_fields: vec![ContextFieldSpec {
                name: "github.issue".to_string(),
                description: "The linked GitHub issue".to_string(),
                schema: serde_json::json!({"type": "object"}),
            }],
        }
    }

    #[test]
    fn valid_manifest_roundtrips_through_serde_json() {
        let manifest = valid_manifest();
        let json = serde_json::to_string(&manifest).expect("serialize");
        let parsed: IntegrationManifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, manifest.id);
        assert_eq!(parsed.version, manifest.version);
        assert_eq!(parsed.capabilities.len(), manifest.capabilities.len());
        assert!(matches!(parsed.transport, McpTransport::Stdio));
        assert!(matches!(parsed.transport_config, TransportConfig::Stdio { .. }));
        assert!(matches!(parsed.auth, Some(AuthSpec::ApiKey { .. })));
        assert_eq!(parsed.events.len(), manifest.events.len());
        assert_eq!(parsed.context_fields.len(), manifest.context_fields.len());
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn validation_catches_duplicate_capability_names() {
        let mut manifest = valid_manifest();
        manifest.capabilities.push(CapabilitySpec {
            name: "create_issue".to_string(),
            description: "Duplicate".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: None,
            permission_mode: PermissionMode::ReadOnly,
            idempotent: true,
        });
        let issues = manifest.validate().expect_err("should fail");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field == "capabilities[1].name"
                    && issue.message.contains("duplicate capability name")),
            "expected duplicate capability issue, got {issues:?}"
        );
    }

    #[test]
    fn validation_catches_invalid_id_format() {
        let mut manifest = valid_manifest();
        manifest.id = "GitHub_1".to_string();
        let issues = manifest.validate().expect_err("should fail");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field == "id" && issue.message.contains("lowercase")),
            "expected id format issue, got {issues:?}"
        );
    }

    #[test]
    fn validation_catches_invalid_version() {
        let mut manifest = valid_manifest();
        manifest.version = "not-a-version".to_string();
        let issues = manifest.validate().expect_err("should fail");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field == "version" && issue.message.contains("semver")),
            "expected semver issue, got {issues:?}"
        );
    }

    #[test]
    fn validation_catches_empty_capabilities() {
        let mut manifest = valid_manifest();
        manifest.capabilities.clear();
        let issues = manifest.validate().expect_err("should fail");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field == "capabilities"
                    && issue.message.contains("non-empty")),
            "expected empty capabilities issue, got {issues:?}"
        );
    }

    #[test]
    fn validation_catches_duplicate_event_names() {
        let mut manifest = valid_manifest();
        manifest.events.push(EventSpec {
            name: "issue.opened".to_string(),
            description: "Duplicate".to_string(),
            payload_schema: serde_json::json!({"type": "object"}),
            signature: None,
        });
        let issues = manifest.validate().expect_err("should fail");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field == "events[1].name"
                    && issue.message.contains("duplicate event name")),
            "expected duplicate event issue, got {issues:?}"
        );
    }

    #[test]
    fn validation_catches_empty_auth_env_vars() {
        let mut manifest = valid_manifest();
        manifest.auth = Some(AuthSpec::ApiKey {
            env_var: String::new(),
            header: None,
        });
        let issues = manifest.validate().expect_err("should fail");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field == "auth.env_var"
                    && issue.message.contains("non-empty")),
            "expected empty env var issue, got {issues:?}"
        );
    }

    #[test]
    fn registry_register_get_list_capability_work() {
        let mut registry = ManifestRegistry::new();
        let manifest = valid_manifest();
        registry.register(manifest.clone()).expect("register");

        assert!(registry.get("github").is_some());
        assert!(registry.get("missing").is_none());
        assert_eq!(registry.list(), [&manifest]);
        assert!(registry.capability("github", "create_issue").is_some());
        assert!(registry.capability("github", "missing").is_none());
        assert!(registry.capability("missing", "create_issue").is_none());

        // Registering the same id again is rejected.
        let err = registry
            .register(manifest)
            .expect_err("duplicate id must fail");
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "id");
    }

    #[test]
    fn registry_rejects_invalid_manifest() {
        let mut registry = ManifestRegistry::new();
        let mut manifest = valid_manifest();
        manifest.id = "Bad Id!".to_string();
        let err = registry.register(manifest).expect_err("invalid manifest must fail");
        assert!(
            err.iter().any(|issue| issue.field == "id"),
            "expected id issue, got {err:?}"
        );
    }
}
