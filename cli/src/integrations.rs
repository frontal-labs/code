//! `frontal-code integrations` subcommand handlers.
//!
//! Loads typed [`IntegrationManifest`]s from the project settings
//! (`.frontal-code/settings.json` → `mcp.integrations`) and the
//! `FRONTAL_CODE_INTEGRATIONS_CONFIG` environment variable, then renders
//! them for the CLI.

use std::path::Path;

use code_integrations::mcp::config::McpTransport;
use code_integrations::mcp::tools::PermissionMode;
use code_integrations::{
    AuthSpec, CapabilitySpec, EventSpec, IntegrationManifest, ManifestRegistry, SignatureSpec,
    TransportConfig, ValidationIssue,
};
use serde_json::{json, Value};

/// Environment variable holding a JSON array of integration manifests.
pub const ENV_INTEGRATIONS_CONFIG: &str = "FRONTAL_CODE_INTEGRATIONS_CONFIG";

/// Subcommand help text shown with no arguments or an unknown subcommand.
#[must_use]
pub fn help_text() -> &'static str {
    "Usage: frontal-code integrations <subcommand> [options]

Subcommands:
  list                       List all configured integrations
  show <id>                  Show full details for an integration
  capabilities <id>          List the capabilities of an integration
  validate <path>            Validate a manifest file

Options:
  --json                     Emit JSON instead of a table"
}

/// Load the manifest registry: the built-in providers (GitHub, Linear,
/// `PostHog`, and Sentry) as defaults, layered with settings discovered by
/// walking up from `cwd` and the `FRONTAL_CODE_INTEGRATIONS_CONFIG` env var.
///
/// Configuration entries with the same id as a built-in override it; entries
/// that fail to parse are skipped (invalid entries cannot be registered
/// anyway).
pub fn load_registry(cwd: &Path) -> ManifestRegistry {
    let mut registry =
        code_integrations::builtin_providers().expect("built-in provider manifests are valid");

    if let Some(settings) = find_settings_file(cwd) {
        if let Ok(contents) = std::fs::read_to_string(&settings) {
            let Ok(parsed) = serde_json::from_str::<Value>(&contents) else {
                return registry;
            };
            if let Some(integrations) = parsed
                .get("mcp")
                .and_then(|mcp| mcp.get("integrations"))
                .and_then(|value| value.as_object())
            {
                for (_, manifest_json) in integrations {
                    let Ok(manifest) =
                        serde_json::from_value::<IntegrationManifest>(manifest_json.clone())
                    else {
                        continue;
                    };
                    register_or_override(&mut registry, manifest);
                }
            }
        }
    }

    if let Ok(raw) = std::env::var(ENV_INTEGRATIONS_CONFIG) {
        let Ok(entries) = serde_json::from_str::<Vec<IntegrationManifest>>(&raw) else {
            return registry;
        };
        for manifest in entries {
            register_or_override(&mut registry, manifest);
        }
    }

    registry
}

/// Register a manifest, replacing any existing manifest with the same id so
/// that project configuration can override a built-in provider.
fn register_or_override(registry: &mut ManifestRegistry, manifest: IntegrationManifest) {
    let id = manifest.id.clone();
    if registry.get(&id).is_some() {
        registry.remove(&id);
    }
    let _ = registry.register(manifest);
}

