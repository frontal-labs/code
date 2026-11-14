//! Full-screen TUI for the interactive REPL (`frontal-code --tui`).
//!
//! The TUI shows the same interface as the line-based REPL. The conversation
//! scrolls above a frame drawn like the REPL's: a rule, the `❯` input row, a
//! rule, and the footer (model, access, branch, mode, and context). Banner,
//! tool headlines, tool results, turn summaries, shortcuts, and permission
//! prompts are the REPL's own strings, rendered through [`ansi`].
//!
//! Architecture: a single UI thread owns the terminal. Turns run on a worker
//! thread through the same runtime entry points the REPL uses
//! (`prepare_turn_runtime` + `run_turn_stream`). The worker ships [`UiEvent`]s
//! over an `mpsc` channel, including each tool result as it completes, and the
//! UI thread applies them to shared [`TuiState`]. Keys that need the CLI
//! (permission mode, completion, clearing) become [`Action`]s the UI thread
//! applies between frames.

mod ansi;
mod transcript;

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use rustyline::history::{DefaultHistory, History};

use code_runtime::{
    pricing_for_model, AssistantEvent, EventSink, PermissionMode, PermissionPromptDecision,
    PermissionPrompter, PermissionRequest, TokenUsage,
};

use crate::tui::transcript::Transcript;
use crate::{
    collect_tool_uses, format_auto_compaction_notice, format_turn_summary, mentions, LiveCli,
    SlashCommand,
};

type CrosstermTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// A second Ctrl-C within this window exits, as in the REPL.
const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);
/// The theme's spinner blue, as in the REPL.
const SPINNER_COLOR: Color = Color::Indexed(33);
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_COMPLETION_LIST: usize = 12;

/// Runs the full-screen TUI on `cli` and returns when the user exits.
///
/// `run_slash_command` handles one parsed slash command, writing its output into
/// `buffer`, so this module never reaches into `LiveCli`'s private fields.
pub(crate) fn run<F>(
    cli: &mut LiveCli,
    run_slash_command: &mut F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&mut LiveCli, SlashCommand, &mut Vec<u8>) -> Result<bool, Box<dyn std::error::Error>>,
{
    if !terminal_supported() {
        return Err(
            "the full-screen TUI requires a real terminal (try plain `frontal-code` instead)"
                .into(),
        );
    }

    let mut terminal = prepare_terminal()?;
    let state = Arc::new(Mutex::new(TuiState::new(cli)));
    lock(&state).transcript.push_ansi(&cli.startup_banner());
    let result = run_loop(cli, &mut terminal, &state, run_slash_command);
    teardown_terminal(&mut terminal);
    let should_persist = lock(&state).should_persist;
    if should_persist {
        let _ = cli.persist_session();
    }
    result
}

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

/// A permission request waiting for the user's y, n, or a.
struct PendingPermission {
    tool: String,
}

/// Work that needs the CLI, requested by a key and applied by the UI loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    CyclePermission,
    ClearScreen,
    Complete,
}

struct TuiState {
    transcript: Transcript,
    footer: String,
    draft: String,
    cursor: usize,
    history: Vec<String>,
    history_cursor: Option<usize>,
    history_path: Option<PathBuf>,
    completions: Vec<String>,
    mention_paths: Vec<String>,
    busy: bool,
    turn_started: Option<Instant>,
    frame_index: usize,
    permission: Option<PendingPermission>,
    permission_tx: Option<Sender<char>>,
    ctrl_c_armed: Option<Instant>,
    pending_action: Option<Action>,
    quit: bool,
    should_persist: bool,
}

