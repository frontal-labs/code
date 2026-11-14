//! Diff rendering for edits and file writes in the TUI.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::{ChangeTag, TextDiff};

/// Unchanged lines shown around each change.
const CONTEXT_LINES: usize = 2;

/// Renders the difference between two texts as unified-diff hunks: removed lines in red,
/// added lines in green, and surrounding context dimmed.
pub fn unified(old: &str, new: &str) -> Vec<Line<'static>> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();
    for group in diff.grouped_ops(CONTEXT_LINES) {
        let (old_start, old_len, new_start, new_len) = group_bounds(&group);
        lines.push(Line::from(Span::styled(
            format!(
                "@@ -{},{} +{},{} @@",
                old_start + 1,
                old_len,
                new_start + 1,
                new_len
            ),
            Style::default().fg(Color::Cyan),
        )));
        for op in &group {
            for change in diff.iter_changes(op) {
                let (sign, style) = match change.tag() {
                    ChangeTag::Delete => ("-", Style::default().fg(Color::Red)),
                    ChangeTag::Insert => ("+", Style::default().fg(Color::Green)),
                    ChangeTag::Equal => (" ", Style::default().add_modifier(Modifier::DIM)),
                };
                let text = change.value().trim_end_matches('\n').to_string();
                lines.push(Line::from(Span::styled(format!("{sign} {text}"), style)));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "no changes".to_string(),
            Style::default().add_modifier(Modifier::DIM),
        )));
    }
    lines
}

fn group_bounds(group: &[similar::DiffOp]) -> (usize, usize, usize, usize) {
    let first = group.first().map_or((0, 0, 0, 0), |op| {
        let (_, old_range, new_range) = (0, op.old_range(), op.new_range());
        (old_range.start, 0, new_range.start, 0)
    });
    let last = group
        .last()
        .map_or((0, 0), |op| (op.old_range().end, op.new_range().end));
    let old_len = last.0.saturating_sub(first.0);
    let new_len = last.1.saturating_sub(first.2);
    (first.0, old_len, first.2, new_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn marks_removed_and_added_lines() {
        let rendered = text(&unified("a\nold\nc\n", "a\nnew\nc\n"));
        assert!(rendered.iter().any(|line| line == "- old"));
        assert!(rendered.iter().any(|line| line == "+ new"));
        assert!(rendered.iter().any(|line| line.starts_with("@@")));
        assert!(rendered.iter().any(|line| line == "  a"));
    }

    #[test]
    fn identical_texts_report_no_changes() {
        assert_eq!(
            text(&unified("same\n", "same\n")),
            vec!["no changes".to_string()]
        );
    }
}