/// Walk up from `cwd` (inclusive) looking for `.frontal-code/settings.json`.
fn find_settings_file(cwd: &Path) -> Option<std::path::PathBuf> {
    let mut current: Option<&Path> = Some(cwd);
    while let Some(dir) = current {
        let candidate = dir.join(".frontal-code").join("settings.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        current = dir.parent();
    }
    None
}

/// Summary of one integration for table/JSON rendering.
#[derive(serde::Serialize)]
struct IntegrationSummary {
    id: String,
    version: String,
    display_name: String,
    capability_count: usize,
    auth: String,
}

fn summarize(manifest: &IntegrationManifest) -> IntegrationSummary {
    IntegrationSummary {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        display_name: manifest.display_name.clone(),
        capability_count: manifest.capabilities.len(),
        auth: auth_label(manifest.auth.as_ref()),
    }
}

fn auth_label(auth: Option<&AuthSpec>) -> String {
    match auth {
        Some(AuthSpec::OAuth { .. }) => "oauth".to_string(),
        Some(AuthSpec::ApiKey { .. }) => "api_key".to_string(),
        Some(AuthSpec::None) | None => "none".to_string(),
    }
}

fn transport_label(transport: &McpTransport) -> String {
    match transport {
        McpTransport::Stdio => "stdio",
        McpTransport::Sse => "sse",
        McpTransport::Http => "http",
        McpTransport::WebSocket => "websocket",
        McpTransport::Sdk => "sdk",
        McpTransport::ManagedProxy => "managed_proxy",
    }
    .to_string()
}

fn permission_label(mode: PermissionMode) -> String {
    match mode {
        PermissionMode::ReadOnly => "read-only",
        PermissionMode::WorkspaceWrite => "workspace-write",
        PermissionMode::DangerFullAccess => "danger-full-access",
    }
    .to_string()
}

fn transport_detail(config: &TransportConfig) -> String {
    match config {
        TransportConfig::Stdio { command, args, .. } => {
            let args = args
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {args}")
            }
        }
        TransportConfig::Http { url, .. }
        | TransportConfig::Sse { url, .. }
        | TransportConfig::WebSocket { url } => url.clone(),
        TransportConfig::Sdk { module } => module.clone(),
        TransportConfig::ManagedProxy { target, .. } => target.clone(),
    }
}

/// Render the integration list as a human-readable table.
pub fn list_integrations(registry: &ManifestRegistry) -> String {
    let manifests = registry.list();
    let mut lines = vec![format!("INTEGRATIONS ({})", manifests.len())];
    if manifests.is_empty() {
        lines.push(String::new());
        lines.push("  No integrations configured.".to_string());
        return lines.join("\n");
    }
    lines.push(String::new());

    let id_width = manifests
        .iter()
        .map(|manifest| manifest.id.len())
        .max()
        .unwrap_or(0)
        .max("ID".len());
    let name_width = manifests
        .iter()
        .map(|manifest| manifest.display_name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    lines.push(format!(
        "  {id:<id_w$}  {version:<9} {name:<name_w$}  {caps:<5} {auth}",
        id = "ID",
        id_w = id_width,
        version = "VERSION",
        name = "NAME",
        name_w = name_width,
        caps = "CAPS",
        auth = "AUTH",
    ));
    for manifest in manifests {
        let summary = summarize(manifest);
        lines.push(format!(
            "  {id:<id_w$}  {version:<9} {name:<name_w$}  {caps:<5} {auth}",
            id = summary.id,
            id_w = id_width,
            version = summary.version,
            name = summary.display_name,
            name_w = name_width,
            caps = summary.capability_count,
            auth = summary.auth,
        ));
    }
    lines.join("\n")
}

/// Render the integration list as a pretty-printed JSON array of summaries.
pub fn list_integrations_json(registry: &ManifestRegistry) -> String {
    let summaries: Vec<Value> = registry
        .list()
        .iter()
        .map(|manifest| json!(summarize(manifest)))
        .collect();
    serde_json::to_string_pretty(&Value::Array(summaries)).unwrap_or_else(|_| "[]".to_string())
}

/// Render full detail for one integration.
pub fn show_integration(registry: &ManifestRegistry, id: &str) -> Result<String, String> {
    let manifest = registry.get(id).ok_or_else(|| {
        format!("integration `{id}` not found (see `frontal-code integrations list`)")
    })?;

    let transport = format!(
        "{} ({})",
        transport_label(&manifest.transport),
        transport_detail(&manifest.transport_config)
    );
    let auth = auth_detail(manifest.auth.as_ref()).unwrap_or_else(|| "none".to_string());
    let mut lines = vec![
        format!("INTEGRATION: {}", manifest.id),
        String::new(),
        format!("  {id:<16}", id = manifest.id),
        format!("  version: {version}", version = manifest.version),
        format!(
            "  name: {display_name}",
            display_name = manifest.display_name
        ),
        format!(
            "  description: {description}",
            description = manifest.description
        ),
        format!("  transport: {transport}"),
        format!("  auth: {auth}"),
    ];

    lines.push(String::new());
    lines.push(format!("  CAPABILITIES ({}):", manifest.capabilities.len()));
    for capability in &manifest.capabilities {
        let idempotent = if capability.idempotent {
            " idempotent"
        } else {
            ""
        };
        lines.push(format!(
            "    {name:<20} [{permission}{idempotent}",
            name = capability.name,
            permission = permission_label(capability.permission_mode),
            idempotent = idempotent,
        ));
        lines.push(format!("      {}", capability.description));
    }

    if !manifest.events.is_empty() {
        lines.push(String::new());
        lines.push(format!("  EVENTS ({}):", manifest.events.len()));
        for event in &manifest.events {
            let signature = event
                .signature
                .as_ref()
                .map_or_else(|| "none".to_string(), signature_label);
            lines.push(format!(
                "    {name:<20} {description}",
                name = event.name,
                description = event.description
            ));
            lines.push(format!("      signature: {signature}"));
        }
    }

    if !manifest.context_fields.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "  CONTEXT FIELDS ({}):",
            manifest.context_fields.len()
        ));
        for field in &manifest.context_fields {
            lines.push(format!(
                "    {name:<20} {description}",
                name = field.name,
                description = field.description
            ));
        }
    }

    Ok(lines.join("\n"))
}

