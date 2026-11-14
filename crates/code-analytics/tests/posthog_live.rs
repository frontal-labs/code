//! Sends one event to a real `PostHog` project. Ignored by default, so normal
//! test runs never leave the machine. Run it deliberately with a *test* project key:
//!
//! ```sh
//! FRONTAL_CODE_POSTHOG_TEST_KEY=phc_... \
//!   cargo test -p code-analytics --test posthog_live -- --ignored
//! ```
//!
//! Optionally set `FRONTAL_CODE_POSTHOG_TEST_HOST` for EU or self-hosted projects.
//! Check the `PostHog` live events view for a `session_started` event with
//! `app_version` set to `live-smoke`.

#![cfg(feature = "posthog")]

use code_analytics::{
    AnalyticsBuilder, AnalyticsClient, Consent, Event, SessionMode, DEFAULT_HOST,
};

#[test]
#[ignore = "needs FRONTAL_CODE_POSTHOG_TEST_KEY and a real PostHog project"]
fn sends_one_event_to_a_real_project() {
    let key = std::env::var("FRONTAL_CODE_POSTHOG_TEST_KEY")
        .expect("set FRONTAL_CODE_POSTHOG_TEST_KEY to a test project key");
    let host = std::env::var("FRONTAL_CODE_POSTHOG_TEST_HOST")
        .unwrap_or_else(|_| DEFAULT_HOST.to_string());

    let client: AnalyticsClient = AnalyticsBuilder::default()
        .api_key(Some(key))
        .host(host)
        .settings_enabled(Some(true))
        .app_version("live-smoke")
        .build();
    assert_eq!(client.consent(), Consent::Enabled, "client must be enabled");

    client.capture(&Event::SessionStarted {
        mode: SessionMode::Prompt,
    });
    client.shutdown();
}
