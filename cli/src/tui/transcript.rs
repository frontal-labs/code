//! Conversation transcript for the full-screen TUI.
//!
//! The transcript is a bounded list of styled lines plus a small amount of
//! turn metadata (who is speaking, what tool is currently running, elapsed
//! seconds). [`Transcript::snapshot`] produces a plain-`Clone`
//! [`TranscriptSnapshot`] that the render pass can use without holding the
//! state lock.
//!
//! Styling is intentionally simple: one color per element kind, with bold for
//! the active tool. Richer markdown rendering is intentionally left to the
//! line-based REPL (it uses `render.rs`'s pulldown-cmark pipeline); the TUI
//! shows raw text and structured tool frames, which is how Claude Code itself
//! presents tool traffic.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Outcome of a tool call, as recorded in the transcript.
///
/// The runtime's [`code_runtime::ContentBlock::ToolResult`] only carries an
/// `is_error: bool`, so we collapse the three possible outcomes (completed,
/// failed, cancelled) into this enum for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Completed,
    Failed,
    Cancelled,
}

const MAX_LINES: usize = 1_200;
const MAX_TOOL_INPUT_CHARS: usize = 600;
const MAX_TOOL_RESULT_CHARS: usize = 2_000;

/// One tool call inside the transcript.
pub struct ToolEntry {
    pub name: String,
    pub input: String,
    pub result: Option<String>,
    pub status: ToolStatus,
}

/// A point-in-time, renderable copy of the transcript.
#[derive(Debug, Clone, Default)]
pub struct TranscriptSnapshot {
    pub lines: Vec<Line<'static>>,
    pub pending_text: String,
    pub active_tool: Option<String>,
    pub turn_active: bool,
    pub turn_start_secs: u64,
}

/// The live, mutating transcript.
#[derive(Debug, Default)]
pub struct Transcript {
    lines: Vec<Line<'static>>,
    pending_text: String,
    active_tool: Option<String>,
    turn_active: bool,
    turn_start_secs: u64,
}

