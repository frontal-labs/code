//! Project policy: restricts which providers, models, auth methods, and permission
//! modes a workspace may use.
//!
//! Each settings source (user, project, local) may contribute a `policy` object. The
//! effective policy is the intersection of all of them, so a source can only add
//! restrictions. Nothing a repository sets here can widen what the user allowed.

use std::fmt;

use crate::config::{ConfigSource, ResolvedPermissionMode};

/// How a provider is authenticated for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthMethod {
    /// A static credential: an API key, or a bearer token set in the environment.
    ApiKey,
    /// A saved login from `frontal-code login`.
    OAuth,
}

impl AuthMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "apiKey",
            Self::OAuth => "oauth",
        }
    }
}

/// Restrictions contributed by one settings source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyLayer {
    pub source: ConfigSource,
    /// Provider names allowed, lowercase (for example `anthropic`). `None` means no limit.
    pub allowed_providers: Option<Vec<String>>,
    /// Model IDs allowed, after alias resolution. Entries may use `*` wildcards.
    pub allowed_models: Option<Vec<String>>,
    /// Auth methods allowed per provider name. A provider absent here is unrestricted.
    pub auth_methods: std::collections::BTreeMap<String, Vec<AuthMethod>>,
    /// Highest permission mode this source allows.
    pub permission_ceiling: Option<ResolvedPermissionMode>,
    /// Tool names that may not run at all.
    pub deny_tools: Vec<String>,
}

/// The effective policy: every layer must allow a request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectPolicy {
    layers: Vec<PolicyLayer>,
}

/// A request the policy refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyViolation {
    Provider {
        provider: String,
        source: ConfigSource,
    },
    Model {
        model: String,
        source: ConfigSource,
    },
    Auth {
        provider: String,
        method: AuthMethod,
        source: ConfigSource,
    },
}

impl fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provider { provider, source } => write!(
                f,
                "provider `{provider}` is not allowed by {}; allowed providers are set by its `policy.allowedProviders`",
                source_label(*source)
            ),
            Self::Model { model, source } => write!(
                f,
                "model `{model}` is not allowed by {}; check `policy.allowedModels`",
                source_label(*source)
            ),
            Self::Auth {
                provider,
                method,
                source,
            } => write!(
                f,
                "{} authentication for `{provider}` is not allowed by {}; check `policy.authMethods`",
                method.as_str(),
                source_label(*source)
            ),
        }
    }
}

impl ProjectPolicy {
    #[must_use]
    pub fn from_layers(layers: Vec<PolicyLayer>) -> Self {
        Self { layers }
    }

    #[must_use]
    pub fn layers(&self) -> &[PolicyLayer] {
        &self.layers
    }

    /// Refuses a provider that some layer does not list.
    pub fn check_provider(&self, provider: &str) -> Result<(), PolicyViolation> {
        let provider = provider.to_ascii_lowercase();
        for layer in &self.layers {
            if let Some(allowed) = &layer.allowed_providers {
                if !allowed.contains(&provider) {
                    return Err(PolicyViolation::Provider {
                        provider,
                        source: layer.source,
                    });
                }
            }
        }
        Ok(())
    }

    /// Refuses a model that some layer's allowlist does not match.
    pub fn check_model(&self, model: &str) -> Result<(), PolicyViolation> {
        for layer in &self.layers {
            if let Some(allowed) = &layer.allowed_models {
                if !allowed.iter().any(|pattern| glob_matches(pattern, model)) {
                    return Err(PolicyViolation::Model {
                        model: model.to_string(),
                        source: layer.source,
                    });
                }
            }
        }
        Ok(())
    }

    /// Refuses an auth method that some layer does not allow for `provider`.
    pub fn check_auth(&self, provider: &str, method: AuthMethod) -> Result<(), PolicyViolation> {
        let provider = provider.to_ascii_lowercase();
        for layer in &self.layers {
            if let Some(methods) = layer.auth_methods.get(&provider) {
                if !methods.contains(&method) {
                    return Err(PolicyViolation::Auth {
                        provider,
                        method,
                        source: layer.source,
                    });
                }
            }
        }
        Ok(())
    }

    /// Checks every rule that applies to one request. `auth` is `None` when the
    /// credential source is not known yet; the auth rule is then skipped.
    pub fn check_request(
        &self,
        provider: Option<&str>,
        model: Option<&str>,
        auth: Option<AuthMethod>,
    ) -> Result<(), PolicyViolation> {
        if let Some(provider) = provider {
            self.check_provider(provider)?;
            if let Some(auth) = auth {
                self.check_auth(provider, auth)?;
            }
        }
        if let Some(model) = model {
            self.check_model(model)?;
        }
        Ok(())
    }

    /// The lowest ceiling any layer sets, if any does.
    #[must_use]
    pub fn permission_ceiling(&self) -> Option<ResolvedPermissionMode> {
        self.layers
            .iter()
            .filter_map(|layer| layer.permission_ceiling)
            .min()
    }

    /// Caps a requested mode at the ceiling. Prompt-style and allow-all requests
    /// are reduced to the ceiling, because a user approving a prompt can otherwise
    /// go past it.
    #[must_use]
    pub fn cap_permission_mode(&self, requested: ResolvedPermissionMode) -> ResolvedPermissionMode {
        match self.permission_ceiling() {
            Some(ceiling) if requested > ceiling => ceiling,
            _ => requested,
        }
    }

