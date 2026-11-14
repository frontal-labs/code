//! The conversation as structured entries, and how each entry becomes styled lines.
//!
//! Entries stay structured (not pre-rendered) so tool calls can be collapsed or expanded,
//! selected with the keyboard, and re-rendered for the current terminal width. Assistant
//! replies are rendered as markdown; tool headlines reuse the REPL's wording.

use std::time::Duration;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::{ansi, diff, highlight, markdown};
use crate::render::strip_emoji;

const MAX_ENTRIES: usize = 2_000;
const MAX_EXPANDED_LINES: usize = 200;
const TRACE_COLOR: Color = Color::Indexed(177);
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Where a tool call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
}

/// One tool call, from its start to its result.
#[derive(Debug, Clone)]
pub struct ToolEntry {
    pub name: String,
    pub input: String,
    pub state: ToolState,
    pub output: Option<String>,
    pub duration: Option<Duration>,
    pub expanded: bool,
}

/// One item in the conversation.
#[derive(Debug, Clone)]
pub enum Entry {
    Banner(String),
    User(String),
    Assistant(String),
    Tool(ToolEntry),
    /// Pre-styled text from the REPL's formatters (notices, summaries, command output).
    Notice(String),
    /// An observability line: timing, usage, permission decisions, and errors.
    Trace(String),
}

/// The conversation, plus the assistant text still streaming in.
#[derive(Debug, Default)]
pub struct Transcript {
    entries: Vec<Entry>,
    streaming: String,
}

impl Transcript {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn streaming(&self) -> &str {
        &self.streaming
    }

    pub fn push(&mut self, entry: Entry) {
        self.end_stream();
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(0..excess);
        }
    }

    /// Adds streamed assistant text. It is shown raw until the message ends.
    pub fn push_text(&mut self, text: &str) {
        self.streaming.push_str(&strip_emoji(text));
    }

    /// Ends the streaming message, turning it into a markdown entry.
    pub fn end_stream(&mut self) {
        if self.streaming.trim().is_empty() {
            self.streaming.clear();
            return;
        }
        let text = std::mem::take(&mut self.streaming);
        self.entries.push(Entry::Assistant(text));
    }

    /// Starts a tool call entry.
    pub fn start_tool(&mut self, name: &str, input: &str) {
        self.push(Entry::Tool(ToolEntry {
            name: name.to_string(),
            input: input.to_string(),
            state: ToolState::Running,
            output: None,
            duration: None,
            expanded: false,
        }));
    }

    /// Records the result of the most recent running call with this name.
    pub fn finish_tool(&mut self, name: &str, output: &str, is_error: bool, duration: Duration) {
        self.end_stream();
        let state = if is_error {
            ToolState::Failed
        } else {
            ToolState::Done
        };
        let target = self.entries.iter_mut().rev().find_map(|entry| match entry {
            Entry::Tool(tool) if tool.name == name && tool.state == ToolState::Running => {
                Some(tool)
            }
            _ => None,
        });
        match target {
            Some(tool) => {
                tool.state = state;
                tool.output = Some(output.to_string());
                tool.duration = Some(duration);
            }
            None => self.push(Entry::Tool(ToolEntry {
                name: name.to_string(),
                input: String::new(),
                state,
                output: Some(output.to_string()),
                duration: Some(duration),
                expanded: false,
            })),
        }
    }

    pub fn toggle_expanded(&mut self, index: usize) {
        if let Some(Entry::Tool(tool)) = self.entries.get_mut(index) {
            tool.expanded = !tool.expanded;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.streaming.clear();
    }
}

/// Wraps styled lines to `width` columns, breaking at spaces where possible.
pub fn wrap(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for line in lines {
        if line.width() <= width {
            wrapped.push(line);
            continue;
        }
        let chars: Vec<(char, Style)> = line
            .spans
            .iter()
            .flat_map(|span| span.content.chars().map(move |c| (c, span.style)))
            .collect();
        let mut start = 0;
        while start < chars.len() {
            let mut end = (start + width).min(chars.len());
            if end < chars.len() {
                if let Some(space) = (start..end).rev().find(|&index| chars[index].0 == ' ') {
                    if space > start {
                        end = space + 1;
                    }
                }
            }
            wrapped.push(rebuild(&chars[start..end]));
            start = end;
        }
    }
    wrapped
}

