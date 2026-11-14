//! Adapter from the existing `code-telemetry` stream to typed analytics events.
//!
//! The runtime already emits detailed local telemetry, including raw prompts
//! (`turn_started.user_input`), tool names and error messages. This adapter
//! forwards that stream unchanged to the local sink, and separately maps only
//! the records it recognises onto [`Event`]s. Attribute values are read for
//! counts, flags and timings only, and nothing else from them is kept.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use code_telemetry::{SessionTraceRecord, TelemetryEvent, TelemetrySink};
use serde_json::{Map, Value};

use crate::client::AnalyticsClient;
use crate::event::{duration_ms, ErrorClass, Event, Outcome, ProviderKind, ToolKind};

/// Forwards telemetry to a local sink and to analytics.
pub struct AnalyticsTelemetrySink {
    inner: Option<Arc<dyn TelemetrySink>>,
    analytics: AnalyticsClient,
    provider: ProviderKind,
    timings: Mutex<Timings>,
}

#[derive(Default)]
struct Timings {
    turn_started_ms: Option<u64>,
    tool_started_ms: HashMap<String, u64>,
}

impl AnalyticsTelemetrySink {
    /// `inner` keeps receiving every record unchanged. `provider` labels the
    /// turn events, since the runtime does not report it.
    #[must_use]
    pub fn new(
        inner: Option<Arc<dyn TelemetrySink>>,
        analytics: AnalyticsClient,
        provider: ProviderKind,
    ) -> Self {
        Self {
            inner,
            analytics,
            provider,
            timings: Mutex::new(Timings::default()),
        }
    }

    fn map(&self, event: &TelemetryEvent) -> Option<Event> {
        match event {
            TelemetryEvent::HttpRequestFailed {
                retryable: false, ..
            } => Some(Event::ErrorOccurred {
                class: ErrorClass::Provider,
            }),
            TelemetryEvent::SessionTrace(record) => self.map_trace(record),
            _ => None,
        }
    }

    fn map_trace(&self, record: &SessionTraceRecord) -> Option<Event> {
        let mut timings = self.timings.lock().unwrap_or_else(PoisonError::into_inner);
        let attrs = &record.attributes;
        match record.name.as_str() {
            "turn_started" => {
                timings.turn_started_ms = Some(record.timestamp_ms);
                None
            }
            "turn_completed" => {
                self.analytics.note_turn();
                let started = timings.turn_started_ms.take();
                timings.tool_started_ms.clear();
                Some(Event::TurnCompleted {
                    provider: self.provider,
                    iterations: count(attrs, "iterations"),
                    tool_calls: count(attrs, "tool_results"),
                    duration_ms: elapsed(started, record.timestamp_ms),
                })
            }
            "turn_failed" => {
                // A failed turn can leave tools unfinished. Forget their start times
                // so the map cannot grow across turns.
                timings.tool_started_ms.clear();
                timings.turn_started_ms = None;
                Some(Event::ErrorOccurred {
                    class: ErrorClass::Turn,
                })
            }
            "tool_execution_started" => {
                if let Some(name) = text(attrs, "tool_name") {
                    timings
                        .tool_started_ms
                        .insert(name.to_string(), record.timestamp_ms);
                }
                None
            }
            "tool_execution_finished" => {
                let name = text(attrs, "tool_name").unwrap_or_default();
                let started = timings.tool_started_ms.remove(name);
                let is_error = attrs
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Some(Event::ToolUsed {
                    tool: ToolKind::from_tool_name(name),
                    outcome: Outcome::from_success(!is_error),
                    duration_ms: elapsed(started, record.timestamp_ms),
                })
            }
            _ => None,
        }
    }
}

impl TelemetrySink for AnalyticsTelemetrySink {
    fn record(&self, event: TelemetryEvent) {
        if let Some(mapped) = self.map(&event) {
            self.analytics.capture(&mapped);
        }
        if let Some(inner) = &self.inner {
            inner.record(event);
        }
    }
}