impl TuiState {
    fn new(cli: &LiveCli) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let history_path = crate::ConfigLoader::default_for(&cwd)
            .config_home()
            .join("repl_history");
        let mut history = DefaultHistory::new();
        let _ = history.load(&history_path);
        let mut state = Self {
            transcript: Transcript::default(),
            footer: String::new(),
            draft: String::new(),
            cursor: 0,
            history: history.iter().cloned().collect(),
            history_cursor: None,
            history_path: Some(history_path),
            completions: Vec::new(),
            mention_paths: Vec::new(),
            busy: false,
            turn_started: None,
            frame_index: 0,
            permission: None,
            permission_tx: None,
            ctrl_c_armed: None,
            pending_action: None,
            quit: false,
            should_persist: false,
        };
        state.refresh_from_cli(cli);
        state
    }

    /// Recomputes the values derived from the CLI: footer, completions, and mentions.
    fn refresh_from_cli(&mut self, cli: &LiveCli) {
        self.footer = cli.input_footer();
        self.completions = cli.repl_completion_candidates().unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_default();
        self.mention_paths = mentions::workspace_files(&cwd);
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

    fn set_draft(&mut self, text: String) {
        self.cursor = text.chars().count();
        self.draft = text;
    }

    /// The grey completion of the draft from history, shown when the cursor is at the end.
    fn history_suggestion(&self) -> Option<String> {
        if self.draft.is_empty() || self.cursor != self.draft.chars().count() {
            return None;
        }
        self.history
            .iter()
            .rev()
            .find(|entry| entry.len() > self.draft.len() && entry.starts_with(&self.draft))
            .map(|entry| entry[self.draft.len()..].to_string())
    }

    fn push_history(&mut self, line: String) {
        if let Some(position) = self.history_cursor {
            self.history.insert(position, line);
        } else {
            self.history.push(line);
        }
        self.history_cursor = None;
        self.save_history();
    }

    /// Writes history in the same file the line-based REPL uses.
    fn save_history(&self) {
        let Some(path) = &self.history_path else {
            return;
        };
        let mut history = DefaultHistory::new();
        for entry in &self.history {
            let _ = history.add(entry);
        }
        let _ = history.save(path);
    }
}

/// A point-in-time copy of [`TuiState`] handed to the draw pass so no lock is
/// held while ratatui writes to the terminal.
#[derive(Debug, Clone)]
struct Snapshot {
    footer: String,
    draft: String,
    cursor: usize,
    suggestion: Option<String>,
    busy: bool,
    frame_index: usize,
    turn_secs: u64,
    permission_tool: Option<String>,
    transcript: transcript::TranscriptSnapshot,
}

impl TuiState {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            footer: self.footer.clone(),
            draft: self.draft.clone(),
            cursor: self.cursor,
            suggestion: self.history_suggestion(),
            busy: self.busy,
            frame_index: self.frame_index,
            turn_secs: self
                .turn_started
                .map_or(0, |started| started.elapsed().as_secs()),
            permission_tool: self.permission.as_ref().map(|prompt| prompt.tool.clone()),
            transcript: self.transcript.snapshot(),
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
    F: FnMut(&mut LiveCli, SlashCommand, &mut Vec<u8>) -> Result<bool, Box<dyn std::error::Error>>,
{
    let (ui_tx, ui_rx) = channel::<UiEvent>();
    let (turn_tx, turn_rx) = channel::<String>();
    let (turn_done_tx, turn_done_rx) = channel::<(crate::BuiltRuntime, crate::HookAbortMonitor)>();

    loop {
        // Apply events from the turn worker.
        while let Ok(event) = ui_rx.try_recv() {
            apply_ui_event(state, event);
        }

        // Dispatch a submitted line once no turn is running.
        if !lock(state).busy {
            if let Ok(line) = turn_rx.try_recv() {
                dispatch_line(cli, state, &ui_tx, &turn_done_tx, line, run_slash_command);
            }
        }

        // Reinstall a finished turn's runtime and refresh what depends on it.
        if let Ok((runtime, _hook_abort_monitor)) = turn_done_rx.try_recv() {
            match cli.replace_runtime(runtime) {
                Ok(()) => {}
                Err(error) => {
                    let mut guard = lock(state);
                    guard.transcript.push_ansi(&format!(
                        "error: failed to update session after turn: {error}"
                    ));
                }
            }
            lock(state).refresh_from_cli(cli);
        }

        let pending = lock(state).pending_action.take();
        if let Some(action) = pending {
            apply_action(cli, state, action);
        }

        if lock(state).quit {
            break;
        }

        draw(terminal, state);

        match crossterm::event::poll(Duration::from_millis(80)) {
            Ok(true) => match crossterm::event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if handle_key(key, state, &turn_tx) {
                        break;
                    }
                }
                Ok(_) => {}
                Err(error) => eprintln!("warning: failed to read terminal event: {error}"),
            },
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => eprintln!("warning: failed to poll terminal events: {error}"),
        }
    }

    // A turn still running at exit finishes first, so the session stays consistent.
    if lock(state).busy {
        while let Ok(event) = ui_rx.recv_timeout(Duration::from_millis(50)) {
            apply_ui_event(state, event);
            if !lock(state).busy {
                break;
            }
        }
        if let Ok((runtime, _hook_abort_monitor)) = turn_done_rx.recv() {
            let _ = cli.replace_runtime(runtime);
        }
    }

    Ok(())
}

