//! Full-screen TUI for the interactive REPL (`frontal-code --tui`).
//!
//! The TUI renders a Claude-Code-style conversation: a header with the model,
//! a live transcript (assistant text, tool calls, tool results, and captured
//! slash command output appear as the turn runs), a status bar with provider /
//! mode / context / cost, and an inline composer with history at the bottom.
//!
//! Architecture: a single UI thread owns the terminal. Turns run on a worker
//! thread through the same runtime entry points the line-based REPL uses
//! (`prepare_turn_runtime` + `run_turn_stream`); the worker ships [`UiEvent`]s
//! over an `mpsc` channel and the UI thread applies them to shared
//! [`TuiState`]. The finished runtime is handed back over a return channel and
//! reinstalled into the CLI, so sessions keep accumulating across turns
//! exactly like the line-based REPL. Slash commands run inline on the UI
//! thread (their output is captured into the transcript, or a banner explains
//! when a command must run in the line-based REPL).

mod transcript;

use std::io::{self, Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use code_runtime::{
    pricing_for_model, AssistantEvent, ContentBlock, EventSink, PermissionMode,
    PermissionPromptDecision, PermissionPrompter, PermissionRequest,
};

use crate::tui::transcript::{ToolStatus, Transcript};
use crate::{format_auto_compaction_notice, LiveCli, SlashCommand};

type CrosstermTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// Runs the full-screen TUI on `cli` and returns when the user exits.
///
/// `run_slash_command` is a thin callback into [`LiveCli`] that handles one
/// parsed slash command, writing its output into `buffer`. Keeping it as a
/// callback means this module never reaches into `LiveCli`'s private fields
/// except through the accessors declared on `LiveCli` itself.
pub(crate) fn run<F>(
    cli: &mut LiveCli,
    run_slash_command: &mut F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(
        &mut LiveCli,
        SlashCommand,
        &mut Vec<u8>,
    ) -> Result<bool, Box<dyn std::error::Error>>,
{
    if !terminal_supported() {
        return Err(
            "the full-screen TUI requires a real terminal (try plain `frontal-code` instead)"
                .into(),
        );
    }

    let mut terminal = prepare_terminal()?;
    let state = Arc::new(Mutex::new(TuiState::from_cli(cli)));
    let result = run_loop(cli, &mut terminal, &state, run_slash_command);
    teardown_terminal(&mut terminal);
    let should_persist = state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .should_persist;
    if should_persist {
        let _ = cli.persist_session();
    }
    result
}

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

struct TuiState {
    model: String,
    provider: String,
    mode: String,
    context_tokens: usize,
    cost_usd: f64,
    transcript: Transcript,
    draft: String,
    cursor: usize,
    history: Vec<String>,
    history_cursor: Option<usize>,
    busy: bool,
    frame_index: usize,
    status_line: Option<String>,
    error: Option<String>,
    quit: bool,
    should_persist: bool,
    session_started: Instant,
    turn_started: Option<Instant>,
}

impl TuiState {
    fn from_cli(cli: &LiveCli) -> Self {
        Self {
            model: cli.model().to_string(),
            provider: cli.provider().map_or_else(|| "default".to_string(), str::to_string),
            mode: cli.permission_mode().as_str().to_string(),
            context_tokens: cli.estimated_tokens(),
            cost_usd: cli.cumulative_cost_usd(),
            transcript: Transcript::default(),
            draft: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_cursor: None,
            busy: false,
            frame_index: 0,
            status_line: Some(
                "Type /help for commands · /status for live context · /exit to quit".to_string(),
            ),
            error: None,
            quit: false,
            should_persist: false,
            session_started: Instant::now(),
            turn_started: None,
        }
    }

    fn insert_at_cursor(&mut self, text: &str) {
        let left: String = self.draft.chars().take(self.cursor).collect();
        let right: String = self.draft.chars().skip(self.cursor).collect();
        self.draft = format!("{left}{text}{right}");
        self.cursor += text.chars().count();
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            let mut chars: Vec<char> = self.draft.chars().collect();
            chars.remove(self.cursor - 1);
            self.draft = chars.into_iter().collect();
            self.cursor -= 1;
        }
    }

    fn push_history(&mut self, line: String) {
        if let Some(position) = self.history_cursor {
            self.history.insert(position, line);
        } else {
            self.history.push(line);
        }
        self.history_cursor = None;
    }
}

/// A point-in-time copy of [`TuiState`] handed to the draw closure so no lock
/// is held while ratatui writes to the terminal.
#[derive(Debug, Clone)]
struct Snapshot {
    model: String,
    provider: String,
    mode: String,
    context_tokens: usize,
    cost_usd: f64,
    busy: bool,
    frame_index: usize,
    transcript: crate::tui::transcript::TranscriptSnapshot,
    status_line: Option<String>,
    error: Option<String>,
    draft: String,
    cursor: usize,
    session_secs: u64,
    turn_secs: Option<u64>,
}

impl TuiState {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            model: self.model.clone(),
            provider: self.provider.clone(),
            mode: self.mode.clone(),
            context_tokens: self.context_tokens,
            cost_usd: self.cost_usd,
            busy: self.busy,
            frame_index: self.frame_index,
            transcript: self.transcript.snapshot(),
            status_line: self.status_line.clone(),
            error: self.error.clone(),
            draft: self.draft.clone(),
            cursor: self.cursor,
            session_secs: self.session_started.elapsed().as_secs(),
            turn_secs: self.turn_started.map(|started| started.elapsed().as_secs()),
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal setup / teardown
// ---------------------------------------------------------------------------

fn terminal_supported() -> bool {
    std::env::var("NO_COLOR").is_err() && std::env::var("TERM").map_or(true, |term| term != "dumb")
}

fn prepare_terminal() -> Result<CrosstermTerminal, Box<dyn std::error::Error>> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn teardown_terminal(terminal: &mut CrosstermTerminal) {
    let result = (|| -> io::Result<()> {
        terminal.show_cursor()?;
        terminal.backend_mut().execute(LeaveAlternateScreen)?;
        disable_raw_mode()?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("warning: failed to restore terminal state: {error}");
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn run_loop<F>(
    cli: &mut LiveCli,
    terminal: &mut CrosstermTerminal,
    state: &Arc<Mutex<TuiState>>,
    run_slash_command: &mut F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(
        &mut LiveCli,
        SlashCommand,
        &mut Vec<u8>,
    ) -> Result<bool, Box<dyn std::error::Error>>,
{
    let (ui_tx, ui_rx) = channel::<UiEvent>();
    let (turn_tx, turn_rx) = channel::<String>();
    let (turn_done_tx, turn_done_rx) = channel::<(crate::BuiltRuntime, crate::HookAbortMonitor)>();
    let (permission_tx, mut permission_rx) = channel::<char>();
    let mut pending_turn: Option<PendingTurn> = None;
    let mut last_redraw = Instant::now();

    'outer: loop {
        // Drain any UI events queued by the turn worker.
        while let Ok(event) = ui_rx.try_recv() {
            apply_ui_event(state, event);
        }

        // Kick off a newly submitted line (unless one turn is already running).
        if pending_turn.is_none() {
            if let Ok(line) = turn_rx.try_recv() {
                pending_turn = Some(PendingTurn { line });
            }
        }

        // Reinstall a finished turn's runtime into the CLI.
        if let Ok((runtime, _hook_abort_monitor)) = turn_done_rx.try_recv() {
            if let Err(error) = cli.replace_runtime(runtime) {
                let mut guard = lock(state);
                guard.busy = false;
                guard.error = Some(format!("failed to update session after turn: {error}"));
            }
        }

        // Dispatch the pending line: slash commands inline, turns on a worker.
        if let Some(pending) = pending_turn.take() {
            let permission_rx = std::mem::replace(&mut permission_rx, channel::<char>().1);
            dispatch_line(
                cli,
                state,
                &ui_tx,
                &turn_done_tx,
                permission_rx,
                pending,
                run_slash_command,
            );
        }

        if lock(state).quit {
            break 'outer;
        }

        draw(terminal, state);

        let frame_budget = Duration::from_millis(80);
        let timeout = frame_budget.saturating_sub(last_redraw.elapsed());
        match crossterm::event::poll(timeout) {
            Ok(true) => match crossterm::event::read() {
                Ok(Event::Resize(width, height)) => {
                    let _ = terminal.resize(ratatui::layout::Rect::default());
                }
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if handle_key(key, state, &turn_tx) {
                        break 'outer;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("warning: failed to read terminal event: {error}");
                }
            },
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                eprintln!("warning: failed to poll terminal events: {error}");
            }
        }
        last_redraw = Instant::now();
    }

    // If a turn is still running when the user exits, wait for it to finish so
    // the session ends in a consistent state.
    if pending_turn.is_some() || turn_done_rx.try_recv().is_ok() {
        while let Ok(event) = ui_rx.try_recv() {
            apply_ui_event(state, event);
        }
        let guard = lock(state);
        if guard.busy {
            drop(guard);
            if let Ok((runtime, _hook_abort_monitor)) = turn_done_rx.recv() {
                let _ = cli.replace_runtime(runtime);
            }
        }
    }

    Ok(())
}

/// A submitted line that has not been dispatched to a worker yet.
struct PendingTurn {
    line: String,
}

/// Dispatches one submitted line: `/exit` quits, other slash commands run
/// inline with captured output, and everything else starts a streaming turn
/// on a worker thread.
fn dispatch_line<F>(
    cli: &mut LiveCli,
    state: &Arc<Mutex<TuiState>>,
    ui_tx: &Sender<UiEvent>,
    turn_done_tx: &Sender<(crate::BuiltRuntime, crate::HookAbortMonitor)>,
    permission_rx: Receiver<char>,
    pending: PendingTurn,
    run_slash_command: &mut F,
) where
    F: FnMut(
        &mut LiveCli,
        SlashCommand,
        &mut Vec<u8>,
    ) -> Result<bool, Box<dyn std::error::Error>>,
{
    let line = pending.line;

    // `/exit` is intercepted here because `handle_repl_command` treats it as
    // unavailable; the TUI handles it directly.
    if line.trim() == "/exit" || line.trim() == "/quit" {
        let mut guard = lock(state);
        guard.quit = true;
        guard.should_persist = true;
        return;
    }

    let parsed = match crate::SlashCommand::parse(&line) {
        Ok(Some(command)) => Some(command),
        Ok(None) => None,
        Err(error) => {
            let mut guard = lock(state);
            guard.error = Some(error.to_string());
            return;
        }
    };

    if let Some(command) = parsed {
        let mut buffer: Vec<u8> = Vec::new();
        let handled = match run_slash_command(cli, command, &mut buffer) {
            Ok(handled) => handled,
            Err(error) => {
                let _ = ui_tx.send(UiEvent::Output(format!("error: {error}")));
                return;
            }
        };
        if !buffer.is_empty() {
            let _ = ui_tx.send(UiEvent::Output(
                String::from_utf8_lossy(&buffer).into_owned(),
            ));
        }
        if handled {
            // The command cleared the session (e.g. `/clear`): refresh the
            // derived status values from the CLI's new state.
            let mut guard = lock(state);
            guard.context_tokens = cli.estimated_tokens();
            guard.cost_usd = cli.cumulative_cost_usd();
            guard.mode = cli.permission_mode().as_str().to_string();
            guard.model = cli.model().to_string();
            guard.provider = cli.provider().map_or_else(|| "default".to_string(), str::to_string);
        }
        return;
    }

    start_turn(
        cli,
        state,
        line,
        ui_tx.clone(),
        turn_done_tx.clone(),
        permission_rx,
    );
}

/// Starts a streaming turn on a worker thread. The worker owns a freshly
/// built runtime and ships UI events + the finished runtime back to the main
/// loop.
fn start_turn(
    cli: &mut LiveCli,
    state: &Arc<Mutex<TuiState>>,
    line: String,
    ui_tx: Sender<UiEvent>,
    turn_done_tx: Sender<(crate::BuiltRuntime, crate::HookAbortMonitor)>,
    permission_rx: Receiver<char>,
) {
    let (runtime, hook_abort_monitor) = match cli.prepare_turn_runtime(false) {
        Ok(pair) => pair,
        Err(error) => {
            let _ = ui_tx.send(UiEvent::TurnEnd {
                ok: false,
                error: Some(error.to_string()),
                status: None,
            });
            return;
        }
    };

    let model = cli.model().to_string();
    let permission_mode = cli.permission_mode();

    let mut guard = lock(state);
    guard.transcript.push_user(&line);
    guard.busy = true;
    guard.turn_started = Some(Instant::now());
    guard.status_line = Some("working…".to_string());
    guard.error = None;
    drop(guard);

    std::thread::spawn(move || {
        let mut runtime = runtime;
        let mut prompter = TuiPermissionPrompter {
            current_mode: permission_mode,
            tx: ui_tx.clone(),
            rx: permission_rx,
        };
        let mut sink = TuiEventSink { tx: ui_tx.clone() };
        let result = runtime.run_turn_stream(&line, Some(&mut prompter), &mut sink);

        let (ok, error, status) = match &result {
            Ok(summary) => {
                let status = summary
                    .auto_compaction
                    .map(|event| format_auto_compaction_notice(event.removed_message_count))
                    .or_else(|| {
                        Some(format!(
                            "turn complete · {} iteration(s)",
                            summary.iterations
                        ))
                    });
                (true, None, status)
            }
            Err(error) => (false, Some(error.to_string()), None),
        };

        // The runtime only exposes tool results on the TurnSummary (they are
        // not part of the per-call AssistantEvent stream), so push them into
        // the transcript now, before the final TurnEnd flushes the pending
        // assistant text.
        if let Ok(summary) = &result {
            for message in &summary.tool_results {
                for block in &message.blocks {
                    if let ContentBlock::ToolResult {
                        tool_name,
                        output,
                        is_error,
                        ..
                    } = block
                    {
                        let _ = ui_tx.send(UiEvent::ToolResult {
                            status: if *is_error {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Completed
                            },
                            output: format!("{tool_name}: {output}"),
                        });
                    }
                }
            }
        }

        let context_tokens = runtime.estimated_tokens();
        let cost_usd = match pricing_for_model(&model) {
            Some(pricing) => runtime
                .usage()
                .cumulative_usage()
                .estimate_cost_usd_with_pricing(pricing)
                .total_cost_usd(),
            None => 0.0,
        };

        let _ = ui_tx.send(UiEvent::ContextTokens(context_tokens));
        let _ = ui_tx.send(UiEvent::CostUsd(cost_usd));
        let _ = ui_tx.send(UiEvent::TurnEnd { ok, error, status });

        // Hand the finished runtime back to the UI thread so the session keeps
        // accumulating across turns. If the loop already exited, dropping the
        // runtime here still runs its shutdown (MCP servers, plugins).
        if turn_done_tx.send((runtime, hook_abort_monitor)).is_err() {
            // The UI loop exited; dropping ends the runtime's lifecycle.
        }
    });
}

// ---------------------------------------------------------------------------
// Event sink / permission prompter
// ---------------------------------------------------------------------------

struct TuiEventSink {
    tx: Sender<UiEvent>,
}

impl EventSink for TuiEventSink {
    fn on_event(&mut self, event: &AssistantEvent) -> bool {
        let ui_event = match event {
            AssistantEvent::TextDelta(delta) => UiEvent::Text(delta.clone()),
            AssistantEvent::ToolUse { name, input, .. } => UiEvent::ToolStart {
                name: name.clone(),
                input: input.clone(),
            },
            AssistantEvent::Usage(usage) => UiEvent::Usage(*usage),
            AssistantEvent::PromptCache(_) => return true,
            AssistantEvent::MessageStop => UiEvent::MessageStop,
        };
        self.tx.send(ui_event).is_ok()
    }
}

/// Permission prompter that renders the request in the transcript and blocks
/// until the UI thread routes the user's y/n decision through a per-prompt
/// channel. (Raw mode is owned by the UI thread, so the worker cannot read
/// stdin directly.)
struct TuiPermissionPrompter {
    current_mode: PermissionMode,
    tx: Sender<UiEvent>,
    rx: Receiver<char>,
}

impl PermissionPrompter for TuiPermissionPrompter {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        let _ = self.tx.send(UiEvent::PermissionRequest {
            tool: request.tool_name.clone(),
            reason: request.reason.clone(),
        });

        let choice = self.rx.recv().unwrap_or('n');
        match choice {
            'y' => PermissionPromptDecision::Allow,
            'n' => PermissionPromptDecision::Deny {
                reason: format!(
                    "denied by user (requested {} mode)",
                    request.required_mode.as_str()
                ),
            },
            _ => PermissionPromptDecision::Deny {
                reason: "permission prompt cancelled".to_string(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Key handling
// ---------------------------------------------------------------------------

/// Returns `true` when the TUI should exit.
fn handle_key(key: KeyEvent, state: &Arc<Mutex<TuiState>>, turn_tx: &Sender<String>) -> bool {
    let mut guard = lock(state);
    if guard.busy {
        return false;
    }

    // Shift+Enter requests a newline inside the draft; most terminals send
    // this as Ctrl+J, but also accept the explicit shift-modified Enter.
    if key.code == KeyCode::Enter
        && (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::SHIFT))
    {
        guard.insert_at_cursor("\n");
        guard.history_cursor = None;
        return false;
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if guard.draft.is_empty() {
                guard.status_line = Some("press /exit to quit (or Ctrl+D)".to_string());
            } else {
                guard.draft.clear();
                guard.cursor = 0;
            }
            false
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            guard.quit = true;
            guard.should_persist = true;
            true
        }
        KeyCode::Enter => {
            let trimmed = guard.draft.trim().to_string();
            if trimmed.is_empty() {
                return false;
            }
            guard.push_history(trimmed.clone());
            guard.draft.clear();
            guard.cursor = 0;
            guard.error = None;
            guard.busy = true;
            guard.status_line = Some("working…".to_string());
            if turn_tx.send(trimmed).is_err() {
                guard.busy = false;
                guard.error = Some("internal error: TUI event channel closed".to_string());
            }
            false
        }
        KeyCode::Backspace => {
            guard.backspace();
            guard.history_cursor = None;
            false
        }
        KeyCode::Left => {
            guard.cursor = guard.cursor.saturating_sub(1);
            false
        }
        KeyCode::Right => {
            guard.cursor = (guard.cursor + 1).min(guard.draft.chars().count());
            false
        }
        KeyCode::Home => {
            guard.cursor = 0;
            false
        }
        KeyCode::End => {
            guard.cursor = guard.draft.chars().count();
            false
        }
        KeyCode::Up => {
            let len = guard.history.len();
            if len > 0 {
                let position = match guard.history_cursor {
                    None => len - 1,
                    Some(current) => (current + len - 1) % len,
                };
                guard.history_cursor = Some(position);
                let entry = guard.history[position].clone();
                guard.draft = entry;
                guard.cursor = guard.draft.chars().count();
            }
            false
        }
        KeyCode::Down => {
            let len = guard.history.len();
            if let Some(current) = guard.history_cursor {
                let next = current + 1;
                if next >= len {
                    guard.history_cursor = None;
                    guard.draft.clear();
                    guard.cursor = 0;
                } else {
                    guard.history_cursor = Some(next);
                    let entry = guard.history[next].clone();
                    guard.draft = entry;
                    guard.cursor = guard.draft.chars().count();
                }
            }
            false
        }
        KeyCode::Tab => {
            guard.insert_at_cursor("    ");
            guard.history_cursor = None;
            false
        }
        KeyCode::Esc => {
            guard.draft.clear();
            guard.cursor = 0;
            guard.history_cursor = None;
            false
        }
        KeyCode::Char(character) => {
            guard.insert_at_cursor(&character.to_string());
            guard.history_cursor = None;
            false
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// UI events
// ---------------------------------------------------------------------------

enum UiEvent {
    Text(String),
    ToolStart {
        name: String,
        input: String,
    },
    ToolResult {
        status: ToolStatus,
        output: String,
    },
    Usage(code_runtime::TokenUsage),
    MessageStop,
    Output(String),
    PermissionRequest {
        tool: String,
        reason: Option<String>,
    },
    ContextTokens(usize),
    CostUsd(f64),
    TurnEnd {
        ok: bool,
        error: Option<String>,
        status: Option<String>,
    },
}

fn apply_ui_event(state: &Arc<Mutex<TuiState>>, event: UiEvent) {
    let mut guard = lock(state);
    match event {
        UiEvent::Text(delta) => guard.transcript.push_text(&delta),
        UiEvent::ToolStart { name, input } => guard.transcript.push_tool_start(&name, &input),
        UiEvent::ToolResult { status, output } => {
            guard.transcript.push_tool_result(status, &output);
        }
        UiEvent::Usage(usage) => {
            guard.cost_usd += usage
                .estimate_cost_usd_with_pricing(match pricing_for_model(&guard.model) {
                    Some(pricing) => pricing,
                    None => return,
                })
                .total_cost_usd();
        }
        UiEvent::MessageStop => guard.transcript.end_turn(),
        UiEvent::Output(output) => guard.transcript.push_output(&output),
        UiEvent::PermissionRequest { tool, reason } => {
            let reason = reason.unwrap_or_default();
            guard.transcript.push_output(&format!(
                "permission requested: {tool}\n  {reason}\n  [y] allow · [n] deny"
            ));
        }
        UiEvent::ContextTokens(tokens) => guard.context_tokens = tokens,
        UiEvent::CostUsd(cost) => guard.cost_usd = cost,
        UiEvent::TurnEnd { ok, error, status } => {
            guard.busy = false;
            let secs = guard
                .turn_started
                .map_or(0, |started| started.elapsed().as_secs());
            guard.turn_started = None;
            guard.status_line = status.or_else(|| {
                if ok {
                    Some(format!("done in {secs}s"))
                } else {
                    Some("turn failed".to_string())
                }
            });
            guard.error = error;
        }
    }
}

fn lock(state: &Arc<Mutex<TuiState>>) -> MutexGuard<'_, TuiState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn draw(terminal: &mut CrosstermTerminal, state: &Arc<Mutex<TuiState>>) {
    let snapshot = {
        let mut guard = lock(state);
        guard.frame_index = guard.frame_index.wrapping_add(1);
        guard.snapshot()
    };
    let _ = terminal.draw(|frame| render_frame(frame, &snapshot));
}

fn render_frame(frame: &mut Frame, snapshot: &Snapshot) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(frame, chunks[0], snapshot);
    render_transcript(frame, chunks[1], snapshot);
    render_status(frame, chunks[2], snapshot);
    render_composer(frame, chunks[3], snapshot);
}

fn render_header(frame: &mut Frame, area: ratatui::layout::Rect, snapshot: &Snapshot) {
    let title = format!("  Frontal Code · {} · {}", snapshot.model, snapshot.mode);
    frame.render_widget(
        Paragraph::new(title).style(
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn render_status(frame: &mut Frame, area: ratatui::layout::Rect, snapshot: &Snapshot) {
    let context = format_tokens(snapshot.context_tokens);
    let timer = if let Some(secs) = snapshot.turn_secs {
        format!(" {secs}s")
    } else {
        format!(
            " up {}m {:02}s",
            snapshot.session_secs / 60,
            snapshot.session_secs % 60
        )
    };
    let label = if let Some(status) = &snapshot.status_line {
        format!(
            "  {status} · {provider} · {mode} · {context} ctx · ${cost:.4}{timer}",
            provider = snapshot.provider,
            mode = snapshot.mode,
            cost = snapshot.cost_usd
        )
    } else if snapshot.busy {
        format!(
            "  {provider} · {mode} · {context} ctx · ${cost:.4}{timer} · working…",
            provider = snapshot.provider,
            mode = snapshot.mode,
            cost = snapshot.cost_usd
        )
    } else {
        format!(
            "  {provider} · {mode} · {context} ctx · ${cost:.4}{timer}",
            provider = snapshot.provider,
            mode = snapshot.mode,
            cost = snapshot.cost_usd
        )
    };
    let style = if snapshot.busy {
        Style::default().fg(Color::Reset)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(label).style(style), area);
}

fn render_composer(frame: &mut Frame, area: ratatui::layout::Rect, snapshot: &Snapshot) {
    let prompt_style = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Line::from(vec![
            Span::styled(" input ", prompt_style),
            Span::styled(
                "  ·  Enter to send · Shift+Enter for newline · /help · /exit to quit",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (left, right) = split_at_cursor(&snapshot.draft, snapshot.cursor);
    let line = Line::from(vec![
        Span::styled("> ", prompt_style),
        Span::raw(&left),
        Span::styled("█", prompt_style),
        Span::raw(&right),
    ]);
    frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: false }), inner);
}

fn render_transcript(frame: &mut Frame, area: ratatui::layout::Rect, snapshot: &Snapshot) {
    let mut lines: Vec<Line<'static>> = snapshot.transcript.lines.clone();

    // Live in-progress text (not yet ended by a MessageStop).
    if !snapshot.transcript.pending_text.is_empty() {
        for paragraph in snapshot.transcript.pending_text.split('\n') {
            lines.push(Line::from(Span::raw(paragraph.to_string())));
        }
    }

    if lines.is_empty() && !snapshot.busy {
        lines.push(Line::from(
            "  Ask me anything. Try /help for commands, /status for live context.",
        ));
    }

    if snapshot.busy {
        let glyph = braille_frame(snapshot.frame_index);
        let tool = snapshot
            .transcript
            .active_tool
            .clone()
            .unwrap_or_else(|| "thinking".to_string());
        lines.push(Line::from(Span::styled(
            format!("  {glyph} {tool}…"),
            Style::default().fg(Color::Blue),
        )));
    }

    if let Some(error) = &snapshot.error {
        lines.push(Line::from(Span::styled(
            format!("  ✗ {error}"),
            Style::default().fg(Color::Reset),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" conversation ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn split_at_cursor(draft: &str, cursor: usize) -> (String, String) {
    let mut left = String::new();
    let mut right = String::new();
    for (index, character) in draft.chars().enumerate() {
        if index < cursor {
            left.push(character);
        } else {
            right.push(character);
        }
    }
    (left, right)
}

fn braille_frame(index: usize) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[index % FRAMES.len()]
}

fn format_tokens(tokens: usize) -> String {
    if tokens >= 1_000 {
        // Token counts are bounded well below u32::MAX in practice.
        // SAFETY: tokens / 100 cannot exceed u32::MAX for any realistic
        // context window (would require > 4.3e12 tokens).
        let tenths_of_k = u32::try_from(tokens / 100).unwrap_or(u32::MAX);
        format!("{:.1}k", f64::from(tenths_of_k) / 10.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tokens_compacts_thousands() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_500), "1.5k");
        assert_eq!(format_tokens(24_000), "24.0k");
    }

    #[test]
    fn split_at_cursor_bounds() {
        assert_eq!(
            split_at_cursor("hello", 0),
            (String::new(), "hello".to_string())
        );
        assert_eq!(
            split_at_cursor("hello", 5),
            ("hello".to_string(), String::new())
        );
        assert_eq!(
            split_at_cursor("hello", 100),
            ("hello".to_string(), String::new())
        );
        assert_eq!(
            split_at_cursor("hello", 2),
            ("he".to_string(), "llo".to_string())
        );
    }

    #[test]
    fn braille_cycles_within_len() {
        for index in 0..20 {
            assert_ne!(braille_frame(index), "");
        }
    }
}
