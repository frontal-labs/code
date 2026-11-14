//! Keeps `docs/ANALYTICS.md` in sync with the event catalogue.

use code_analytics::CATALOG;

const DOC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/ANALYTICS.md"
));

#[test]
fn every_event_has_a_row_in_the_docs() {
    for (event, _) in CATALOG {
        let marker = format!("| `{event}` |");
        assert!(
            DOC.lines().any(|line| line.starts_with(&marker)),
            "event `{event}` is missing from docs/ANALYTICS.md"
        );
    }
}

#[test]
fn every_property_is_named_in_the_docs() {
    for (event, keys) in CATALOG {
        for key in *keys {
            assert!(
                DOC.contains(&format!("`{key}`")),
                "property `{key}` of `{event}` is missing from docs/ANALYTICS.md"
            );
        }
    }
}