fn text<'a>(attrs: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    attrs.get(key).and_then(Value::as_str)
}

fn count(attrs: &Map<String, Value>, key: &str) -> u16 {
    attrs
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(u16::MAX)
}

fn elapsed(started_ms: Option<u64>, finished_ms: u64) -> u32 {
    let millis = finished_ms.saturating_sub(started_ms.unwrap_or(finished_ms));
    duration_ms(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::MemorySink;
    use code_telemetry::MemoryTelemetrySink;

    fn trace(name: &str, timestamp_ms: u64, attributes: Map<String, Value>) -> TelemetryEvent {
        TelemetryEvent::SessionTrace(SessionTraceRecord {
            session_id: "session".to_string(),
            sequence: 0,
            name: name.to_string(),
            timestamp_ms,
            attributes,
        })
    }

    #[test]
    fn maps_turns_and_tools_without_forwarding_raw_text_to_analytics() {
        let analytics_sink = Arc::new(MemorySink::default());
        let local_sink = Arc::new(MemoryTelemetrySink::default());
        let bridge = AnalyticsTelemetrySink::new(
            Some(local_sink.clone()),
            AnalyticsClient::with_sink(analytics_sink.clone()),
            ProviderKind::Anthropic,
        );

        let mut started = Map::new();
        started.insert(
            "user_input".to_string(),
            Value::from("secret prompt about ~/private"),
        );
        bridge.record(trace("turn_started", 1_000, started));

        let mut tool_started = Map::new();
        tool_started.insert("tool_name".to_string(), Value::from("bash"));
        bridge.record(trace("tool_execution_started", 1_100, tool_started));

        let mut tool_finished = Map::new();
        tool_finished.insert("tool_name".to_string(), Value::from("bash"));
        tool_finished.insert("is_error".to_string(), Value::Bool(false));
        bridge.record(trace("tool_execution_finished", 1_400, tool_finished));

        let mut completed = Map::new();
        completed.insert("iterations".to_string(), Value::from(2_u64));
        completed.insert("tool_results".to_string(), Value::from(1_u64));
        bridge.record(trace("turn_completed", 2_000, completed));

        assert_eq!(
            analytics_sink.events(),
            vec![
                Event::ToolUsed {
                    tool: ToolKind::Bash,
                    outcome: Outcome::Success,
                    duration_ms: 300,
                },
                Event::TurnCompleted {
                    provider: ProviderKind::Anthropic,
                    iterations: 2,
                    tool_calls: 1,
                    duration_ms: 1_000,
                },
            ]
        );

        // The local sink still receives every record, including the raw prompt.
        assert_eq!(local_sink.events().len(), 4);
    }

    #[test]
    fn unknown_tools_and_non_retryable_failures_are_classified_not_copied() {
        let analytics_sink = Arc::new(MemorySink::default());
        let bridge = AnalyticsTelemetrySink::new(
            None,
            AnalyticsClient::with_sink(analytics_sink.clone()),
            ProviderKind::Frontal,
        );

        let mut tool_finished = Map::new();
        tool_finished.insert(
            "tool_name".to_string(),
            Value::from("mcp__github__create_issue"),
        );
        tool_finished.insert("is_error".to_string(), Value::Bool(true));
        bridge.record(trace("tool_execution_finished", 10, tool_finished));

        bridge.record(TelemetryEvent::HttpRequestFailed {
            session_id: "session".to_string(),
            attempt: 1,
            method: "POST".to_string(),
            path: "/v1/messages".to_string(),
            error: "token sk-ant-should-never-leave".to_string(),
            retryable: false,
            attributes: Map::new(),
        });

        assert_eq!(
            analytics_sink.events(),
            vec![
                Event::ToolUsed {
                    tool: ToolKind::Other,
                    outcome: Outcome::Failure,
                    duration_ms: 0,
                },
                Event::ErrorOccurred {
                    class: ErrorClass::Provider,
                },
            ]
        );
    }
}