impl Transcript {
    fn flush_pending(&mut self) {
        if self.pending_text.trim().is_empty() {
            self.pending_text.clear();
            return;
        }
        let text = std::mem::take(&mut self.pending_text);
        for paragraph in text.split('\n') {
            if paragraph.trim().is_empty() {
                self.push_line(Line::from(Span::raw(" ")));
            } else {
                self.push_line(Line::from(Span::raw(paragraph.to_string())));
            }
        }
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            let excess = self.lines.len() - MAX_LINES;
            self.lines.drain(0..excess);
        }
    }

    /// Appends streamed text to the in-progress assistant message.
    pub fn push_text(&mut self, text: &str) {
        self.pending_text.push_str(text);
    }

    /// Emits the in-progress assistant text as transcript lines.
    pub fn end_turn(&mut self) {
        self.flush_pending();
        if self.turn_active {
            self.push_line(Line::default());
        }
        self.turn_active = false;
        self.active_tool = None;
        self.turn_start_secs = 0;
    }

    /// Records the start of a tool call.
    pub fn push_tool_start(&mut self, name: &str, input: &str) {
        self.flush_pending();
        let input_preview = abbreviate(input, MAX_TOOL_INPUT_CHARS);
        self.push_line(Line::from(vec![
            Span::styled(
                "  ⚙ ".to_string(),
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                name.to_string(),
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {input_preview}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        self.active_tool = Some(name.to_string());
        if self.turn_start_secs == 0 {
            self.turn_start_secs = 1;
        }
    }

    /// Records the outcome of a tool call and opens a fresh assistant message.
    pub fn push_tool_result(&mut self, status: ToolStatus, output: &str) {
        let (glyph, style) = match status {
            ToolStatus::Completed => ("✓", Style::default().fg(Color::Reset)),
            ToolStatus::Failed | ToolStatus::Cancelled => ("✗", Style::default().fg(Color::Reset)),
        };
        let preview = abbreviate(output, MAX_TOOL_RESULT_CHARS);
        for (index, fragment) in preview.split('\n').take(8).enumerate() {
            let prefix = if index == 0 {
                format!("  {glyph} ")
            } else {
                "    ".to_string()
            };
            let styled_prefix = if index == 0 {
                Span::styled(prefix.clone(), style)
            } else {
                Span::styled(prefix.clone(), Style::default().fg(Color::DarkGray))
            };
            let fragment_style = if index == 0 {
                Style::default().fg(Color::Gray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            self.push_line(Line::from(vec![
                styled_prefix,
                Span::styled(fragment.to_string(), fragment_style),
            ]));
        }
        self.active_tool = None;
        self.turn_start_secs = 0;
    }

    /// Appends captured stdout from a slash command to the transcript.
    pub fn push_output(&mut self, output: &str) {
        for paragraph in output.split('\n').take(60) {
            if paragraph.trim().is_empty() {
                self.push_line(Line::from(Span::raw(" ")));
            } else {
                self.push_line(Line::from(vec![
                    Span::styled("  │ ".to_string(), Style::default().fg(Color::DarkGray)),
                    Span::raw(paragraph.to_string()),
                ]));
            }
        }
        self.push_line(Line::default());
    }

    /// Renders a user prompt into the transcript.
    pub fn push_user(&mut self, text: &str) {
        self.flush_pending();
        self.push_line(Line::default());
        for paragraph in text.split('\n') {
            self.push_line(Line::from(vec![
                Span::styled(
                    "  > ".to_string(),
                    Style::default()
                        .fg(Color::Reset)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(paragraph.to_string()),
            ]));
        }
        self.turn_active = true;
        self.turn_start_secs = 1;
    }

    /// Point-in-time copy for the render pass.
    pub fn snapshot(&self) -> TranscriptSnapshot {
        TranscriptSnapshot {
            lines: self.lines.clone(),
            pending_text: self.pending_text.clone(),
            active_tool: self.active_tool.clone(),
            turn_active: self.turn_active,
            turn_start_secs: self.turn_start_secs,
        }
    }
}

fn abbreviate(text: &str, max_chars: usize) -> String {
    let characters: Vec<char> = text.chars().collect();
    if characters.len() <= max_chars {
        return text.trim().to_string();
    }
    let mut out: String = characters.iter().take(max_chars).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_text_flushes_into_lines_on_turn_end() {
        let mut transcript = Transcript::default();
        transcript.push_text("hello ");
        transcript.push_text("world\n");
        transcript.end_turn();
        let snapshot = transcript.snapshot();
        assert_eq!(snapshot.lines.len(), 2);
        assert_eq!(snapshot.lines[0].spans[0].content, "hello world");
        assert_eq!(snapshot.pending_text, "");
        assert!(!snapshot.turn_active);
    }

    #[test]
    fn tool_call_frames_and_results_are_recorded() {
        let mut transcript = Transcript::default();
        transcript.push_user("run ls");
        transcript.push_tool_start("bash", "ls -la");
        transcript.push_tool_result(ToolStatus::Completed, "file.txt\n");
        let snapshot = transcript.snapshot();
        let joined: String = snapshot
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("bash"));
        assert!(joined.contains("ls -la"));
        assert!(joined.contains("✓"));
        assert!(snapshot.active_tool.is_none());
    }

    #[test]
    fn transcript_is_bounded() {
        let mut transcript = Transcript::default();
        for index in 0..MAX_LINES + 100 {
            transcript.push_output(&format!("line {index}"));
        }
        assert!(transcript.lines.len() <= MAX_LINES);
    }

    #[test]
    fn long_tool_input_is_abbreviated() {
        let mut transcript = Transcript::default();
        transcript.push_tool_start("bash", &"x".repeat(10_000));
        let joined: String = transcript
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(joined.contains('…'));
        assert!(joined.chars().count() < MAX_TOOL_INPUT_CHARS + 40);
    }
}
