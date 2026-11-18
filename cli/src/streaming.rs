//! Live rendering of streamed turn events into the terminal.
//!
//! Design: the turn thread forwards each [`StreamEvent`] into a channel; a
//! single dedicated render thread owns all writes to stdout for the "live
//! region". Nothing else (prompt, permission dialog, summary) writes to the
//! terminal while a turn is streaming, so a redraw can never be interleaved
//! with other output. When the turn ends the caller drops the sender; the
//! render thread draws the final frame and exits, leaving the cursor at the
//! start of the line after the region.
//!
//! What the live region shows, top to bottom:
//! - the transcript so far: each message's text (word-wrapped at the terminal
//!   width) followed by the tool rows it requested (`⚙ tool  1.2s`),
//! - the assistant text streaming right now, still growing,
//! - or, while nothing has arrived yet, an animated thinking line.
//!
//! The region is redrawn in place (cursor up, clear line, rewrite). When it
//! grows beyond [`MAX_LINES`] the newest lines stay on screen and the older
//! content scrolls into the terminal's scrollback.

use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use code_runtime::EventSink;

/// Events the render thread consumes. A lean copy of the subset of
/// [`code_runtime::AssistantEvent`] the live region renders, kept so the
/// channel payload is `Send + Clone` without importing the full runtime type.
#[derive(Clone)]
enum StreamEvent {
    TextDelta(String),
    ToolUse { name: String },
    MessageStop,
}

impl StreamEvent {
    fn from_assistant_event(event: &code_runtime::AssistantEvent) -> Option<Self> {
        match event {
            code_runtime::AssistantEvent::TextDelta(delta) => Some(Self::TextDelta(delta.clone())),
            code_runtime::AssistantEvent::ToolUse { name, .. } => {
                Some(Self::ToolUse { name: name.clone() })
            }
            code_runtime::AssistantEvent::MessageStop => Some(Self::MessageStop),
            code_runtime::AssistantEvent::Usage(_)
            | code_runtime::AssistantEvent::PromptCache(_) => None,
        }
    }
}

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_INTERVAL: Duration = Duration::from_millis(90);
const THINKING_COLOR: &str = "\x1b[38;5;33m";
const TEXT_COLOR: &str = "\x1b[1m";
const TOOL_COLOR: &str = "\x1b[38;5;33m";
const DIM: &str = "\x1b[2m";
/// Erases the current line, then moves the cursor to column 0.
const CLEAR_LINE: &str = "\x1b[2K\r";
const RESET: &str = "\x1b[0m";
/// Maximum height of the live region before it starts scrolling the terminal.
const MAX_LINES: usize = 12;
/// Fallback width used when the terminal size cannot be determined.
const FALLBACK_COLS: usize = 100;

enum RenderCmd {
    Event(StreamEvent),
    Tick,
}

struct LiveState {
    /// Transcript of this turn, oldest first: assistant text and tool rows.
    segments: Vec<Segment>,
    /// Text streamed into the current segment (the live, still-growing part).
    pending_text: String,
    /// When the current segment's tool (if any) started; the idle timer and
    /// tool elapsed time are read from it.
    seg_started: Instant,
    frame: usize,
}

impl LiveState {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            pending_text: String::new(),
            seg_started: Instant::now(),
            frame: 0,
        }
    }
}

/// One immutable row of the transcript: finished assistant text or a tool.
#[derive(Debug)]
enum Segment {
    Text(String),
    Tool { name: String, started: Instant },
}

/// Renders streaming events for one turn in a live region above the prompt.
///
/// Create one with [`TurnStreamSink::new`], hand it to
/// `ConversationRuntime::run_turn_stream`, then call [`finish`]. The sink is
/// safe to use as the `EventSink` directly: `on_event` only forwards to the
/// render thread and returns `true` (never aborting the turn).
///
/// When stdout is not a terminal the sink is a no-op, so call sites can
/// always create one without checking.
pub struct TurnStreamSink {
    tx: Option<mpsc::Sender<RenderCmd>>,
    state: Arc<Mutex<LiveState>>,
    enabled: bool,
}

impl TurnStreamSink {
    /// Starts live rendering for a turn.
    #[must_use]
    pub fn new() -> Self {
        let enabled = io::stdout().is_terminal();
        let state = Arc::new(Mutex::new(LiveState::new()));

        let (tx, rx) = mpsc::channel();
        if enabled {
            let render_state = Arc::clone(&state);
            let cols = terminal_width().unwrap_or(FALLBACK_COLS);
            std::thread::spawn(move || render_loop(rx, render_state, cols));
        }

        Self {
            tx: if enabled { Some(tx) } else { None },
            state,
            enabled,
        }
    }