fn rebuild(chars: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (character, style) in chars {
        match spans.last_mut() {
            Some(last) if last.style == *style => last.content.to_mut().push(*character),
            _ => spans.push(Span::styled(character.to_string(), *style)),
        }
    }
    Line::from(spans)
}

/// Renders one entry to styled lines (before wrapping).
pub fn render_entry(entry: &Entry, frame: usize) -> Vec<Line<'static>> {
    match entry {
        Entry::Banner(text) | Entry::Notice(text) => ansi::lines(text),
        Entry::User(text) => text
            .split('\n')
            .enumerate()
            .map(|(index, paragraph)| {
                let prefix = if index == 0 { "❯ " } else { "  " };
                Line::from(vec![
                    Span::styled(prefix, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(paragraph.to_string()),
                ])
            })
            .collect(),
        Entry::Assistant(text) => markdown::render(text),
        Entry::Trace(text) => vec![Line::from(Span::styled(
            format!("  ◦ {text}"),
            Style::default().fg(TRACE_COLOR),
        ))],
        Entry::Tool(tool) => render_tool(tool, frame),
    }
}

fn render_tool(tool: &ToolEntry, frame: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let headline = ansi::lines(&crate::format_tool_call_start(&tool.name, &tool.input));
    let mut headline_spans = headline
        .into_iter()
        .next()
        .map(|line| line.spans)
        .unwrap_or_default();
    let mut first = Vec::new();
    if tool.state == ToolState::Running {
        let glyph = SPINNER_FRAMES[frame % SPINNER_FRAMES.len()];
        first.push(Span::styled(
            format!("{glyph} "),
            Style::default().fg(Color::Indexed(33)),
        ));
    }
    first.append(&mut headline_spans);
    if let Some(duration) = tool.duration {
        first.push(Span::styled(
            format!("  {}", format_duration(duration)),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    match tool.state {
        ToolState::Running => {}
        ToolState::Done => first.push(Span::styled(" ✓", Style::default().fg(Color::Green))),
        ToolState::Failed => first.push(Span::styled(" ✗", Style::default().fg(Color::Red))),
    }
    lines.push(Line::from(first));

    let Some(output) = &tool.output else {
        return lines;
    };
    if !tool.expanded {
        let body = expanded_body(tool, output);
        let preview = body
            .first()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .unwrap_or_default();
        if !preview.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  {}", truncate(&preview, 120)),
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
        if body.len() > 1 {
            lines.push(Line::from(Span::styled(
                "  ▸ Enter to expand".to_string(),
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
        return lines;
    }

    lines.push(Line::from(Span::styled(
        "  ▾ result".to_string(),
        Style::default().add_modifier(Modifier::DIM),
    )));
    let body = expanded_body(tool, output);
    let total = body.len();
    for line in body.into_iter().take(MAX_EXPANDED_LINES) {
        let mut spans = vec![Span::raw("    ")];
        spans.extend(line.spans);
        lines.push(Line::from(spans));
    }
    if total > MAX_EXPANDED_LINES {
        lines.push(Line::from(Span::styled(
            format!("    … {} more lines", total - MAX_EXPANDED_LINES),
            Style::default().add_modifier(Modifier::DIM),
        )));
    }
    lines
}

/// The expanded body for a tool: a diff for edits and writes, highlighted contents for
/// reads, stdout and stderr for shell commands, and plain text otherwise.
fn expanded_body(tool: &ToolEntry, output: &str) -> Vec<Line<'static>> {
    let input: serde_json::Value = serde_json::from_str(&tool.input).unwrap_or_default();
    let string_field = |names: &[&str]| -> String {
        names
            .iter()
            .find_map(|name| input.get(*name).and_then(serde_json::Value::as_str))
            .unwrap_or_default()
            .to_string()
    };
    match tool.name.as_str() {
        "edit_file" | "Edit" => {
            let old = string_field(&["old_string", "oldString"]);
            let new = string_field(&["new_string", "newString"]);
            diff::unified(&old, &new)
        }
        "write_file" | "Write" => diff::unified("", &string_field(&["content"])),
        "read_file" | "Read" => {
            highlight::file_contents(output, &string_field(&["file_path", "filePath", "path"]))
        }
        "bash" | "Bash" => {
            let parsed: serde_json::Value = serde_json::from_str(output).unwrap_or_default();
            let stdout = parsed
                .get("stdout")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(output);
            let stderr = parsed
                .get("stderr")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let mut lines: Vec<Line<'static>> = stdout
                .lines()
                .map(|line| Line::from(line.to_string()))
                .collect();
            lines.extend(stderr.lines().map(|line| {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Red),
                ))
            }));
            lines
        }
        _ => output
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect(),
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Formats an elapsed time for display: "120ms", "3.4s", or "2m05s".
pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        format!("{millis}ms")
    } else if millis < 60_000 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        format!("{}m{:02}s", millis / 60_000, (millis / 1_000) % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(lines: &[Line<'static>]) -> Vec<String> {
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
    fn streamed_text_becomes_an_assistant_entry() {
        let mut transcript = Transcript::default();
        transcript.push_text("hi 👋 there");
        transcript.end_stream();
        assert!(matches!(&transcript.entries()[0], Entry::Assistant(text) if text == "hi  there"));
    }

    #[test]
    fn tool_call_completes_with_its_result() {
        let mut transcript = Transcript::default();
        transcript.start_tool("bash", r#"{"command":"ls"}"#);
        transcript.finish_tool(
            "bash",
            r#"{"stdout":"a.txt"}"#,
            false,
            Duration::from_millis(12),
        );
        let Entry::Tool(tool) = &transcript.entries()[0] else {
            panic!("expected a tool entry");
        };
        assert_eq!(tool.state, ToolState::Done);
        assert_eq!(tool.duration, Some(Duration::from_millis(12)));
    }

    #[test]
    fn collapsed_tool_shows_a_preview_and_expand_hint() {
        let mut transcript = Transcript::default();
        transcript.start_tool("bash", r#"{"command":"ls"}"#);
        transcript.finish_tool(
            "bash",
            r#"{"stdout":"a.txt\nb.txt"}"#,
            false,
            Duration::from_millis(3),
        );
        let rendered = joined(&render_entry(&transcript.entries()[0], 0));
        assert!(rendered.iter().any(|line| line.contains("Enter to expand")));
        transcript.toggle_expanded(0);
        let expanded = joined(&render_entry(&transcript.entries()[0], 0));
        assert!(expanded.iter().any(|line| line.contains("b.txt")));
    }

    #[test]
    fn edit_expands_to_a_diff() {
        let mut transcript = Transcript::default();
        transcript.start_tool(
            "edit_file",
            r#"{"path":"a.rs","old_string":"x","new_string":"y"}"#,
        );
        transcript.finish_tool("edit_file", "ok", false, Duration::from_millis(1));
        transcript.toggle_expanded(0);
        let rendered = joined(&render_entry(&transcript.entries()[0], 0));
        assert!(rendered.iter().any(|line| line.contains("- x")));
        assert!(rendered.iter().any(|line| line.contains("+ y")));
    }

    #[test]
    fn wrap_breaks_long_lines_at_spaces() {
        let lines = wrap(vec![Line::from("one two three four")], 9);
        let rendered = joined(&lines);
        assert!(rendered.iter().all(|line| line.chars().count() <= 9));
        assert_eq!(rendered.concat().replace(' ', ""), "onetwothreefour");
    }

    #[test]
    fn durations_are_compact() {
        assert_eq!(format_duration(Duration::from_millis(120)), "120ms");
        assert_eq!(format_duration(Duration::from_millis(3_400)), "3.4s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2m05s");
    }
}