fn signature_label(signature: &SignatureSpec) -> String {
    match signature {
        SignatureSpec::HmacSha256 { secret_env } => format!("hmac_sha256 ({secret_env})"),
        SignatureSpec::None => "none".to_string(),
    }
}

fn auth_detail(auth: Option<&AuthSpec>) -> Option<String> {
    match auth {
        Some(AuthSpec::OAuth {
            client_id_env,
            client_secret_env,
            auth_url,
            scopes,
            ..
        }) => Some(format!(
            "oauth (client_id_env={client_id_env}, client_secret_env={}, scopes=[{}]) auth_url={auth_url}",
            client_secret_env
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("unset"),
            scopes.join(", "),
        )),
        Some(AuthSpec::ApiKey { env_var, header }) => Some(format!(
            "api_key (env_var={env_var}, header={})",
            header.as_deref().unwrap_or("default")
        )),
        Some(AuthSpec::None) => Some("none".to_string()),
        None => None,
    }
}

/// Render full detail for one integration as a JSON object.
pub fn show_integration_json(registry: &ManifestRegistry, id: &str) -> Result<String, String> {
    let manifest = registry.get(id).ok_or_else(|| {
        format!("integration `{id}` not found (see `frontal-code integrations list`)")
    })?;
    serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("failed to serialize integration `{id}`: {error}"))
}

