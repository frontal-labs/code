//! Pure state machine for the interactive REPL.
//!
//! The [`App`] owns all REPL display state: the conversation history, the
//! current turn's streaming status, and the permission mode. It knows nothing
//! about the terminal, the runtime, or I/O — it is a pure state machine that
//! receives [`AppEvent`]s and exposes state a renderer can paint.
//!
//! Why this exists:
//! - The turn loop in `main.rs` mixes state transitions with terminal writes.
//!   Extracting the state lets us test the conversation model in isolation and
//!   swap the renderer without touching the runtime wiring.
//! - The streaming sink (`streaming.rs`) maintains its own segment model for
//!   live rendering. The `App`'s message model is the *settled* view, distinct
//!   from the live region.
//!
//! This module is deliberately framework-free. `main.rs` constructs an `App`,
//! feeds it events as the turn progresses, and reads the resulting state.

use std::time::Instant;

use code_runtime::{ContentBlock, ConversationMessage, MessageRole, TokenUsage, TurnSummary};

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// One message in the conversation history.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A user prompt.
    User {
        /// The prompt text (after mention expansion).
        text: String,
        /// Files attached via `@` mentions.
        attached: Vec<String>,
    },
    /// An assistant response: the final text plus the tool calls it made.
    Assistant {
        /// The assistant's final text.
        text: String,
        /// Tool names invoked during this turn, in order.
        tools: Vec<String>,
        /// Wall-clock duration of the entire turn in seconds.
        elapsed_secs: f64,
        /// Token usage for the turn.
        usage: Option<TokenUsage>,
        /// Number of messages removed by auto-compaction, if it fired.
        auto_compaction: Option<usize>,
    },
}

// ---------------------------------------------------------------------------
// Streaming status
// ---------------------------------------------------------------------------

/// What the UI should show while a turn is in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStatus {
    /// Waiting for the first model event — show the thinking animation.
    Thinking,
    /// A tool is currently executing.
    ToolRunning { name: String },
    /// The model is producing text (or just finished a tool and is planning).
    Streaming,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

/// The full REPL state. Pure data — no I/O, no terminal, no runtime handle.
#[derive(Debug)]
pub struct App {
    /// The conversation so far, oldest first.
    pub messages: Vec<Message>,
    /// The current permission mode (e.g. "default", "plan", "read-only").
    pub permission_mode: String,
    /// The model name.
    pub model: String,
    /// Set while a turn is in flight; `None` when idle.
    pub turn_status: Option<TurnStatus>,
    /// Text being streamed right now (grows as deltas arrive).
    pub pending_text: String,
    /// When the current status segment started (drives elapsed-time display).
    pub seg_started: Instant,
    /// Set when the last turn ended in an error.
    pub last_error: Option<String>,
    /// Monotonic frame counter for the thinking animation.
    pub frame: u32,
}

