//! Privacy-first product analytics for `frontal-code`.
//!
//! Callers record a typed [`Event`] on a shared [`AnalyticsClient`]. Events are
//! sent to `PostHog` in the background. Recording never blocks, never returns an
//! error, and never panics into the caller.
//!
//! Analytics is **off by default**. It turns on only when the user opts in
//! (`analytics.enabled = true` in the user settings, or
//! `FRONTAL_CODE_ANALYTICS_ENABLED=1`) and no opt-out signal is present. The
//! event catalogue and privacy rules are in `docs/ANALYTICS.md`.
//!
//! ```
//! use code_analytics::{AnalyticsClient, Event, MemorySink, SessionMode};
//! use std::sync::Arc;
//!
//! // Tests and alternative backends plug in through `AnalyticsSink`.
//! let sink = Arc::new(MemorySink::default());
//! let client = AnalyticsClient::with_sink(sink.clone());
//! client.capture(&Event::SessionStarted { mode: SessionMode::Prompt });
//! client.shutdown();
//! assert_eq!(sink.events().len(), 1);
//! ```

mod bridge;
mod client;
mod config;
mod consent;
mod error;
mod event;
mod identity;
#[cfg(feature = "posthog")]
mod posthog;
#[cfg(feature = "posthog")]
mod scrub;
mod sink;

pub use bridge::AnalyticsTelemetrySink;
pub use client::AnalyticsClient;
pub use config::{
    AnalyticsBuilder, Environment, DEFAULT_BATCH_SIZE, DEFAULT_FLUSH_INTERVAL, DEFAULT_HOST,
};
pub use consent::{is_truthy, Consent, ConsentInputs, DisabledReason};
pub use error::AnalyticsError;
pub use event::{
    duration_ms, duration_secs, CommandKind, ErrorClass, Event, ExitKind, Outcome, Prop,
    ProviderKind, SessionMode, ToolKind, CATALOG,
};
pub use identity::{
    load_or_create as load_or_create_install_id, reset as reset_install_id, ID_FILE_NAME,
};
#[cfg(feature = "posthog")]
pub use posthog::{BackendConfig, PostHogSink};
pub use sink::{AnalyticsSink, MemorySink, NoopSink};