    /// Ends the turn: signals the render thread to draw the final frame and
    /// exit. Idempotent.
    pub fn finish(&mut self) {
        self.tx = None;
    }
}

impl Default for TurnStreamSink {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TurnStreamSink {
    fn drop(&mut self) {
        self.finish();
    }
}

impl EventSink for TurnStreamSink {
    fn on_event(&mut self, event: &code_runtime::AssistantEvent) -> bool {
        if let Some(tx) = &self.tx {
            if let Some(cmd) = StreamEvent::from_assistant_event(event) {
                let _ = tx.send(RenderCmd::Event(cmd));
            }
        }
        true
    }
}

fn render_loop(rx: mpsc::Receiver<RenderCmd>, state: Arc<Mutex<LiveState>>, cols: usize) {
    let mut prev_lines: usize = 0;
    loop {
        match rx.recv_timeout(FRAME_INTERVAL) {
            Ok(RenderCmd::Event(event)) => {
                if let Ok(mut s) = state.lock() {
                    apply(&mut s, event);
                }
            }
            Ok(RenderCmd::Tick) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = state.lock().map(|mut s| s.frame += 1);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = draw(&state, cols, prev_lines);
                // Leave cursor at start of the line after the region.
                let mut out = io::stdout();
                let _ = out.write_all(b"\r\n");
                let _ = out.flush();
                break;
            }
        }
        prev_lines = draw(&state, cols, prev_lines).unwrap_or(0);
    }
}

fn apply(state: &mut LiveState, event: StreamEvent) {
    match event {
        StreamEvent::TextDelta(delta) => state.pending_text.push_str(&delta),
        StreamEvent::ToolUse { name } => {
            // A ToolUse event precedes the next assistant message; start a new
            // text segment so tool calls stay visually attached to the text
            // that produced them, and restart the idle timer so the thinking
            // line reappears while the tool runs.
            state.seg_started = Instant::now();
            // The text streamed so far was produced by the message that
            // requested this tool; freeze it into the transcript first so the
            // tool row sits directly beneath its context.
            if state.pending_text.trim().is_empty() {
                state.pending_text.clear();
            } else {
                state
                    .segments
                    .push(Segment::Text(std::mem::take(&mut state.pending_text)));
            }
            state.segments.push(Segment::Tool {
                name,
                started: Instant::now(),
            });
        }
        StreamEvent::MessageStop => {}
    }
}

fn draw(state: &Arc<Mutex<LiveState>>, cols: usize, prev_lines: usize) -> Option<usize> {
    let Ok(s) = state.lock() else {
        return None;
    };
    let frame = render_frame(&s, cols);
    let lines = frame.len();

    let mut buf = String::new();
    if prev_lines > 0 {
        let _ = write!(buf, "\x1b[{prev_lines}A");
    }
    for line in &frame {
        buf.push_str(CLEAR_LINE);
        buf.push_str(line);
        buf.push('\n');
    }
    if lines > 0 {
        let _ = write!(buf, "\x1b[{lines}A");
    }

    let mut out = io::stdout();
    let _ = out.write_all(buf.as_bytes());
    let _ = out.flush();
    Some(lines)
}

fn render_frame(s: &LiveState, cols: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for segment in &s.segments {
        match segment {
            Segment::Text(text) => {
                for line in wrap(text, cols) {
                    lines.push(format!("{TEXT_COLOR}{line}{RESET}"));
                }
            }
            Segment::Tool { name, started } => {
                let secs = started.elapsed().as_secs_f64();
                lines.push(format!(
                    "{TOOL_COLOR}⚙ {name}{RESET} {DIM}{secs:.1}s{RESET}"
                ));
            }
        }
    }
    if !s.pending_text.is_empty() {
        let text = s.pending_text.trim_end_matches(['\r', '\n']);
        for line in wrap(text, cols) {
            lines.push(format!("{TEXT_COLOR}{line}{RESET}"));
        }
    }
    // Idle animation: nothing growing right now (no pending text) and the
    // current segment is not a running tool.
    let tool_running = matches!(s.segments.last(), Some(Segment::Tool { .. }));
    if s.pending_text.is_empty() && !tool_running {
        let glyph = FRAMES[s.frame % FRAMES.len()];
        let secs = s.seg_started.elapsed().as_secs();
        lines.push(format!(
            "{THINKING_COLOR}{glyph} Thinking... {secs}s{RESET}"
        ));
    }
    // Keep the newest lines on screen; the rest scrolls into scrollback.
    let keep = lines.len().saturating_sub(MAX_LINES);
    lines.drain(0..keep);
    lines
}