    /// Tool names every layer's deny list has put out of bounds.
    #[must_use]
    pub fn deny_tools(&self) -> Vec<String> {
        let mut tools = Vec::new();
        for layer in &self.layers {
            for tool in &layer.deny_tools {
                if !tools.contains(tool) {
                    tools.push(tool.clone());
                }
            }
        }
        tools
    }
}

/// Matches `text` against `pattern`, where `*` matches any run of characters.
#[must_use]
pub fn glob_matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let first = parts[0];
    let last = parts[parts.len() - 1];
    if text.len() < first.len() + last.len() || !text.starts_with(first) || !text.ends_with(last) {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for middle in &parts[1..parts.len() - 1] {
        match rest.find(middle) {
            Some(index) => rest = &rest[index + middle.len()..],
            None => return false,
        }
    }
    true
}

fn source_label(source: ConfigSource) -> &'static str {
    match source {
        ConfigSource::User => "user settings",
        ConfigSource::Project => "project settings",
        ConfigSource::Local => "local settings",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{glob_matches, AuthMethod, PolicyLayer, PolicyViolation, ProjectPolicy};
    use crate::config::{ConfigSource, ResolvedPermissionMode};

    fn layer(source: ConfigSource) -> PolicyLayer {
        PolicyLayer {
            source,
            allowed_providers: None,
            allowed_models: None,
            auth_methods: BTreeMap::new(),
            permission_ceiling: None,
            deny_tools: Vec::new(),
        }
    }

    #[test]
    fn glob_matches_wildcards_and_exact_names() {
        assert!(glob_matches("claude-sonnet-*", "claude-sonnet-4-6"));
        assert!(glob_matches("*-codex", "gpt-5-codex"));
        assert!(glob_matches("gpt-*-codex", "gpt-5.3-codex"));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("exact", "exact"));
        assert!(!glob_matches("exact", "exact-plus"));
        assert!(!glob_matches("claude-sonnet-*", "claude-opus-5"));
        assert!(!glob_matches("gpt-*-codex", "gpt-codex"));
    }

    #[test]
    fn layers_intersect_so_any_layer_can_refuse() {
        let mut user = layer(ConfigSource::User);
        user.allowed_providers = Some(vec!["anthropic".to_string(), "openai".to_string()]);
        let mut project = layer(ConfigSource::Project);
        project.allowed_providers = Some(vec!["anthropic".to_string()]);
        let policy = ProjectPolicy::from_layers(vec![user, project]);

        assert!(policy.check_provider("anthropic").is_ok());
        assert_eq!(
            policy.check_provider("openai"),
            Err(PolicyViolation::Provider {
                provider: "openai".to_string(),
                source: ConfigSource::Project,
            })
        );
    }

    #[test]
    fn model_allowlist_must_match_in_every_layer() {
        let mut user = layer(ConfigSource::User);
        user.allowed_models = Some(vec!["claude-*".to_string()]);
        let mut project = layer(ConfigSource::Project);
        project.allowed_models = Some(vec!["claude-sonnet-*".to_string()]);
        let policy = ProjectPolicy::from_layers(vec![user, project]);

        assert!(policy.check_model("claude-sonnet-4-6").is_ok());
        assert!(policy.check_model("claude-opus-5").is_err());
        assert!(policy.check_model("gpt-5-codex").is_err());
    }

    #[test]
    fn auth_method_rules_apply_per_provider() {
        let mut project = layer(ConfigSource::Project);
        project
            .auth_methods
            .insert("anthropic".to_string(), vec![AuthMethod::OAuth]);
        let policy = ProjectPolicy::from_layers(vec![project]);

        assert!(policy.check_auth("anthropic", AuthMethod::OAuth).is_ok());
        assert!(policy.check_auth("anthropic", AuthMethod::ApiKey).is_err());
        // Providers without a rule are unrestricted.
        assert!(policy.check_auth("openai", AuthMethod::ApiKey).is_ok());
    }

    #[test]
    fn ceiling_is_the_lowest_layer_and_caps_requests() {
        let mut user = layer(ConfigSource::User);
        user.permission_ceiling = Some(ResolvedPermissionMode::WorkspaceWrite);
        let mut project = layer(ConfigSource::Project);
        project.permission_ceiling = Some(ResolvedPermissionMode::ReadOnly);
        let policy = ProjectPolicy::from_layers(vec![user, project]);

        assert_eq!(
            policy.permission_ceiling(),
            Some(ResolvedPermissionMode::ReadOnly)
        );
        assert_eq!(
            policy.cap_permission_mode(ResolvedPermissionMode::DangerFullAccess),
            ResolvedPermissionMode::ReadOnly
        );
        assert_eq!(
            policy.cap_permission_mode(ResolvedPermissionMode::ReadOnly),
            ResolvedPermissionMode::ReadOnly
        );
    }

    #[test]
    fn empty_policy_allows_everything() {
        let policy = ProjectPolicy::default();
        assert!(policy
            .check_request(Some("xai"), Some("grok-3"), Some(AuthMethod::ApiKey))
            .is_ok());
        assert_eq!(
            policy.cap_permission_mode(ResolvedPermissionMode::DangerFullAccess),
            ResolvedPermissionMode::DangerFullAccess
        );
    }
}