/// Runs a submitted line: `/exit` quits, `!` runs a shell command, slash commands run
/// inline with their output captured, and everything else starts a turn.
fn dispatch_line<F>(
    cli: &mut LiveCli,
    state: &Arc<Mutex<TuiState>>,
    ui_tx: &Sender<UiEvent>,
    turn_done_tx: &Sender<(crate::BuiltRuntime, crate::HookAbortMonitor)>,
    line: String,
    run_slash_command: &mut F,
) where
    F: FnMut(&mut LiveCli, SlashCommand, &mut Vec<u8>) -> Result<bool, Box<dyn std::error::Error>>,
{
    let trimmed = line.trim().to_string();

    if matches!(trimmed.as_str(), "/exit" | "/quit") {
        let mut guard = lock(state);
        guard.quit = true;
        guard.should_persist = true;
        return;
    }

    if let Some(command) = trimmed.strip_prefix('!') {
        let mut guard = lock(state);
        guard.transcript.push_user(&trimmed);
        let output = capture_shell_command(command.trim());
        guard.transcript.push_ansi(&output);
        return;
    }

    let parsed = match SlashCommand::parse(&trimmed) {
        Ok(parsed) => parsed,
        Err(error) => {
            lock(state).transcript.push_ansi(&format!("{error}"));
            return;
        }
    };

    if let Some(command) = parsed {
        let mut buffer: Vec<u8> = Vec::new();
        let outcome = run_slash_command(cli, command, &mut buffer);
        let mut guard = lock(state);
        match outcome {
            Ok(handled) => {
                if !buffer.is_empty() {
                    guard
                        .transcript
                        .push_ansi(&String::from_utf8_lossy(&buffer));
                }
                if handled {
                    guard.refresh_from_cli(cli);
                }
            }
            Err(error) => guard.transcript.push_ansi(&format!("error: {error}")),
        }
        return;
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let (prompt, attached) = mentions::expand(&trimmed, &cwd);
    if !attached.is_empty() {
        lock(state)
            .transcript
            .push_ansi(&format!("\x1b[2mAttached {}\x1b[0m", attached.join(", ")));
    }
    start_turn(
        cli,
        state,
        trimmed,
        prompt,
        ui_tx.clone(),
        turn_done_tx.clone(),
    );
}

/// Runs a shell command and returns its output as text for the transcript.
fn capture_shell_command(command: &str) -> String {
    if command.is_empty() {
        return "usage: !<command>".to_string();
    }
    match ProcessCommand::new("sh").arg("-c").arg(command).output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            if !output.status.success() {
                let code = output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_string(), |code| code.to_string());
                let _ = write!(text, "\x1b[2mexit {code}\x1b[0m");
            }
            text
        }
        Err(error) => format!("error: {error}"),
    }
}