fn wrap(text: &str, cols: usize) -> Vec<String> {
    let cols = cols.max(3);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut len = 0usize;
        for word in raw.split_inclusive(' ') {
            let word = word.trim_end_matches(' ');
            if word.is_empty() {
                continue;
            }
            let wlen = word.chars().count();
            if wlen > cols {
                // Current word alone exceeds the width: flush the current line,
                // hard-break the word into chunks, and start fresh.
                if len > 0 {
                    out.push(line.clone());
                    line.clear();
                }
                let mut rest = word.to_string();
                while rest.chars().count() > cols {
                    let chunk: String = rest.chars().take(cols).collect();
                    out.push(chunk);
                    rest = rest.chars().skip(cols).collect();
                }
                line = rest;
                len = line.chars().count();
                continue;
            }
            if len > 0 && len + 1 + wlen >= cols {
                // Current line is non-empty and the word (plus its separating
                // space) would push it past the width: flush and start a new
                // line with just this word.
                out.push(line.clone());
                line.clear();
                len = 0;
            }
            if len > 0 {
                line.push(' ');
                len += 1;
            }
            line.push_str(word);
            len += wlen;
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

fn terminal_width() -> Option<usize> {
    match crossterm::terminal::size() {
        Ok((w, _)) if w > 0 => Some(w as usize),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_keeps_short_lines() {
        assert_eq!(wrap("hello world", 80), vec!["hello world"]);
    }

    #[test]
    fn wrap_breaks_long_lines() {
        assert_eq!(
            wrap("a b c d e f g h i j", 10),
            vec!["a b c d e", "f g h i j"]
        );
    }

    #[test]
    fn wrap_respects_newlines() {
        assert_eq!(wrap("one\ntwo", 80), vec!["one", "two"]);
    }

    #[test]
    fn wrap_hard_breaks_long_words() {
        assert_eq!(wrap("aaaaaaaaaa bb", 5), vec!["aaaaa", "aaaaa", "bb"]);
    }

    #[test]
    fn frame_is_thinking_line_when_idle() {
        let s = LiveState::new();
        let frame = render_frame(&s, 80);
        assert_eq!(frame.len(), 1);
        assert!(frame[0].contains("Thinking"));
    }

    #[test]
    fn frame_shows_running_tool_and_pending_text() {
        let mut s = LiveState::new();
        s.segments.push(Segment::Tool {
            name: "Read".into(),
            started: Instant::now(),
        });
        s.pending_text = "hello\nworld".into();
        let frame = render_frame(&s, 80);
        assert_eq!(frame.len(), 3);
        assert!(frame[0].contains("Read"));
        assert!(frame[1].contains("hello"));
        assert!(frame[2].contains("world"));
    }

    #[test]
    fn thinking_line_reappears_after_tool_finished() {
        // A finished assistant segment (no running tool) is idle: the
        // thinking line shows below it while the model plans the next step.
        let mut s = LiveState::new();
        s.segments.push(Segment::Text("done".into()));
        s.pending_text.clear();
        let frame = render_frame(&s, 80);
        assert_eq!(frame.len(), 2);
        assert!(frame[0].contains("done"));
        assert!(frame[1].contains("Thinking"));
    }

    #[test]
    fn no_thinking_line_while_tool_runs() {
        let mut s = LiveState::new();
        s.segments.push(Segment::Tool {
            name: "Bash".into(),
            started: Instant::now(),
        });
        let frame = render_frame(&s, 80);
        assert_eq!(frame.len(), 1);
        assert!(frame[0].contains("Bash"));
    }

    #[test]
    fn frame_is_bounded_to_max_lines() {
        let mut s = LiveState::new();
        s.pending_text = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let frame = render_frame(&s, 80);
        assert_eq!(frame.len(), MAX_LINES);
        assert!(frame.last().unwrap().contains("line 99"));
    }

    #[test]
    fn stream_event_maps_text_delta() {
        let ev = code_runtime::AssistantEvent::TextDelta("hello".into());
        let mapped = StreamEvent::from_assistant_event(&ev);
        assert!(matches!(mapped, Some(StreamEvent::TextDelta(ref s)) if s == "hello"));
    }

    #[test]
    fn stream_event_maps_tool_use() {
        let ev = code_runtime::AssistantEvent::ToolUse {
            id: "1".into(),
            name: "Read".into(),
            input: "{}".into(),
        };
        let mapped = StreamEvent::from_assistant_event(&ev);
        assert!(matches!(mapped, Some(StreamEvent::ToolUse { ref name }) if name == "Read"));
    }

    #[test]
    fn stream_event_drops_usage() {
        let ev = code_runtime::AssistantEvent::Usage(code_runtime::TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        });
        assert!(StreamEvent::from_assistant_event(&ev).is_none());
    }
}