/// Render the capability table for one integration.
pub fn list_capabilities(registry: &ManifestRegistry, id: &str) -> Result<String, String> {
    let manifest = registry.get(id).ok_or_else(|| {
        format!("integration `{id}` not found (see `frontal-code integrations list`)")
    })?;

    let mut lines = vec![format!(
        "CAPABILITIES: {id} ({})",
        manifest.capabilities.len()
    )];
    if manifest.capabilities.is_empty() {
        lines.push(String::new());
        lines.push("  No capabilities declared.".to_string());
        return Ok(lines.join("\n"));
    }
    lines.push(String::new());

    let name_width = manifest
        .capabilities
        .iter()
        .map(|capability| capability.name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    lines.push(format!(
        "  {name:<name_w$}  {perm:<18} {idempotent:<10} DESCRIPTION",
        name = "NAME",
        name_w = name_width,
        perm = "PERMISSION",
        idempotent = "IDEMPOTENT",
    ));
    for capability in &manifest.capabilities {
        let permission = permission_label(capability.permission_mode);
        let idempotent = if capability.idempotent { "yes" } else { "no" };
        lines.push(format!(
            "  {name:<name_w$}  {permission:<18} {idempotent:<10} {description}",
            name = capability.name,
            name_w = name_width,
            permission = permission,
            idempotent = idempotent,
            description = capability.description,
        ));
    }
    Ok(lines.join("\n"))
}

/// Render the capability list for one integration as a JSON array.
pub fn list_capabilities_json(registry: &ManifestRegistry, id: &str) -> Result<String, String> {
    let manifest = registry.get(id).ok_or_else(|| {
        format!("integration `{id}` not found (see `frontal-code integrations list`)")
    })?;
    let capabilities: Vec<Value> = manifest
        .capabilities
        .iter()
        .map(|capability| {
            let summary = json!(capability);
            json!({
                "name": summary.get("name"),
                "description": summary.get("description"),
                "permission_mode": summary.get("permission_mode"),
                "idempotent": summary.get("idempotent"),
            })
        })
        .collect();
    serde_json::to_string_pretty(&Value::Array(capabilities))
        .map_err(|error| format!("failed to serialize capabilities for `{id}`: {error}"))
}

/// Parse a file as an [`IntegrationManifest`] and run its validation.
///
/// Returns a human-readable report: `valid` when the manifest passes, or a
/// list of issues otherwise.
pub fn validate_manifest_file(path: &Path) -> Result<String, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let manifest: IntegrationManifest = serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    match manifest.validate() {
        Ok(()) => Ok(format!(
            "{}: valid ({} {})",
            path.display(),
            manifest.id,
            manifest.version
        )),
        Err(issues) => Ok(render_validation_report(path, &manifest, issues)),
    }
}

fn render_validation_report(
    path: &Path,
    manifest: &IntegrationManifest,
    issues: Vec<ValidationIssue>,
) -> String {
    let mut lines = vec![format!(
        "{}: invalid ({} {}, {} issue(s))",
        path.display(),
        manifest.id,
        manifest.version,
        issues.len()
    )];
    for issue in issues {
        lines.push(format!(
            "  {field}: {message}",
            field = issue.field,
            message = issue.message
        ));
    }
    lines.join("\n")
}

