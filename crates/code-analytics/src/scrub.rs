//! Last-line filter applied to every event before it leaves the process.
//!
//! [`Event`](crate::Event) cannot express free text, so this is defence in
//! depth: it drops unknown event names, unknown property keys, and any string
//! value that is not a known label. It runs as the `PostHog` `before_send` hook.

use serde_json::Value;

use crate::config::Environment;
use crate::event::{allowed_labels, CATALOG};

/// Properties added by the `PostHog` SDK itself.
const SDK_KEYS: &[&str] = &[
    "$lib",
    "$lib_version",
    "$os",
    "$is_server",
    "$geoip_disable",
    "$process_person_profile",
];

/// Keeps only allowlisted names, keys and values. Returns `None` to drop the event.
#[must_use]
pub fn sanitize(mut event: posthog_rs::Event) -> Option<posthog_rs::Event> {
    let name = event.event_name().to_string();
    let event_keys = CATALOG
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, keys)| *keys)?;

    let labels = allowed_labels();
    let stale: Vec<String> = event
        .properties()
        .iter()
        .filter(|(key, value)| !is_allowed(key, value, event_keys, &labels))
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale {
        let _ = event.remove_prop(&key);
    }
    Some(event)
}

fn is_allowed(key: &str, value: &Value, event_keys: &[&str], labels: &[&str]) -> bool {
    if SDK_KEYS.contains(&key) {
        return true;
    }
    match key {
        "app_version" => return value.as_str().is_some_and(is_version_string),
        "environment" => {
            return value
                .as_str()
                .is_some_and(|v| Environment::ALL.iter().any(|env| env.as_str() == v));
        }
        _ => {}
    }
    if !event_keys.contains(&key) {
        return false;
    }
    match value {
        Value::Number(_) => true,
        Value::String(text) => labels.contains(&text.as_str()),
        _ => false,
    }
}

fn is_version_string(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_strings_are_restricted() {
        assert!(is_version_string("0.3.0"));
        assert!(is_version_string("0.3.0-rc.1+build"));
        assert!(!is_version_string(""));
        assert!(!is_version_string("/Users/someone/project"));
        assert!(!is_version_string(&"1".repeat(33)));
    }

    #[test]
    fn sanitize_drops_unknown_events_keys_and_free_text() {
        let mut event = posthog_rs::Event::new("session_started", "anon");
        event
            .insert_prop("mode", "repl")
            .expect("insert should succeed");
        event
            .insert_prop("prompt", "summarize my secrets")
            .expect("insert should succeed");
        event
            .insert_prop("app_version", "0.3.0")
            .expect("insert should succeed");
        event
            .insert_prop("$os", "Mac OS")
            .expect("insert should succeed");
        event
            .insert_prop("$os_version", "26.6.2")
            .expect("insert should succeed");
        let cleaned = sanitize(event).expect("known event should survive");
        assert!(cleaned.properties().contains_key("mode"));
        assert!(cleaned.properties().contains_key("app_version"));
        assert!(cleaned.properties().contains_key("$os"));
        assert!(!cleaned.properties().contains_key("prompt"));
        assert!(!cleaned.properties().contains_key("$os_version"));

        let unknown = posthog_rs::Event::new("made_up_event", "anon");
        assert!(sanitize(unknown).is_none());

        let mut path_as_mode = posthog_rs::Event::new("session_started", "anon");
        path_as_mode
            .insert_prop("mode", "/Users/someone/project")
            .expect("insert should succeed");
        let cleaned = sanitize(path_as_mode).expect("known event should survive");
        assert!(!cleaned.properties().contains_key("mode"));
    }

    #[test]
    fn free_text_values_are_rejected() {
        let event_keys: &[&str] = &["mode"];
        let labels = allowed_labels();
        assert!(is_allowed(
            "mode",
            &Value::from("repl"),
            event_keys,
            &labels
        ));
        assert!(!is_allowed(
            "mode",
            &Value::from("summarize ~/secrets.txt"),
            event_keys,
            &labels
        ));
        assert!(!is_allowed(
            "prompt",
            &Value::from("hi"),
            event_keys,
            &labels
        ));
    }
}
