//! Conversation transcript for the full-screen TUI.
//!
//! The transcript holds styled lines produced from the same ANSI strings the
//! line-based REPL prints: the startup banner, user input, assistant replies
//! (rendered as markdown with emoji removed), tool headlines and results, turn
//! summaries, and notices. Text that is still streaming is kept raw until the
//! turn ends, then rendered as markdown.

use ratatui::text::{Line, Span};

use super::ansi;
use crate::render::{strip_emoji, TerminalRenderer};

const MAX_LINES: usize = 1_200;

/// A point-in-time, renderable copy of the transcript.
#[derive(Debug, Clone, Default)]
pub struct TranscriptSnapshot {
    pub lines: Vec<Line<'static>>,
    pub pending_text: String,
}

/// The live, mutating transcript.
#[derive(Debug, Default)]
pub struct Transcript {
    lines: Vec<Line<'static>>,
    pending_text: String,
}

impl Transcript {
    /// Renders the streamed assistant text so far as markdown, then clears it.
    pub fn flush_text(&mut self) {
        if self.pending_text.trim().is_empty() {
            self.pending_text.clear();
            return;
        }
        let text = std::mem::take(&mut self.pending_text);
        let rendered = TerminalRenderer::new().markdown_to_ansi(&strip_emoji(&text));
        self.push_ansi(&rendered);
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            let excess = self.lines.len() - MAX_LINES;
            self.lines.drain(0..excess);
        }
    }

    /// Appends ANSI-styled text, one transcript line per text line.
    pub fn push_ansi(&mut self, text: &str) {
        for line in ansi::lines(text) {
            self.push_line(line);
        }
    }

    /// Appends streamed assistant text. It is shown raw until the turn ends.
    pub fn push_text(&mut self, text: &str) {
        self.pending_text.push_str(&strip_emoji(text));
    }

    /// Ends an assistant message: renders any pending text as markdown.
    pub fn end_turn(&mut self) {
        self.flush_text();
    }

    /// Records a user line the way the REPL echoes its prompt.
    pub fn push_user(&mut self, text: &str) {
        self.flush_text();
        self.push_line(Line::default());
        for (index, paragraph) in text.split('\n').enumerate() {
            let prefix = if index == 0 { "❯ " } else { "  " };
            self.push_line(Line::from(Span::raw(format!("{prefix}{paragraph}"))));
        }
    }

    /// Records a tool call headline, using the REPL's format.
    pub fn push_tool_start(&mut self, name: &str, input: &str) {
        self.flush_text();
        self.push_ansi(&format!("\n{}", crate::format_tool_call_start(name, input)));
    }

    /// Records a tool result, using the REPL's format.
    pub fn push_tool_result(&mut self, name: &str, output: &str, is_error: bool) {
        self.flush_text();
        self.push_ansi(&crate::format_tool_result(name, output, is_error));
    }

    /// Clears the transcript (used by Ctrl-L before the banner is drawn again).
    pub fn clear(&mut self) {
        self.lines.clear();
        self.pending_text.clear();
    }

    /// Point-in-time copy for the render pass.
    pub fn snapshot(&self) -> TranscriptSnapshot {
        TranscriptSnapshot {
            lines: self.lines.clone(),
            pending_text: self.pending_text.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(transcript: &Transcript) -> String {
        transcript
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn streamed_text_is_kept_until_the_turn_ends() {
        let mut transcript = Transcript::default();
        transcript.push_text("hello ");
        transcript.push_text("world");
        assert_eq!(transcript.snapshot().pending_text, "hello world");
        transcript.end_turn();
        assert_eq!(transcript.snapshot().pending_text, "");
        assert!(joined(&transcript).contains("hello world"));
    }

    #[test]
    fn assistant_text_has_emoji_removed() {
        let mut transcript = Transcript::default();
        transcript.push_text("hi 👋 there");
        transcript.end_turn();
        assert!(!joined(&transcript).contains('👋'));
    }

    #[test]
    fn tool_headline_and_result_use_the_repl_format() {
        let mut transcript = Transcript::default();
        transcript.push_user("run ls");
        transcript.push_tool_start("bash", r#"{"command":"ls -la"}"#);
        transcript.push_tool_result("bash", r#"{"stdout":"file.txt","stderr":""}"#, false);
        let text = joined(&transcript);
        assert!(text.contains("❯ run ls"));
        assert!(text.contains("● Bash"));
        assert!(text.contains("ls -la"));
        assert!(text.contains("file.txt"));
    }

    #[test]
    fn clear_removes_everything() {
        let mut transcript = Transcript::default();
        transcript.push_ansi("banner");
        transcript.push_text("pending");
        transcript.clear();
        let snapshot = transcript.snapshot();
        assert_eq!(snapshot.lines, []);
        assert_eq!(snapshot.pending_text, "");
    }

    #[test]
    fn transcript_is_bounded() {
        let mut transcript = Transcript::default();
        for index in 0..MAX_LINES + 100 {
            transcript.push_ansi(&format!("line {index}"));
        }
        assert!(transcript.lines.len() <= MAX_LINES);
    }
}
