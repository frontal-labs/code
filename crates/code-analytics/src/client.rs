use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::config::AnalyticsBuilder;
use crate::consent::{Consent, DisabledReason};
use crate::event::Event;
use crate::sink::{AnalyticsSink, NoopSink};

/// The application-facing analytics handle.
///
/// Cheap to clone and safe to share across threads. Calls never return errors
/// and never panic, and [`AnalyticsClient::capture`] only enqueues the event,
/// so it cannot stall the caller on the network.
#[derive(Clone)]
pub struct AnalyticsClient {
    inner: Arc<Inner>,
}

struct Inner {
    sink: Arc<dyn AnalyticsSink>,
    consent: Consent,
    shut_down: AtomicBool,
    turns: AtomicU32,
}

impl AnalyticsClient {
    /// Starts configuring a client. See [`AnalyticsBuilder`].
    pub fn builder() -> AnalyticsBuilder {
        AnalyticsBuilder::default()
    }

    /// A client that drops every event, for the given reason.
    #[must_use]
    pub fn disabled(reason: DisabledReason) -> Self {
        Self::new(Arc::new(NoopSink), Consent::Disabled(reason))
    }

    /// A client that sends events to `sink`. Intended for tests and alternative
    /// backends; it bypasses consent resolution.
    #[must_use]
    pub fn with_sink(sink: Arc<dyn AnalyticsSink>) -> Self {
        Self::new(sink, Consent::Enabled)
    }

    fn new(sink: Arc<dyn AnalyticsSink>, consent: Consent) -> Self {
        Self {
            inner: Arc::new(Inner {
                sink,
                consent,
                shut_down: AtomicBool::new(false),
                turns: AtomicU32::new(0),
            }),
        }
    }

    /// Number of agent turns completed by this process. Counted whether or not
    /// analytics is enabled, so the session summary is always accurate.
    #[must_use]
    pub fn turns_completed(&self) -> u32 {
        self.inner.turns.load(Ordering::Relaxed)
    }

    /// Records one completed turn. Called by the telemetry bridge.
    pub(crate) fn note_turn(&self) {
        self.inner.turns.fetch_add(1, Ordering::Relaxed);
    }

    /// Why analytics is or is not enabled.
    #[must_use]
    pub fn consent(&self) -> Consent {
        self.inner.consent
    }

    /// Whether events are being sent.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner.consent == Consent::Enabled
    }

    /// Records an event. Dropped when disabled or after shutdown.
    pub fn capture(&self, event: &Event) {
        if !self.is_enabled() || self.inner.shut_down.load(Ordering::Acquire) {
            return;
        }
        let sink = &self.inner.sink;
        let _ = catch_unwind(AssertUnwindSafe(|| sink.capture(event)));
    }

    /// Asks the backend to send buffered events now.
    pub fn flush(&self) {
        if !self.is_enabled() || self.inner.shut_down.load(Ordering::Acquire) {
            return;
        }
        let sink = &self.inner.sink;
        let _ = catch_unwind(AssertUnwindSafe(|| sink.flush()));
    }

    /// Flushes and releases the backend. Safe to call more than once; only the
    /// first call reaches the sink.
    pub fn shutdown(&self) {
        if self.inner.shut_down.swap(true, Ordering::AcqRel) {
            return;
        }
        let sink = &self.inner.sink;
        let _ = catch_unwind(AssertUnwindSafe(|| sink.shutdown()));
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Safety net for exits that skip explicit shutdown. The backend bounds its own drain time.
        if !self.shut_down.swap(true, Ordering::AcqRel) {
            let sink = &self.sink;
            let _ = catch_unwind(AssertUnwindSafe(|| sink.shutdown()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::event::{ErrorClass, SessionMode};
    use crate::sink::MemorySink;

    fn started() -> Event {
        Event::SessionStarted {
            mode: SessionMode::Repl,
        }
    }

    #[test]
    fn disabled_client_drops_events() {
        let client = AnalyticsClient::disabled(DisabledReason::EnvOptOut);
        client.capture(&started());
        assert!(!client.is_enabled());
        assert_eq!(
            client.consent(),
            Consent::Disabled(DisabledReason::EnvOptOut)
        );
    }

    #[test]
    fn enabled_client_forwards_events_in_order() {
        let sink = Arc::new(MemorySink::default());
        let client = AnalyticsClient::with_sink(sink.clone());
        client.capture(&started());
        client.capture(&Event::ErrorOccurred {
            class: ErrorClass::Turn,
        });
        assert_eq!(sink.events().len(), 2);
        assert_eq!(sink.events()[0], started());
    }

    #[test]
    fn capture_after_shutdown_is_dropped() {
        let sink = Arc::new(MemorySink::default());
        let client = AnalyticsClient::with_sink(sink.clone());
        client.shutdown();
        client.capture(&started());
        assert_eq!(sink.events(), Vec::<Event>::new());
    }

    #[test]
    fn shutdown_reaches_the_sink_once() {
        struct Counting(Arc<AtomicUsize>);
        impl AnalyticsSink for Counting {
            fn capture(&self, _event: &Event) {}
            fn shutdown(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let client = AnalyticsClient::with_sink(Arc::new(Counting(calls.clone())));
        client.shutdown();
        client.shutdown();
        drop(client);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn panicking_sink_does_not_escape() {
        struct Panicking;
        impl AnalyticsSink for Panicking {
            fn capture(&self, _event: &Event) {
                panic!("sink failure");
            }
        }

        let client = AnalyticsClient::with_sink(Arc::new(Panicking));
        client.capture(&started());
        client.flush();
    }
}