/// JSON report for `validate`.
pub fn validate_manifest_file_json(path: &Path) -> Result<String, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let manifest: IntegrationManifest = serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    let issues = match manifest.validate() {
        Ok(()) => Vec::new(),
        Err(issues) => issues,
    };
    let report = json!({
        "path": path.display().to_string(),
        "id": manifest.id,
        "version": manifest.version,
        "valid": issues.is_empty(),
        "issues": issues.iter().map(|issue| json!({
            "field": issue.field,
            "message": issue.message,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&report)
        .map_err(|error| format!("failed to serialize validation report: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards tests that read `FRONTAL_CODE_INTEGRATIONS_CONFIG`; cargo runs
    /// tests on multiple threads and env vars are process-global.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn test_registry() -> ManifestRegistry {
        let manifest = IntegrationManifest {
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
            capabilities: vec![
                CapabilitySpec {
                    name: "create_issue".to_string(),
                    description: "Create an issue".to_string(),
                    input_schema: json!({"type": "object"}),
                    output_schema: Some(json!({"type": "object"})),
                    permission_mode: PermissionMode::WorkspaceWrite,
                    idempotent: false,
                },
                CapabilitySpec {
                    name: "list_issues".to_string(),
                    description: "List issues".to_string(),
                    input_schema: json!({"type": "object"}),
                    output_schema: None,
                    permission_mode: PermissionMode::ReadOnly,
                    idempotent: true,
                },
            ],
            auth: Some(AuthSpec::OAuth {
                client_id_env: "GITHUB_OAUTH_CLIENT_ID".to_string(),
                client_secret_env: Some("GITHUB_OAUTH_CLIENT_SECRET".to_string()),
                auth_url: "https://github.com/login/oauth/authorize".to_string(),
                token_url: "https://github.com/login/oauth/access_token".to_string(),
                scopes: vec!["repo".to_string()],
                redirect_path: None,
            }),
            events: Vec::<EventSpec>::new(),
            context_fields: Vec::new(),
        };
        let mut registry = ManifestRegistry::new();
        registry.register(manifest).expect("register");
        registry
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "frontal-integrations-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn load_registry_reads_settings_file_and_stops_at_first_match() {
        let dir = temp_dir("settings");
        let settings_dir = dir.join(".frontal-code");
        std::fs::create_dir_all(&settings_dir).expect("create settings dir");
        let manifest_json = json!({
            "id": "linear",
            "version": "2.1.0",
            "display_name": "Linear",
            "description": "Linear integration",
            "transport": "stdio",
            "transport_config": {
                "kind": "stdio",
                "command": "linear-mcp",
                "args": ["serve"],
                "env": {}
            },
            "capabilities": [{
                "name": "create_issue",
                "description": "Create an issue",
                "input_schema": {"type": "object"},
                "permission_mode": "workspace_write",
                "idempotent": false
            }],
            "auth": {
                "kind": "api_key",
                "env_var": "LINEAR_API_KEY",
                "header": null
            },
            "events": [],
            "context_fields": []
        });
        let settings = json!({ "mcp": { "integrations": { "linear": manifest_json } } });
        std::fs::write(
            settings_dir.join("settings.json"),
            serde_json::to_string_pretty(&settings).expect("serialize"),
        )
        .expect("write settings");

        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        std::env::remove_var(ENV_INTEGRATIONS_CONFIG);
        let registry = load_registry(&dir);
        // The built-in linear manifest is overridden by the settings file;
        // the other built-ins remain.
        assert_eq!(registry.list().len(), 4);
        let linear = registry.get("linear").expect("linear registered");
        assert_eq!(linear.version, "2.1.0");
        assert!(matches!(linear.auth, Some(AuthSpec::ApiKey { .. })));
        assert!(registry.get("github").is_some());
        assert!(registry.get("sentry").is_some());
    }

    #[test]
    fn load_registry_seeds_builtins_when_no_config() {
        let dir = temp_dir("empty");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        std::env::remove_var(ENV_INTEGRATIONS_CONFIG);
        let registry = load_registry(&dir);
        assert_eq!(registry.list().len(), 4);
        for id in code_integrations::BUILTIN_PROVIDER_IDS {
            assert!(registry.get(id).is_some(), "missing built-in `{id}`");
        }
    }

    #[test]
    fn list_integrations_renders_table() {
        let registry = test_registry();
        let output = list_integrations(&registry);
        assert!(output.contains("INTEGRATIONS (1)"), "got: {output}");
        assert!(output.contains("  ID      VERSION"), "got: {output}");
        assert!(output.contains("github"), "got: {output}");
        assert!(output.contains("1.0.0"), "got: {output}");
        assert!(output.contains("GitHub"), "got: {output}");
        assert!(output.contains("oauth"), "got: {output}");
    }

    #[test]
    fn list_integrations_json_renders_array() {
        let registry = test_registry();
        let output = list_integrations_json(&registry);
        let parsed: Value = serde_json::from_str(&output).expect("valid json");
        let array = parsed.as_array().expect("array");
        assert_eq!(array.len(), 1);
        assert_eq!(array[0].get("id").and_then(Value::as_str), Some("github"));
        assert_eq!(
            array[0].get("capability_count").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(array[0].get("auth").and_then(Value::as_str), Some("oauth"));
    }

    #[test]
    fn show_integration_known_id() {
        let registry = test_registry();
        let output = show_integration(&registry, "github").expect("known id");
        assert!(output.contains("INTEGRATION: github"), "got: {output}");
        assert!(output.contains("gh-mcp serve"), "got: {output}");
        assert!(output.contains("create_issue"), "got: {output}");
        assert!(output.contains("workspace-write"), "got: {output}");
        assert!(output.contains("idempotent"), "got: {output}");
    }

    #[test]
    fn show_integration_unknown_id() {
        let registry = test_registry();
        let error = show_integration(&registry, "missing").expect_err("unknown id fails");
        assert!(error.contains("`missing` not found"), "got: {error}");
    }

    #[test]
    fn show_integration_json_roundtrips() {
        let registry = test_registry();
        let output = show_integration_json(&registry, "github").expect("known id");
        let parsed: IntegrationManifest =
            serde_json::from_str(&output).expect("valid manifest json");
        assert_eq!(parsed.id, "github");
    }

    #[test]
    fn list_capabilities_renders_table() {
        let registry = test_registry();
        let output = list_capabilities(&registry, "github").expect("known id");
        assert!(output.contains("CAPABILITIES: github (2)"), "got: {output}");
        assert!(output.contains("create_issue"), "got: {output}");
        assert!(output.contains("list_issues"), "got: {output}");
        assert!(output.contains("read-only"), "got: {output}");
        assert!(output.contains("no"), "got: {output}");
    }

    #[test]
    fn list_capabilities_unknown_id() {
        let registry = test_registry();
        let error = list_capabilities(&registry, "missing").expect_err("unknown id fails");
        assert!(error.contains("`missing` not found"), "got: {error}");
    }

    fn write_manifest(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("write manifest");
        path
    }

    fn valid_manifest_json() -> String {
        serde_json::to_string_pretty(&json!({
            "id": "github",
            "version": "1.0.0",
            "display_name": "GitHub",
            "description": "GitHub integration",
            "transport": "stdio",
            "transport_config": {
                "kind": "stdio",
                "command": "gh-mcp",
                "args": ["serve"],
                "env": {}
            },
            "capabilities": [{
                "name": "create_issue",
                "description": "Create an issue",
                "input_schema": {"type": "object"},
                "permission_mode": "workspace_write",
                "idempotent": false
            }],
            "auth": {
                "kind": "api_key",
                "env_var": "GITHUB_TOKEN",
                "header": null
            },
            "events": [],
            "context_fields": []
        }))
        .expect("serialize")
    }

    #[test]
    fn validate_manifest_file_valid() {
        let dir = temp_dir("validate-valid");
        let path = write_manifest(&dir, "valid.json", &valid_manifest_json());
        let output = validate_manifest_file(&path).expect("valid manifest");
        assert!(output.contains("valid"), "got: {output}");
        assert!(output.contains("github"), "got: {output}");
    }

    #[test]
    fn validate_manifest_file_invalid_reports_issues() {
        let dir = temp_dir("validate-invalid");
        let bad = valid_manifest_json().replace("\"github\"", "\"Bad Id!\"");
        let path = write_manifest(&dir, "invalid.json", &bad);
        let output = validate_manifest_file(&path).expect("parses but is invalid");
        assert!(output.contains("invalid"), "got: {output}");
        assert!(output.contains("id"), "got: {output}");
    }

    #[test]
    fn validate_manifest_file_unparseable() {
        let dir = temp_dir("validate-unparseable");
        let path = write_manifest(&dir, "unparseable.json", "{ not json");
        let error = validate_manifest_file(&path).expect_err("unparseable fails");
        assert!(error.contains("failed to parse"), "got: {error}");
    }

    #[test]
    fn validate_manifest_file_json_shape() {
        let dir = temp_dir("validate-json");
        let path = write_manifest(&dir, "valid.json", &valid_manifest_json());
        let output = validate_manifest_file_json(&path).expect("valid manifest");
        let parsed: Value = serde_json::from_str(&output).expect("valid json");
        assert_eq!(parsed.get("valid").and_then(Value::as_bool), Some(true));
        assert!(parsed.get("issues").and_then(Value::as_array).is_some());
    }
}
