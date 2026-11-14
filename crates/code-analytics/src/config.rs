//! Builder for [`AnalyticsClient`].
//!
//! Every input is explicit, so the builder never reads the environment on its
//! own. The CLI decides which environment variables and settings to pass in.

use std::path::PathBuf;
#[cfg(feature = "posthog")]
use std::sync::Arc;
use std::time::Duration;

use crate::client::AnalyticsClient;
use crate::consent::{resolve, Consent, ConsentInputs, DisabledReason};
#[cfg(feature = "posthog")]
use crate::posthog::{BackendConfig, PostHogSink};

/// Default ingestion host.
pub const DEFAULT_HOST: &str = "https://us.i.posthog.com";
/// Default events per batch.
pub const DEFAULT_BATCH_SIZE: usize = 20;
/// Default maximum time an event waits before it is sent.
pub const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(10);

/// The deployment environment, attached to every event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Environment {
    /// Released builds.
    #[default]
    Production,
    /// Local development builds.
    Development,
}

impl Environment {
    /// Every variant.
    pub const ALL: &'static [Self] = &[Self::Production, Self::Development];

    /// The label sent to the backend.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Development => "development",
        }
    }
}

/// Configures and creates an [`AnalyticsClient`].
///
/// [`AnalyticsBuilder::build`] never fails. If analytics is off or the backend
/// cannot start, it returns a disabled client that records why.
#[derive(Debug, Clone)]
#[must_use]
#[allow(clippy::struct_excessive_bools)] // Each flag is a separate, named opt-out signal.
pub struct AnalyticsBuilder {
    api_key: Option<String>,
    host: String,
    environment: Environment,
    batch_size: usize,
    flush_interval: Duration,
    config_home: Option<PathBuf>,
    app_version: String,
    settings_enabled: Option<bool>,
    env_enabled: bool,
    env_disabled: bool,
    do_not_track: bool,
    ci: bool,
    cli_opt_out: bool,
}

impl Default for AnalyticsBuilder {
    fn default() -> Self {
        Self {
            api_key: None,
            host: DEFAULT_HOST.to_string(),
            environment: Environment::default(),
            batch_size: DEFAULT_BATCH_SIZE,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            config_home: None,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            settings_enabled: None,
            env_enabled: false,
            env_disabled: false,
            do_not_track: false,
            ci: false,
            cli_opt_out: false,
        }
    }
}

impl AnalyticsBuilder {
    /// Sets the write-only `PostHog` project key. `None` or blank disables analytics.
    pub fn api_key(mut self, key: Option<String>) -> Self {
        self.api_key = key.filter(|k| !k.trim().is_empty());
        self
    }

    /// Sets the ingestion host.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Sets the environment label.
    pub fn environment(mut self, environment: Environment) -> Self {
        self.environment = environment;
        self
    }

    /// Sets events per batch.
    pub fn batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    /// Sets the maximum time an event waits before it is sent.
    pub fn flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = interval;
        self
    }

    /// Sets the directory that holds the anonymous install id.
    pub fn config_home(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config_home = Some(dir.into());
        self
    }

    /// Sets the application version attached to every event.
    pub fn app_version(mut self, version: impl Into<String>) -> Self {
        self.app_version = version.into();
        self
    }

    /// Sets `analytics.enabled` from the user settings file.
    pub fn settings_enabled(mut self, enabled: Option<bool>) -> Self {
        self.settings_enabled = enabled;
        self
    }

    /// Marks analytics as opted in through `FRONTAL_CODE_ANALYTICS_ENABLED`.
    pub fn env_enabled(mut self, enabled: bool) -> Self {
        self.env_enabled = enabled;
        self
    }

    /// Marks analytics as opted out through `FRONTAL_CODE_ANALYTICS_DISABLED`.
    pub fn env_disabled(mut self, disabled: bool) -> Self {
        self.env_disabled = disabled;
        self
    }

    /// Honours `DO_NOT_TRACK`.
    pub fn do_not_track(mut self, set: bool) -> Self {
        self.do_not_track = set;
        self
    }

    /// Disables analytics under CI.
    pub fn ci(mut self, set: bool) -> Self {
        self.ci = set;
        self
    }

    /// Records the `--no-analytics` flag.
    pub fn cli_opt_out(mut self, set: bool) -> Self {
        self.cli_opt_out = set;
        self
    }

    /// The consent inputs this builder would resolve.
    #[must_use]
    pub fn consent_inputs(&self) -> ConsentInputs {
        ConsentInputs {
            settings_enabled: self.settings_enabled,
            env_enabled: self.env_enabled,
            env_disabled: self.env_disabled,
            do_not_track: self.do_not_track,
            ci: self.ci,
            cli_opt_out: self.cli_opt_out,
            has_api_key: self.api_key.is_some(),
        }
    }

    /// Builds the client. Never fails; see the type-level docs.
    #[must_use]
    pub fn build(self) -> AnalyticsClient {
        match resolve(self.consent_inputs()) {
            Consent::Disabled(reason) => AnalyticsClient::disabled(reason),
            Consent::Enabled => self.start(),
        }
    }

    #[cfg(feature = "posthog")]
    fn start(self) -> AnalyticsClient {
        let config = BackendConfig {
            api_key: self.api_key.unwrap_or_default(),
            host: self.host,
            environment: self.environment,
            batch_size: self.batch_size,
            flush_interval: self.flush_interval,
            config_home: self.config_home,
            app_version: self.app_version,
        };
        match PostHogSink::new(&config) {
            Ok(sink) => AnalyticsClient::with_sink(Arc::new(sink)),
            Err(_) => AnalyticsClient::disabled(DisabledReason::BackendError),
        }
    }

    #[cfg(not(feature = "posthog"))]
    #[allow(clippy::unused_self)] // Same signature as the `posthog` variant.
    fn start(self) -> AnalyticsClient {
        AnalyticsClient::disabled(DisabledReason::NoBackend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_key_is_treated_as_missing() {
        let builder = AnalyticsBuilder::default().api_key(Some("   ".to_string()));
        assert!(!builder.consent_inputs().has_api_key);
    }

    #[test]
    fn default_build_is_disabled_without_opt_in() {
        let client = AnalyticsBuilder::default()
            .api_key(Some("phc_test".to_string()))
            .build();
        assert_eq!(
            client.consent(),
            Consent::Disabled(DisabledReason::NotEnabled)
        );
    }

    #[test]
    fn batch_size_is_at_least_one() {
        assert_eq!(AnalyticsBuilder::default().batch_size(0).batch_size, 1);
    }
}