impl App {
    #[must_use]
    pub fn new(model: String, permission_mode: String) -> Self {
        Self {
            messages: Vec::new(),
            permission_mode,
            model,
            turn_status: None,
            pending_text: String::new(),
            seg_started: Instant::now(),
            last_error: None,
            frame: 0,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(String::new(), String::new())
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// An event that transitions the [`App`].
#[derive(Debug)]
pub enum AppEvent {
    /// The user submitted a prompt (after mention expansion).
    UserSubmit {
        /// The prompt text.
        text: String,
        /// Files attached via `@` mentions.
        attached: Vec<String>,
    },
    /// A turn began — the runtime is about to call the model.
    TurnStarted,
    /// The model is thinking (no tool running, no text yet).
    Thinking,
    /// A tool started executing.
    ToolStarted {
        /// Display name of the tool.
        name: String,
    },
    /// The current tool finished; the model is thinking again.
    ToolFinished,
    /// A piece of streamed text arrived.
    TextDelta(String),
    /// Token usage was reported (informational; rendered at turn end).
    Usage(TokenUsage),
    /// The model finished a message (it may still run tools).
    MessageStop,
    /// The turn completed successfully.
    TurnCompleted(TurnSummary),
    /// The turn failed.
    TurnFailed(String),
    /// The user changed the permission mode.
    PermissionModeChanged {
        /// The previous mode (for the transition notice).
        previous: String,
        /// The new mode.
        next: String,
    },
    /// Advance the animation frame (render loop idle tick).
    Frame,
    /// Clear the screen (e.g. Ctrl+L) — the renderer handles the actual clear.
    Clear,
}

// ---------------------------------------------------------------------------
// Transition
// ---------------------------------------------------------------------------

impl App {
    /// Apply an event, mutating state in place.
    pub fn handle(&mut self, event: &AppEvent) {
        match event {
            AppEvent::UserSubmit { text, attached } => {
                self.messages.push(Message::User {
                    text: text.clone(),
                    attached: attached.clone(),
                });
                self.last_error = None;
            }

            AppEvent::TurnStarted => {
                self.turn_status = Some(TurnStatus::Thinking);
                self.pending_text.clear();
                self.seg_started = Instant::now();
                self.last_error = None;
            }

            AppEvent::Thinking => {
                if !matches!(self.turn_status, Some(TurnStatus::Thinking)) {
                    self.turn_status = Some(TurnStatus::Thinking);
                    self.seg_started = Instant::now();
                }
            }

            AppEvent::ToolStarted { name } => {
                self.turn_status = Some(TurnStatus::ToolRunning { name: name.clone() });
                self.seg_started = Instant::now();
            }

            AppEvent::ToolFinished => {
                self.turn_status = Some(TurnStatus::Thinking);
                self.seg_started = Instant::now();
            }

            AppEvent::TextDelta(delta) => {
                if self.pending_text.is_empty() {
                    self.turn_status = Some(TurnStatus::Streaming);
                    self.seg_started = Instant::now();
                }
                self.pending_text.push_str(delta);
            }

            // Usage is captured on the TurnSummary at completion; clear and
            // both no-ops are grouped so the match is exhaustive.
            AppEvent::Usage(_) | AppEvent::Clear => {}

            AppEvent::MessageStop => {
                self.turn_status = Some(TurnStatus::Thinking);
            }

            AppEvent::TurnCompleted(summary) => {
                let text = final_assistant_text(summary);
                let tools = collect_tool_names(summary);
                let usage = Some(summary.usage);
                let auto_compaction = summary.auto_compaction.map(|e| e.removed_message_count);

                // Fold any still-pending streamed text into the message so we
                // don't lose a final chunk that arrived after the last event.
                let text = if !self.pending_text.is_empty()
                    && !text.is_empty()
                    && !text.ends_with(&self.pending_text)
                {
                    let mut merged = text;
                    merged.push('\n');
                    merged.push_str(&self.pending_text);
                    merged
                } else {
                    text
                };

                self.messages.push(Message::Assistant {
                    text,
                    tools,
                    elapsed_secs: 0.0, // filled in by the caller via `stamp_turn_elapsed`
                    usage,
                    auto_compaction,
                });
                self.turn_status = None;
                self.pending_text.clear();
            }

            AppEvent::TurnFailed(message) => {
                self.last_error = Some(message.clone());
                self.turn_status = None;
                self.pending_text.clear();
            }

            AppEvent::PermissionModeChanged { previous, next } => {
                let _ = previous;
                self.permission_mode.clone_from(next);
            }

            AppEvent::Frame => {
                self.frame = self.frame.wrapping_add(1);
            }
        }
    }

    /// Overwrite the elapsed time on the most recent assistant message.
    /// Called by the runtime wrapper after the turn returns, when the real
    /// wall-clock duration is known.
    pub fn stamp_turn_elapsed(&mut self, elapsed_secs: f64) {
        if let Some(Message::Assistant {
            elapsed_secs: slot, ..
        }) = self.messages.last_mut()
        {
            *slot = elapsed_secs;
        }
    }

    /// Whether a turn is currently in flight.
    #[must_use]
    pub fn is_turn_active(&self) -> bool {
        self.turn_status.is_some()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the final assistant text from a turn summary, mirroring the
/// semantics of `main.rs::final_assistant_text` (last message, all text
/// blocks joined).
#[must_use]
pub fn final_assistant_text(summary: &TurnSummary) -> String {
    summary
        .assistant_messages
        .last()
        .map(|message| {
            message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Extract tool names from a turn summary, in order.
#[must_use]
pub fn collect_tool_names(summary: &TurnSummary) -> Vec<String> {
    summary
        .assistant_messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new("claude-sonnet-4".into(), "default".into())
    }

    fn empty_summary() -> TurnSummary {
        TurnSummary {
            assistant_messages: vec![],
            tool_results: vec![],
            prompt_cache_events: vec![],
            iterations: 1,
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
            },
            auto_compaction: None,
        }
    }

    fn text_summary(text: &str) -> TurnSummary {
        let mut s = empty_summary();
        s.assistant_messages.push(ConversationMessage::new(
            MessageRole::Assistant,
            vec![ContentBlock::Text { text: text.into() }],
            None,
        ));
        s
    }

    #[test]
    fn new_app_is_idle() {
        let a = app();
        assert_eq!(a.messages.len(), 0);
        assert!(a.turn_status.is_none());
        assert_eq!(a.pending_text, "");
        assert!(a.last_error.is_none());
        assert!(!a.is_turn_active());
    }

    #[test]
    fn user_submit_pushes_message() {
        let mut a = app();
        a.handle(&AppEvent::UserSubmit {
            text: "hello".into(),
            attached: vec!["src/main.rs".into()],
        });
        assert_eq!(a.messages.len(), 1);
        match &a.messages[0] {
            Message::User { text, attached } => {
                assert_eq!(text, "hello");
                assert_eq!(attached, &["src/main.rs"]);
            }
            Message::Assistant { .. } => panic!("expected User message"),
        }
    }

    #[test]
    fn turn_started_sets_thinking() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        assert!(matches!(a.turn_status, Some(TurnStatus::Thinking)));
        assert!(a.is_turn_active());
    }

    #[test]
    fn text_delta_after_thinking_moves_to_streaming() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        assert!(matches!(a.turn_status, Some(TurnStatus::Thinking)));
        a.handle(&AppEvent::TextDelta("Hello".into()));
        assert!(matches!(a.turn_status, Some(TurnStatus::Streaming)));
        assert_eq!(a.pending_text, "Hello");
        a.handle(&AppEvent::TextDelta(" world".into()));
        assert_eq!(a.pending_text, "Hello world");
    }

    #[test]
    fn tool_started_sets_tool_running() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::TextDelta("Let me check.".into()));
        a.handle(&AppEvent::ToolStarted {
            name: "Read".into(),
        });
        assert!(matches!(
            a.turn_status,
            Some(TurnStatus::ToolRunning { ref name }) if name == "Read"
        ));
    }

    #[test]
    fn tool_finished_returns_to_thinking() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::ToolStarted {
            name: "Bash".into(),
        });
        a.handle(&AppEvent::ToolFinished);
        assert!(matches!(a.turn_status, Some(TurnStatus::Thinking)));
    }

