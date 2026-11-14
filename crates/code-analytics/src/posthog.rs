//! `PostHog` backend.
//!
//! Wraps the `posthog-rs` blocking client. That client already batches events,
//! sends them on a background thread, retries with backoff, and bounds
//! shutdown. This module only configures it and adds the allowed properties.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use posthog_rs::{ClientOptionsBuilder, PostHogError};
use uuid::Uuid;

use crate::config::Environment;
use crate::error::AnalyticsError;
use crate::event::{Event, Prop};
use crate::identity;
use crate::scrub;
use crate::sink::AnalyticsSink;

/// Settings for the `PostHog` transport.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Write-only project API key.
    pub api_key: String,
    /// Ingestion host, for example `https://us.i.posthog.com`.
    pub host: String,
    /// Environment label attached to every event.
    pub environment: Environment,
    /// Events per request, and the buffer size that triggers a flush.
    pub batch_size: usize,
    /// Maximum time an event waits before it is sent.
    pub flush_interval: Duration,
    /// Directory holding the anonymous install id. `None` uses a per-process id.
    pub config_home: Option<PathBuf>,
    /// Application version attached to every event.
    pub app_version: String,
}

/// Buffered events beyond this are dropped rather than growing without bound.
const MAX_QUEUE_SIZE: usize = 1_000;
const REQUEST_TIMEOUT_SECS: u64 = 5;
const MAX_CAPTURE_ATTEMPTS: u32 = 3;
const RETRY_INITIAL_BACKOFF_MS: u64 = 200;
const RETRY_MAX_BACKOFF_MS: u64 = 5_000;
/// Upper bound on time spent draining at exit. Keeps CLI shutdown short even
/// when the host is unreachable.
const SHUTDOWN_TIMEOUT_MS: u64 = 2_000;
/// Longest wait for background retries before shutdown. With the retry schedule
/// above, unreachable hosts are given up on in well under a second.
const RETRY_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Sends events to `PostHog`.
pub struct PostHogSink {
    client: posthog_rs::Client,
    distinct_id: String,
    app_version: String,
    environment: Environment,
}

impl PostHogSink {
    /// Creates the transport and starts its background worker.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::Backend`] if the client options are rejected.
    pub fn new(config: &BackendConfig) -> Result<Self, AnalyticsError> {
        let distinct_id = config
            .config_home
            .as_deref()
            .and_then(|dir| identity::load_or_create(dir).ok())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let debug = crate::consent::is_truthy(
            std::env::var("FRONTAL_CODE_ANALYTICS_DEBUG")
                .ok()
                .as_deref(),
        );

        let mut builder = ClientOptionsBuilder::default();
        builder
            .api_key(config.api_key.clone())
            .host(config.host.clone())
            .is_server(false)
            .disable_geoip(true)
            .flush_at(config.batch_size.max(1))
            .max_batch_size(config.batch_size.max(1))
            .flush_interval_ms(duration_millis(config.flush_interval))
            .max_queue_size(MAX_QUEUE_SIZE)
            .request_timeout_seconds(REQUEST_TIMEOUT_SECS)
            .shutdown_timeout_ms(SHUTDOWN_TIMEOUT_MS)
            .max_capture_attempts(MAX_CAPTURE_ATTEMPTS)
            .retry_initial_backoff_ms(RETRY_INITIAL_BACKOFF_MS)
            .retry_max_backoff_ms(RETRY_MAX_BACKOFF_MS)
            .before_send(scrub::sanitize)
            .on_error(move |failure: &PostHogError<'_>| report_failure(debug, failure));

        let options = builder
            .build()
            .map_err(|error| AnalyticsError::Backend(error.to_string()))?;

        Ok(Self {
            client: posthog_rs::client(options),
            distinct_id,
            app_version: config.app_version.clone(),
            environment: config.environment,
        })
    }
}

impl AnalyticsSink for PostHogSink {
    fn capture(&self, event: &Event) {
        let mut ph_event = posthog_rs::Event::new(event.name(), self.distinct_id.as_str());
        for (key, value) in event.properties() {
            let _ = match value {
                Prop::Label(label) => ph_event.insert_prop(key, label),
                Prop::Int(number) => ph_event.insert_prop(key, number),
            };
        }
        let _ = ph_event.insert_prop("app_version", self.app_version.as_str());
        let _ = ph_event.insert_prop("environment", self.environment.as_str());
        let _ = ph_event.insert_prop("$process_person_profile", false);
        self.client.capture(ph_event);
    }

    fn flush(&self) {
        self.client.flush();
    }

    fn shutdown(&self) {
        // `posthog-rs` makes one final attempt per held retry at shutdown and does
        // not wait out the backoff. Let the worker finish its own retries first.
        self.client.flush();
        wait_for_pending(&self.client, RETRY_DRAIN_TIMEOUT);
        self.client.shutdown();
    }
}

/// Polls until the client has no undelivered events, or `timeout` passes.
fn wait_for_pending(client: &posthog_rs::Client, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while client.pending_events() > 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Reports delivery failures on stderr, and only when debugging is enabled.
/// The message never includes the event payload or the response body.
fn report_failure(debug: bool, _failure: &PostHogError<'_>) {
    if debug {
        eprintln!("analytics: delivery failed");
    }
}
