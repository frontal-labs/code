//! Typed analytics events.
//!
//! Every property is either a closed enum (serialized as a fixed label) or an
//! integer. Because no variant holds a string the caller controls, prompts,
//! file paths, tool arguments, error messages and secrets cannot be expressed.

use std::time::Duration;

/// Declares a closed set of labels for one property.
macro_rules! labels {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $(
                #[doc = concat!("Reported as `", $label, "`.")]
                $variant,
            )+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The stable label sent to the backend.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),+
                }
            }
        }
    };
}

labels! {
    /// How an interactive or scripted session was started.
    SessionMode {
        Repl => "repl",
        Prompt => "prompt",
        Resume => "resume",
        Hosted => "hosted",
    }
}

labels! {
    /// How a session ended.
    ExitKind {
        Ok => "ok",
        Error => "error",
        Interrupted => "interrupted",
    }
}

labels! {
    /// Top-level CLI command that was run.
    CommandKind {
        Prompt => "prompt",
        Repl => "repl",
        Doctor => "doctor",
        Init => "init",
        Status => "status",
        Config => "config",
        Login => "login",
        Logout => "logout",
        Plugins => "plugins",
        Mcp => "mcp",
        Skills => "skills",
        Agents => "agents",
        Hosted => "hosted",
        Telemetry => "telemetry",
        Other => "other",
    }
}

labels! {
    /// Whether an operation succeeded.
    Outcome {
        Success => "success",
        Failure => "failure",
    }
}

labels! {
    /// Model provider family. Model names are never reported.
    ProviderKind {
        Anthropic => "anthropic",
        OpenAi => "openai",
        Xai => "xai",
        Frontal => "frontal",
        Other => "other",
    }
}

labels! {
    /// Built-in tools. Plugin, MCP and unknown tool names collapse to `Other`.
    ToolKind {
        Bash => "bash",
        ReadFile => "read_file",
        WriteFile => "write_file",
        EditFile => "edit_file",
        GlobSearch => "glob_search",
        GrepSearch => "grep_search",
        WebFetch => "web_fetch",
        WebSearch => "web_search",
        Other => "other",
    }
}

labels! {
    /// Coarse failure category. Error text is never reported.
    ErrorClass {
        Provider => "provider",
        Turn => "turn",
    }
}

impl ToolKind {
    /// Maps a runtime tool name onto a built-in kind.
    #[must_use]
    pub fn from_tool_name(name: &str) -> Self {
        match name {
            "bash" => Self::Bash,
            "read_file" => Self::ReadFile,
            "write_file" => Self::WriteFile,
            "edit_file" => Self::EditFile,
            "glob_search" => Self::GlobSearch,
            "grep_search" => Self::GrepSearch,
            "WebFetch" => Self::WebFetch,
            "WebSearch" => Self::WebSearch,
            _ => Self::Other,
        }
    }
}

impl Outcome {
    /// `Success` when `ok` is true, otherwise `Failure`.
    #[must_use]
    pub const fn from_success(ok: bool) -> Self {
        if ok {
            Self::Success
        } else {
            Self::Failure
        }
    }
}

/// A single analytics event. Construct it from this enum only.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A session began.
    SessionStarted { mode: SessionMode },
    /// A session ended.
    SessionEnded {
        duration_secs: u32,
        turns: u32,
        exit: ExitKind,
    },
    /// A top-level command finished.
    CommandRun {
        command: CommandKind,
        outcome: Outcome,
        duration_ms: u32,
    },
    /// An agent turn finished successfully.
    TurnCompleted {
        provider: ProviderKind,
        iterations: u16,
        tool_calls: u16,
        duration_ms: u32,
    },
    /// A built-in tool finished running.
    ToolUsed {
        tool: ToolKind,
        outcome: Outcome,
        duration_ms: u32,
    },
    /// Something failed. Only the category is kept.
    ErrorOccurred { class: ErrorClass },
    /// A provider login attempt finished.
    LoginCompleted {
        provider: ProviderKind,
        outcome: Outcome,
    },
}

/// A property value. Strings are always `'static` labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prop {
    /// A closed label.
    Label(&'static str),
    /// A non-negative count or duration.
    Int(u64),
}

