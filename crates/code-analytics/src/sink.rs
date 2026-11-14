//! Backend abstraction.
//!
//! Implement [`AnalyticsSink`] to send events somewhere other than `PostHog`,
//! such as a file or an in-process test double.

use std::sync::Mutex;

use crate::event::Event;

/// A destination for analytics events.
///
/// Implementations must not panic on the hot path. [`AnalyticsClient`](crate::AnalyticsClient)
/// catches panics as a last line of defence, but a well-behaved sink never needs it.
pub trait AnalyticsSink: Send + Sync {
    /// Accepts one event. Should return quickly and never block on the network.
    fn capture(&self, event: &Event);

    /// Sends anything buffered. Called on demand, not required to be instant.
    fn flush(&self) {}

    /// Flushes and releases resources. Must be idempotent: the client may call
    /// it more than once.
    fn shutdown(&self) {}
}

/// Discards every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopSink;

impl AnalyticsSink for NoopSink {
    fn capture(&self, _event: &Event) {}
}

/// Keeps events in memory, for tests and debugging.
#[derive(Debug, Default)]
pub struct MemorySink {
    events: Mutex<Vec<Event>>,
}

impl MemorySink {
    /// A snapshot of every event captured so far, in order.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl AnalyticsSink for MemorySink {
    fn capture(&self, event: &Event) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.clone());
    }
}