    #[test]
    fn turn_completed_pushes_assistant_message() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::TextDelta("Done!".into()));
        a.handle(&AppEvent::TurnCompleted(text_summary("Done!")));
        assert!(!a.is_turn_active());
        assert_eq!(a.messages.len(), 1);
        match &a.messages[0] {
            Message::Assistant {
                text,
                tools,
                usage,
                auto_compaction,
                ..
            } => {
                assert_eq!(text, "Done!");
                assert_eq!(tools.len(), 0);
                assert!(usage.is_some());
                assert_eq!(*auto_compaction, None);
            }
            Message::User { .. } => panic!("expected Assistant message"),
        }
    }

    #[test]
    fn turn_completed_collects_tool_names() {
        let mut summary = text_summary("text");
        summary.assistant_messages[0]
            .blocks
            .push(ContentBlock::ToolUse {
                id: "1".into(),
                name: "Read".into(),
                input: "{}".into(),
            });
        summary.assistant_messages[0]
            .blocks
            .push(ContentBlock::ToolUse {
                id: "2".into(),
                name: "Write".into(),
                input: "{}".into(),
            });
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::TurnCompleted(summary));
        match &a.messages[0] {
            Message::Assistant { tools, .. } => assert_eq!(tools, &["Read", "Write"]),
            Message::User { .. } => panic!("expected Assistant message"),
        }
    }

    #[test]
    fn turn_failed_sets_error() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::TurnFailed("API error".into()));
        assert!(!a.is_turn_active());
        assert_eq!(a.last_error.as_deref(), Some("API error"));
    }

    #[test]
    fn user_submit_clears_previous_error() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::TurnFailed("boom".into()));
        assert!(a.last_error.is_some());
        a.handle(&AppEvent::UserSubmit {
            text: "retry".into(),
            attached: vec![],
        });
        assert!(a.last_error.is_none());
    }

    #[test]
    fn permission_mode_change_updates_state() {
        let mut a = app();
        a.handle(&AppEvent::PermissionModeChanged {
            previous: "default".into(),
            next: "plan".into(),
        });
        assert_eq!(a.permission_mode, "plan");
    }

    #[test]
    fn frame_advances() {
        let mut a = app();
        let start = a.frame;
        a.handle(&AppEvent::Frame);
        a.handle(&AppEvent::Frame);
        assert_eq!(a.frame, start + 2);
    }

    #[test]
    fn usage_event_is_noop() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::Usage(TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        }));
        assert!(matches!(a.turn_status, Some(TurnStatus::Thinking)));
    }

    #[test]
    fn message_stop_returns_to_thinking() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::TextDelta("hello".into()));
        assert!(matches!(a.turn_status, Some(TurnStatus::Streaming)));
        a.handle(&AppEvent::MessageStop);
        assert!(matches!(a.turn_status, Some(TurnStatus::Thinking)));
    }

    #[test]
    fn stamp_turn_elapsed_updates_last_assistant() {
        let mut a = app();
        a.handle(&AppEvent::TurnStarted);
        a.handle(&AppEvent::TurnCompleted(text_summary("hi")));
        a.stamp_turn_elapsed(3.5);
        match &a.messages[0] {
            Message::Assistant { elapsed_secs, .. } => {
                assert!((elapsed_secs - 3.5).abs() < f64::EPSILON);
            }
            Message::User { .. } => panic!("expected Assistant message"),
        }
    }

    #[test]
    fn final_assistant_text_extracts_text_block() {
        assert_eq!(
            final_assistant_text(&text_summary("Hello world")),
            "Hello world"
        );
        assert_eq!(final_assistant_text(&empty_summary()), "");
    }
}