/// Starts a streaming turn on a worker thread. The worker owns a freshly built runtime
/// and ships UI events and the finished runtime back to the UI loop.
fn start_turn(
    cli: &mut LiveCli,
    state: &Arc<Mutex<TuiState>>,
    display: String,
    prompt: String,
    ui_tx: Sender<UiEvent>,
    turn_done_tx: Sender<(crate::BuiltRuntime, crate::HookAbortMonitor)>,
) {
    let (runtime, hook_abort_monitor) = match cli.prepare_turn_runtime(false) {
        Ok(pair) => pair,
        Err(error) => {
            lock(state).transcript.push_ansi(&format!("error: {error}"));
            return;
        }
    };

    let model = cli.model().to_string();
    let permission_mode = cli.permission_mode();
    let (permission_tx, permission_rx) = channel::<char>();

    {
        let mut guard = lock(state);
        guard.transcript.push_user(&display);
        guard.busy = true;
        guard.turn_started = Some(Instant::now());
        guard.permission_tx = Some(permission_tx);
    }

    std::thread::spawn(move || {
        let mut runtime = runtime;
        let started = Instant::now();

        // Each tool result reaches the transcript as soon as it completes.
        let observer_tx = ui_tx.clone();
        crate::set_tool_result_observer(move |name, output, is_error| {
            let _ = observer_tx.send(UiEvent::ToolResult {
                name: name.to_string(),
                output: output.to_string(),
                is_error,
            });
        });

        let mut prompter = TuiPermissionPrompter {
            current_mode: permission_mode,
            tx: ui_tx.clone(),
            rx: permission_rx,
        };
        let mut sink = TuiEventSink { tx: ui_tx.clone() };
        let result = runtime.run_turn_stream(&prompt, Some(&mut prompter), &mut sink);
        crate::clear_tool_result_observer();

        match &result {
            Ok(summary) => {
                let tool_calls = collect_tool_uses(summary).len();
                let _ = ui_tx.send(UiEvent::Notice(format_turn_summary(
                    summary,
                    &model,
                    started.elapsed(),
                    tool_calls,
                )));
                if let Some(event) = summary.auto_compaction {
                    let _ = ui_tx.send(UiEvent::Notice(format_auto_compaction_notice(
                        event.removed_message_count,
                    )));
                }
                let _ = ui_tx.send(UiEvent::TurnEnd { error: None });
            }
            Err(error) => {
                let _ = ui_tx.send(UiEvent::TurnEnd {
                    error: Some(error.to_string()),
                });
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

        // Hand the runtime back so the session keeps accumulating across turns.
        let _ = turn_done_tx.send((runtime, hook_abort_monitor));
    });
}

// ---------------------------------------------------------------------------
// Actions that need the CLI
// ---------------------------------------------------------------------------

fn apply_action(cli: &mut LiveCli, state: &Arc<Mutex<TuiState>>, action: Action) {
    match action {
        Action::CyclePermission => {
            let next = crate::next_permission_mode(cli.permission_mode().as_str());
            match cli.switch_permission_mode(next) {
                Ok(previous) => {
                    let mut guard = lock(state);
                    guard.transcript.push_ansi(&format!(
                        "\x1b[2mPermission mode: {previous} -> {next}\x1b[0m"
                    ));
                    guard.refresh_from_cli(cli);
                }
                Err(error) => lock(state).transcript.push_ansi(&format!("error: {error}")),
            }
        }
        Action::ClearScreen => {
            let mut guard = lock(state);
            guard.transcript.clear();
            guard.transcript.push_ansi(&cli.startup_banner());
        }
        Action::Complete => complete_draft(state),
    }
}

/// Completes the word at the cursor: a slash command for a leading `/`, or a workspace
/// path for `@`. A single match is inserted; several matches insert their common prefix
/// and are listed.
fn complete_draft(state: &Arc<Mutex<TuiState>>) {
    let mut guard = lock(state);
    let draft = guard.draft.clone();

    let (start, candidates): (usize, Vec<String>) =
        if draft.starts_with('/') && !draft.contains(' ') {
            (
                0,
                guard
                    .completions
                    .iter()
                    .filter(|candidate| candidate.starts_with(&draft))
                    .cloned()
                    .collect(),
            )
        } else {
            let start = draft
                .rfind(char::is_whitespace)
                .map_or(0, |index| index + 1);
            let word = &draft[start..];
            let Some(prefix) = word.strip_prefix('@') else {
                return;
            };
            (
                start,
                guard
                    .mention_paths
                    .iter()
                    .filter(|path| path.starts_with(prefix))
                    .map(|path| format!("@{path}"))
                    .collect(),
            )
        };

    if candidates.is_empty() {
        return;
    }
    let current = &draft[start..];
    let replacement = if candidates.len() == 1 {
        candidates[0].clone()
    } else {
        let common = common_prefix(&candidates);
        if common.len() <= current.len() {
            if candidates.len() > MAX_COMPLETION_LIST {
                guard.transcript.push_ansi(&format!(
                    "\x1b[2m{} matches, keep typing to narrow them\x1b[0m",
                    candidates.len()
                ));
            } else {
                guard
                    .transcript
                    .push_ansi(&format!("\x1b[2m{}\x1b[0m", candidates.join("  ")));
            }
            return;
        }
        common
    };
    let updated = format!("{}{}", &draft[..start], replacement);
    guard.set_draft(updated);
}

fn common_prefix(values: &[String]) -> String {
    let Some(first) = values.first() else {
        return String::new();
    };
    let mut prefix = first.clone();
    for value in &values[1..] {
        while !value.starts_with(&prefix) {
            prefix.pop();
        }
    }
    prefix
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

/// Permission prompter that shows the REPL's prompt in the transcript and waits for the
/// user's choice. The UI thread routes the key through a per-turn channel, so the worker
/// never reads the terminal itself.
struct TuiPermissionPrompter {
    current_mode: PermissionMode,
    tx: Sender<UiEvent>,
    rx: Receiver<char>,
}

impl PermissionPrompter for TuiPermissionPrompter {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        let always_allowed = crate::ALWAYS_ALLOWED_TOOLS
            .lock()
            .is_ok_and(|set| set.contains(&request.tool_name));
        if always_allowed {
            return PermissionPromptDecision::Allow;
        }

        let _ = self.tx.send(UiEvent::PermissionRequest {
            tool: request.tool_name.clone(),
            required_mode: request.required_mode.as_str().to_string(),
            current_mode: self.current_mode.as_str().to_string(),
            reason: request.reason.clone(),
            input: request.input.clone(),
        });

        match self.rx.recv().unwrap_or('n') {
            'y' => PermissionPromptDecision::Allow,
            'a' => {
                if let Ok(mut set) = crate::ALWAYS_ALLOWED_TOOLS.lock() {
                    set.insert(request.tool_name.clone());
                }
                PermissionPromptDecision::Allow
            }
            _ => PermissionPromptDecision::Deny {
                reason: format!(
                    "tool '{}' denied by user approval prompt",
                    request.tool_name
                ),
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
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // A pending permission prompt takes every key first, even during a turn.
    if let Some(prompt) = guard.permission.take() {
        let choice = match key.code {
            KeyCode::Char('y' | 'Y') => Some('y'),
            KeyCode::Char('a' | 'A') => Some('a'),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => Some('n'),
            KeyCode::Char('c') if ctrl => Some('n'),
            _ => None,
        };
        match choice {
            Some(choice) => {
                if let Some(tx) = guard.permission_tx.take() {
                    let _ = tx.send(choice);
                }
                let decision = match choice {
                    'y' => "allowed once",
                    'a' => "allowed for this session",
                    _ => "denied",
                };
                guard
                    .transcript
                    .push_ansi(&format!("\x1b[2m  {} {decision}\x1b[0m", prompt.tool));
            }
            None => guard.permission = Some(prompt),
        }
        return false;
    }

    // Ctrl-C clears the draft; on an empty prompt it exits on the second press.
    if key.code == KeyCode::Char('c') && ctrl {
        if !guard.draft.is_empty() {
            guard.draft.clear();
            guard.cursor = 0;
            guard.history_cursor = None;
            guard.ctrl_c_armed = None;
            return false;
        }
        let armed = guard
            .ctrl_c_armed
            .is_some_and(|at| at.elapsed() < EXIT_CONFIRM_WINDOW);
        if armed {
            guard.quit = true;
            guard.should_persist = true;
            return true;
        }
        guard.ctrl_c_armed = Some(Instant::now());
        guard
            .transcript
            .push_ansi("\x1b[2mPress Ctrl-C again to exit\x1b[0m");
        return false;
    }

    if key.code == KeyCode::Char('d') && ctrl {
        if guard.draft.is_empty() && !guard.busy {
            guard.quit = true;
            guard.should_persist = true;
            return true;
        }
        return false;
    }

    if guard.busy {
        return false;
    }

    // Enter sends; Alt+Enter, Ctrl+J, and Shift+Enter insert a newline.
    if key.code == KeyCode::Enter {
        if ctrl
            || key.modifiers.contains(KeyModifiers::SHIFT)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            guard.insert_at_cursor("\n");
            guard.history_cursor = None;
            return false;
        }
        let trimmed = guard.draft.trim().to_string();
        if trimmed.is_empty() {
            return false;
        }
        guard.push_history(trimmed.clone());
        guard.draft.clear();
        guard.cursor = 0;
        guard.ctrl_c_armed = None;
        if turn_tx.send(trimmed).is_err() {
            guard
                .transcript
                .push_ansi("error: internal error: TUI event channel closed");
        }
        return false;
    }

    if key.code == KeyCode::Char('l') && ctrl {
        guard.pending_action = Some(Action::ClearScreen);
        return false;
    }

    match key.code {
        KeyCode::BackTab => guard.pending_action = Some(Action::CyclePermission),
        KeyCode::Tab => {
            if guard.draft.starts_with('/') || guard.draft.contains('@') {
                guard.pending_action = Some(Action::Complete);
            }
        }
        KeyCode::Char('?') if guard.draft.is_empty() => {
            guard.transcript.push_ansi(&crate::render_shortcuts());
        }
        KeyCode::Backspace => {
            guard.backspace();
            guard.history_cursor = None;
        }
        KeyCode::Left => guard.cursor = guard.cursor.saturating_sub(1),
        KeyCode::Right => {
            if let Some(suggestion) = guard.history_suggestion() {
                guard.insert_at_cursor(&suggestion);
            } else {
                guard.cursor = (guard.cursor + 1).min(guard.draft.chars().count());
            }
        }
        KeyCode::Home => guard.cursor = 0,
        KeyCode::End => guard.cursor = guard.draft.chars().count(),
        KeyCode::Up => {
            let len = guard.history.len();
            if len > 0 {
                let position = match guard.history_cursor {
                    None => len - 1,
                    Some(current) => current.saturating_sub(1),
                };
                guard.history_cursor = Some(position);
                let entry = guard.history[position].clone();
                guard.set_draft(entry);
            }
        }
        KeyCode::Down => {
            if let Some(current) = guard.history_cursor {
                let next = current + 1;
                if next >= guard.history.len() {
                    guard.history_cursor = None;
                    guard.set_draft(String::new());
                } else {
                    guard.history_cursor = Some(next);
                    let entry = guard.history[next].clone();
                    guard.set_draft(entry);
                }
            }
        }
        KeyCode::Esc => {
            guard.draft.clear();
            guard.cursor = 0;
            guard.history_cursor = None;
        }
        KeyCode::Char(character) if !ctrl => {
            guard.insert_at_cursor(&character.to_string());
            guard.history_cursor = None;
        }
        _ => {}
    }
    false
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
        name: String,
        output: String,
        is_error: bool,
    },
    Usage(TokenUsage),
    MessageStop,
    Notice(String),
    PermissionRequest {
        tool: String,
        required_mode: String,
        current_mode: String,
        reason: Option<String>,
        input: String,
    },
    ContextTokens(usize),
    CostUsd(f64),
    TurnEnd {
        error: Option<String>,
    },
}

fn apply_ui_event(state: &Arc<Mutex<TuiState>>, event: UiEvent) {
    let mut guard = lock(state);
    match event {
        UiEvent::Text(delta) => guard.transcript.push_text(&delta),
        UiEvent::ToolStart { name, input } => guard.transcript.push_tool_start(&name, &input),
        UiEvent::ToolResult {
            name,
            output,
            is_error,
        } => guard.transcript.push_tool_result(&name, &output, is_error),
        UiEvent::MessageStop => guard.transcript.end_turn(),
        UiEvent::Notice(text) => {
            guard.transcript.end_turn();
            guard.transcript.push_ansi(&text);
        }
        UiEvent::PermissionRequest {
            tool,
            required_mode,
            current_mode,
            reason,
            input,
        } => {
            guard.transcript.flush_text();
            let mut text = format!(
                "\n\x1b[1m{tool}\x1b[0m needs {required_mode} (current mode: {current_mode})\n"
            );
            if let Some(reason) = reason {
                let _ = writeln!(text, "  \x1b[2m{reason}\x1b[0m");
            }
            let _ = write!(
                text,
                "  \x1b[2m{}\x1b[0m",
                crate::truncate_for_summary(&input, 160)
            );
            guard.transcript.push_ansi(&text);
            guard.permission = Some(PendingPermission { tool });
        }
        UiEvent::Usage(_) | UiEvent::ContextTokens(_) | UiEvent::CostUsd(_) => {}
        UiEvent::TurnEnd { error } => {
            guard.transcript.end_turn();
            if let Some(error) = error {
                guard.transcript.push_ansi(&format!("error: {error}"));
            }
            guard.busy = false;
            guard.turn_started = None;
            guard.permission = None;
            guard.permission_tx = None;
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

/// The composer is the REPL's frame: rule, input rows, rule, footer.
fn composer_height(snapshot: &Snapshot) -> u16 {
    let input_rows = if snapshot.permission_tool.is_some() {
        1
    } else {
        snapshot.draft.split('\n').count().clamp(1, 6)
    };
    let input_rows = u16::try_from(input_rows).unwrap_or(1);
    input_rows + 3
}

fn render_frame(frame: &mut Frame, snapshot: &Snapshot) {
    let area = frame.area();
    let composer = composer_height(snapshot).min(area.height.saturating_sub(1));
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(composer)]).split(area);
    render_transcript(frame, chunks[0], snapshot);
    render_composer(frame, chunks[1], snapshot);
}

fn render_transcript(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    let mut lines: Vec<Line<'static>> = snapshot.transcript.lines.clone();
    if !snapshot.transcript.pending_text.is_empty() {
        for paragraph in snapshot.transcript.pending_text.split('\n') {
            lines.push(Line::from(Span::raw(paragraph.to_string())));
        }
    }
    if snapshot.busy && snapshot.permission_tool.is_none() {
        let glyph = SPINNER_FRAMES[snapshot.frame_index % SPINNER_FRAMES.len()];
        lines.push(Line::from(Span::styled(
            format!("{glyph} Thinking... {}s", snapshot.turn_secs),
            Style::default().fg(SPINNER_COLOR),
        )));
    }

    // Keep the newest lines in view: estimate wrapped height per line.
    let width = usize::from(area.width.max(1));
    let total: usize = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum();
    let offset = total.saturating_sub(usize::from(area.height));
    let offset = u16::try_from(offset).unwrap_or(u16::MAX);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((offset, 0)),
        area,
    );
}

fn render_composer(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    let rule = Line::from(Span::styled(
        "─".repeat(usize::from(area.width)),
        Style::default().fg(Color::DarkGray),
    ));
    let mut lines = vec![rule.clone()];
    let mut cursor_position = None;

    if let Some(tool) = &snapshot.permission_tool {
        lines.push(Line::from(format!(
            "Allow?  [y] yes   [n] no   [a] always for {tool}  > "
        )));
    } else if snapshot.draft.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("❯ "),
            Span::styled(
                crate::input::PLACEHOLDER.to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        cursor_position = Some((2, 0));
    } else {
        let (before, _after) = split_at_cursor(&snapshot.draft, snapshot.cursor);
        let total_rows = snapshot.draft.split('\n').count();
        for (index, text) in snapshot.draft.split('\n').enumerate() {
            let prefix = if index == 0 { "❯ " } else { "  " };
            let mut spans = vec![Span::raw(prefix), Span::raw(text.to_string())];
            if index == total_rows - 1 {
                if let Some(suggestion) = &snapshot.suggestion {
                    spans.push(Span::styled(
                        suggestion.clone(),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            lines.push(Line::from(spans));
        }
        let cursor_row = before.matches('\n').count();
        let cursor_col = before
            .rsplit('\n')
            .next()
            .map_or(0, |line| line.chars().count());
        cursor_position = Some((2 + cursor_col, cursor_row));
    }

    lines.push(rule);
    let footer_line = ansi::lines(&snapshot.footer)
        .into_iter()
        .next()
        .unwrap_or_default();
    lines.push(footer_line);

    frame.render_widget(Paragraph::new(lines), area);

    if snapshot.permission_tool.is_none() {
        if let Some((column, row)) = cursor_position {
            let x = area.x.saturating_add(u16::try_from(column).unwrap_or(0));
            let y = area
                .y
                .saturating_add(1)
                .saturating_add(u16::try_from(row).unwrap_or(0));
            if x < area.right() && y < area.bottom() {
                frame.set_cursor_position((x, y));
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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

    fn frame_text(snapshot: &Snapshot, width: u16, height: u16) -> Vec<String> {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| render_frame(frame, snapshot))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn snapshot_with(draft: &str, permission_tool: Option<&str>) -> Snapshot {
        let mut transcript = Transcript::default();
        transcript.push_ansi("\x1b[1mFrontal Code\x1b[0m \x1b[90mv0.3.0\x1b[0m");
        Snapshot {
            footer: "\x1b[90mFrontal Code · \x1b[0mclaude-sonnet-4-6\x1b[90m · API key · branch main\x1b[0m"
                .to_string(),
            draft: draft.to_string(),
            cursor: draft.chars().count(),
            suggestion: None,
            busy: false,
            frame_index: 0,
            turn_secs: 0,
            permission_tool: permission_tool.map(str::to_string),
            transcript: transcript.snapshot(),
        }
    }

    #[test]
    fn composer_uses_the_repl_frame() {
        let lines = frame_text(&snapshot_with("", None), 70, 8);
        let bottom = lines.len() - 1;
        assert!(lines[bottom].starts_with("Frontal Code · claude-sonnet-4-6"));
        assert!(lines[bottom - 1].starts_with('─'));
        assert!(lines[bottom - 2].starts_with("❯ Ask Frontal Code"));
        assert!(lines[bottom - 3].starts_with('─'));
    }

    #[test]
    fn composer_shows_the_typed_draft_and_permission_prompt() {
        let typed = frame_text(&snapshot_with("explain this", None), 70, 8);
        assert!(typed.iter().any(|line| line.starts_with("❯ explain this")));

        let prompt = frame_text(&snapshot_with("", Some("write_file")), 90, 8);
        assert!(prompt
            .iter()
            .any(|line| line.contains("Allow?  [y] yes   [n] no   [a] always for write_file")));
    }

    #[test]
    fn transcript_shows_the_banner_above_the_frame() {
        let lines = frame_text(&snapshot_with("", None), 70, 8);
        assert!(lines[0].contains("Frontal Code v0.3.0"));
    }

    fn bare_state(draft: &str, completions: &[&str], mentions: &[&str]) -> Arc<Mutex<TuiState>> {
        Arc::new(Mutex::new(TuiState {
            transcript: Transcript::default(),
            footer: String::new(),
            draft: draft.to_string(),
            cursor: draft.chars().count(),
            history: Vec::new(),
            history_cursor: None,
            history_path: None,
            completions: completions.iter().map(ToString::to_string).collect(),
            mention_paths: mentions.iter().map(ToString::to_string).collect(),
            busy: false,
            turn_started: None,
            frame_index: 0,
            permission: None,
            permission_tx: None,
            ctrl_c_armed: None,
            pending_action: None,
            quit: false,
            should_persist: false,
        }))
    }

    #[test]
    fn tab_completes_slash_commands_and_workspace_paths() {
        let state = bare_state("/mo", &["/model", "/model opus", "/status"], &[]);
        complete_draft(&state);
        assert_eq!(lock(&state).draft, "/model");

        let state = bare_state("look at @src/ma", &[], &["src/main.rs", "README.md"]);
        complete_draft(&state);
        assert_eq!(lock(&state).draft, "look at @src/main.rs");
    }

    #[test]
    fn tab_lists_several_matches_without_changing_the_draft() {
        let state = bare_state("/mo", &["/model opus", "/model sonnet"], &[]);
        complete_draft(&state);
        assert_eq!(lock(&state).draft, "/model ");
    }

    #[test]
    fn history_suggestion_is_the_rest_of_the_latest_match() {
        let state = bare_state("!ech", &[], &[]);
        lock(&state).history = vec!["!echo one".to_string(), "/help".to_string()];
        assert_eq!(lock(&state).history_suggestion().as_deref(), Some("o one"));
    }

    #[test]
    fn common_prefix_of_candidates() {
        let candidates = vec!["/model opus".to_string(), "/model sonnet".to_string()];
        assert_eq!(common_prefix(&candidates), "/model ");
        assert_eq!(common_prefix(&[]), "");
    }
}
