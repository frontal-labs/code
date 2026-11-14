//! Opt-in and opt-out resolution.
//!
//! Analytics is off unless the user opts in. Any opt-out signal wins over an
//! opt-in, and the first matching reason is recorded so `telemetry status` can
//! say why events are not being sent.

use std::fmt::{Display, Formatter};

/// Inputs gathered from the environment, flags and user settings.
///
/// Kept as plain data so [`resolve`] is a pure function that tests can drive
/// without touching process state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Each flag is a separate, named opt-out signal.
pub struct ConsentInputs {
    /// `analytics.enabled` from the user settings file, if set.
    pub settings_enabled: Option<bool>,
    /// `FRONTAL_CODE_ANALYTICS_ENABLED` is set to a truthy value.
    pub env_enabled: bool,
    /// `FRONTAL_CODE_ANALYTICS_DISABLED` is set to a truthy value.
    pub env_disabled: bool,
    /// `DO_NOT_TRACK` is set to a truthy value.
    pub do_not_track: bool,
    /// Running under CI (`CI` is set).
    pub ci: bool,
    /// The `--no-analytics` flag was passed.
    pub cli_opt_out: bool,
    /// A `PostHog` project key is available.
    pub has_api_key: bool,
}

/// Why analytics is not sending events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledReason {
    /// `FRONTAL_CODE_ANALYTICS_DISABLED` is set.
    EnvOptOut,
    /// `DO_NOT_TRACK` is set.
    DoNotTrack,
    /// Running under CI.
    Ci,
    /// `--no-analytics` was passed.
    FlagOptOut,
    /// `analytics.enabled` is `false` in user settings.
    SettingsOptOut,
    /// The user never opted in.
    NotEnabled,
    /// No project key is configured.
    NoApiKey,
    /// The backend could not be created in this build.
    NoBackend,
    /// The backend failed to start at runtime.
    BackendError,
}

impl DisabledReason {
    /// A short, stable label for status output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EnvOptOut => "disabled by FRONTAL_CODE_ANALYTICS_DISABLED",
            Self::DoNotTrack => "disabled by DO_NOT_TRACK",
            Self::Ci => "disabled in CI",
            Self::FlagOptOut => "disabled by --no-analytics",
            Self::SettingsOptOut => "disabled in settings (analytics.enabled = false)",
            Self::NotEnabled => "not enabled (set analytics.enabled = true to opt in)",
            Self::NoApiKey => "no PostHog project key configured",
            Self::NoBackend => "this build has no analytics backend",
            Self::BackendError => "analytics backend failed to start",
        }
    }
}

impl Display for DisabledReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The result of consent resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// Events may be sent.
    Enabled,
    /// Events are dropped, for the given reason.
    Disabled(DisabledReason),
}

/// Applies the opt-out and opt-in rules. Opt-outs are checked first, in order,
/// so the most specific reason is reported.
#[must_use]
pub fn resolve(inputs: ConsentInputs) -> Consent {
    if inputs.env_disabled {
        return Consent::Disabled(DisabledReason::EnvOptOut);
    }
    if inputs.do_not_track {
        return Consent::Disabled(DisabledReason::DoNotTrack);
    }
    if inputs.ci {
        return Consent::Disabled(DisabledReason::Ci);
    }
    if inputs.cli_opt_out {
        return Consent::Disabled(DisabledReason::FlagOptOut);
    }
    if inputs.settings_enabled == Some(false) {
        return Consent::Disabled(DisabledReason::SettingsOptOut);
    }
    if inputs.settings_enabled != Some(true) && !inputs.env_enabled {
        return Consent::Disabled(DisabledReason::NotEnabled);
    }
    if !inputs.has_api_key {
        return Consent::Disabled(DisabledReason::NoApiKey);
    }
    Consent::Enabled
}

/// Parses an environment variable value as a boolean flag.
#[must_use]
pub fn is_truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opted_in() -> ConsentInputs {
        ConsentInputs {
            settings_enabled: Some(true),
            has_api_key: true,
            ..ConsentInputs::default()
        }
    }

    #[test]
    fn off_by_default() {
        assert_eq!(
            resolve(ConsentInputs {
                has_api_key: true,
                ..ConsentInputs::default()
            }),
            Consent::Disabled(DisabledReason::NotEnabled)
        );
    }

    #[test]
    fn settings_opt_in_enables_with_key() {
        assert_eq!(resolve(opted_in()), Consent::Enabled);
    }

    #[test]
    fn env_opt_in_enables_with_key() {
        let inputs = ConsentInputs {
            env_enabled: true,
            has_api_key: true,
            ..ConsentInputs::default()
        };
        assert_eq!(resolve(inputs), Consent::Enabled);
    }

    #[test]
    fn every_opt_out_beats_opt_in() {
        let base = opted_in();
        let cases = [
            (
                ConsentInputs {
                    env_disabled: true,
                    ..base
                },
                DisabledReason::EnvOptOut,
            ),
            (
                ConsentInputs {
                    do_not_track: true,
                    ..base
                },
                DisabledReason::DoNotTrack,
            ),
            (ConsentInputs { ci: true, ..base }, DisabledReason::Ci),
            (
                ConsentInputs {
                    cli_opt_out: true,
                    ..base
                },
                DisabledReason::FlagOptOut,
            ),
            (
                ConsentInputs {
                    settings_enabled: Some(false),
                    env_enabled: true,
                    ..base
                },
                DisabledReason::SettingsOptOut,
            ),
        ];
        for (inputs, reason) in cases {
            assert_eq!(resolve(inputs), Consent::Disabled(reason));
        }
    }

    #[test]
    fn missing_key_disables_even_when_opted_in() {
        let inputs = ConsentInputs {
            has_api_key: false,
            ..opted_in()
        };
        assert_eq!(resolve(inputs), Consent::Disabled(DisabledReason::NoApiKey));
    }

    #[test]
    fn truthy_values() {
        assert!(is_truthy(Some("1")));
        assert!(is_truthy(Some(" TRUE ")));
        assert!(!is_truthy(Some("0")));
        assert!(!is_truthy(Some("")));
        assert!(!is_truthy(None));
    }
}