impl Event {
    /// The event name sent to the backend.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::SessionStarted { .. } => "session_started",
            Self::SessionEnded { .. } => "session_ended",
            Self::CommandRun { .. } => "command_run",
            Self::TurnCompleted { .. } => "turn_completed",
            Self::ToolUsed { .. } => "tool_used",
            Self::ErrorOccurred { .. } => "error_occurred",
            Self::LoginCompleted { .. } => "login_completed",
        }
    }

    /// The property keys this event sends, in the same order as [`Event::properties`].
    #[must_use]
    pub const fn property_keys(&self) -> &'static [&'static str] {
        match self {
            Self::SessionStarted { .. } => &["mode"],
            Self::SessionEnded { .. } => &["duration_secs", "turns", "exit"],
            Self::CommandRun { .. } => &["command", "outcome", "duration_ms"],
            Self::TurnCompleted { .. } => &["provider", "iterations", "tool_calls", "duration_ms"],
            Self::ToolUsed { .. } => &["tool", "outcome", "duration_ms"],
            Self::ErrorOccurred { .. } => &["class"],
            Self::LoginCompleted { .. } => &["provider", "outcome"],
        }
    }

    /// The property values for this event.
    #[must_use]
    pub fn properties(&self) -> Vec<(&'static str, Prop)> {
        let label = |s: &'static str| Prop::Label(s);
        match *self {
            Self::SessionStarted { mode } => vec![("mode", label(mode.as_str()))],
            Self::SessionEnded {
                duration_secs,
                turns,
                exit,
            } => vec![
                ("duration_secs", Prop::Int(u64::from(duration_secs))),
                ("turns", Prop::Int(u64::from(turns))),
                ("exit", label(exit.as_str())),
            ],
            Self::CommandRun {
                command,
                outcome,
                duration_ms,
            } => vec![
                ("command", label(command.as_str())),
                ("outcome", label(outcome.as_str())),
                ("duration_ms", Prop::Int(u64::from(duration_ms))),
            ],
            Self::TurnCompleted {
                provider,
                iterations,
                tool_calls,
                duration_ms,
            } => vec![
                ("provider", label(provider.as_str())),
                ("iterations", Prop::Int(u64::from(iterations))),
                ("tool_calls", Prop::Int(u64::from(tool_calls))),
                ("duration_ms", Prop::Int(u64::from(duration_ms))),
            ],
            Self::ToolUsed {
                tool,
                outcome,
                duration_ms,
            } => vec![
                ("tool", label(tool.as_str())),
                ("outcome", label(outcome.as_str())),
                ("duration_ms", Prop::Int(u64::from(duration_ms))),
            ],
            Self::ErrorOccurred { class } => vec![("class", label(class.as_str()))],
            Self::LoginCompleted { provider, outcome } => vec![
                ("provider", label(provider.as_str())),
                ("outcome", label(outcome.as_str())),
            ],
        }
    }
}

/// Converts a duration to whole milliseconds, saturating at `u32::MAX`.
#[must_use]
pub fn duration_ms(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

/// Converts a duration to whole seconds, saturating at `u32::MAX`.
#[must_use]
pub fn duration_secs(duration: Duration) -> u32 {
    u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
}

/// Every event name and its property keys. This is the source of truth for
/// `docs/ANALYTICS.md`, which a test checks against it.
pub const CATALOG: &[(&str, &[&str])] = &[
    ("session_started", &["mode"]),
    ("session_ended", &["duration_secs", "turns", "exit"]),
    ("command_run", &["command", "outcome", "duration_ms"]),
    (
        "turn_completed",
        &["provider", "iterations", "tool_calls", "duration_ms"],
    ),
    ("tool_used", &["tool", "outcome", "duration_ms"]),
    ("error_occurred", &["class"]),
    ("login_completed", &["provider", "outcome"]),
];

/// Every string label an event may carry. Used by the scrubber.
#[cfg(feature = "posthog")]
#[must_use]
pub fn allowed_labels() -> Vec<&'static str> {
    let mut labels = Vec::new();
    labels.extend(SessionMode::ALL.iter().map(|v| v.as_str()));
    labels.extend(ExitKind::ALL.iter().map(|v| v.as_str()));
    labels.extend(CommandKind::ALL.iter().map(|v| v.as_str()));
    labels.extend(Outcome::ALL.iter().map(|v| v.as_str()));
    labels.extend(ProviderKind::ALL.iter().map(|v| v.as_str()));
    labels.extend(ToolKind::ALL.iter().map(|v| v.as_str()));
    labels.extend(ErrorClass::ALL.iter().map(|v| v.as_str()));
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_keys_match_values_and_catalog() {
        let samples = [
            Event::SessionStarted {
                mode: SessionMode::Repl,
            },
            Event::SessionEnded {
                duration_secs: 3,
                turns: 2,
                exit: ExitKind::Ok,
            },
            Event::CommandRun {
                command: CommandKind::Prompt,
                outcome: Outcome::Success,
                duration_ms: 10,
            },
            Event::TurnCompleted {
                provider: ProviderKind::Anthropic,
                iterations: 2,
                tool_calls: 1,
                duration_ms: 500,
            },
            Event::ToolUsed {
                tool: ToolKind::Bash,
                outcome: Outcome::Failure,
                duration_ms: 42,
            },
            Event::ErrorOccurred {
                class: ErrorClass::Provider,
            },
            Event::LoginCompleted {
                provider: ProviderKind::OpenAi,
                outcome: Outcome::Success,
            },
        ];

        for event in samples {
            let catalog_keys = CATALOG
                .iter()
                .find(|(name, _)| *name == event.name())
                .map(|(_, keys)| *keys)
                .expect("every event must be in the catalog");
            let value_keys: Vec<&str> = event.properties().iter().map(|(k, _)| *k).collect();
            assert_eq!(value_keys, event.property_keys(), "{}", event.name());
            assert_eq!(value_keys, catalog_keys, "{}", event.name());
        }
    }

    #[test]
    fn unknown_tools_collapse_to_other() {
        assert_eq!(ToolKind::from_tool_name("bash"), ToolKind::Bash);
        assert_eq!(
            ToolKind::from_tool_name("mcp__github__create_issue"),
            ToolKind::Other
        );
    }

    #[test]
    fn durations_saturate() {
        assert_eq!(duration_ms(Duration::from_millis(1_500)), 1_500);
        assert_eq!(duration_ms(Duration::MAX), u32::MAX);
        assert_eq!(duration_secs(Duration::from_secs(90)), 90);
    }
}
