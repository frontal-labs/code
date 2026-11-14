//! Full-screen TUI: the default interactive interface for `frontal-code`.
//!
//! Layout, top to bottom: a top bar with the app name and buttons (model, mode, trace,
//! help); the conversation, scrollable, with an optional trace panel beside it; a loader
//! line while a turn runs; and the composer, drawn like the REPL's frame (rule, `❯` input,
//! rule, footer).
//!
//! Architecture: the UI thread owns the terminal and [`TuiState`]. Turns run on a worker
//! thread through the runtime's streaming entry point; the worker sends [`UiEvent`]s, and
//! the UI thread applies them. Keys that need the CLI become [`Action`]s, and selector
//! choices become lines sent through the same channel as typed input.

mod ansi;
mod diff;
mod highlight;
mod markdown;
mod transcript;
mod widgets;

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::{Frame, Terminal};
use rustyline::history::{DefaultHistory, History};

use code_runtime::{
    pricing_for_model, AssistantEvent, EventSink, PermissionMode, PermissionPromptDecision,
    PermissionPrompter, PermissionRequest, TokenUsage,
};

use self::transcript::{Entry, Transcript};
use self::widgets::{Item, Purpose, Selector};
use crate::{
    collect_tool_uses, format_auto_compaction_notice, format_turn_summary, mentions, LiveCli,
    SlashCommand,
};

type CrosstermTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// A second Ctrl-C within this window exits, as in the REPL.
const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);
const TRACE_PANEL_WIDTH: u16 = 44;
const SCROLL_STEP: usize = 3;
const MAX_TRACES: usize = 500;
const BUTTONS: [Button; 5] = [
    Button::Model,
    Button::Mode,
    Button::Sessions,
    Button::Trace,
    Button::Help,
];

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
            "the full-screen TUI requires a real terminal (use `frontal-code --repl` instead)"
                .into(),
        );
    }

    let mut terminal = prepare_terminal()?;
    let mut state = TuiState::new(cli);
    let banner = cli.startup_banner();
    state.transcript.push(Entry::Banner(banner));
    let result = run_loop(cli, &mut terminal, &mut state, run_slash_command);
    teardown_terminal(&mut terminal);
    if state.should_persist {
        let _ = cli.persist_session();
    }
    result
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Which region receives keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Composer,
    Transcript,
    Buttons,
}

/// A top-bar button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Model,
    Mode,
    Sessions,
    Trace,
    Help,
}

impl Button {
    fn label(self, mode: &str) -> String {
        match self {
            Self::Model => "Model ▾".to_string(),
            Self::Mode => format!("Mode: {mode} ▾"),
            Self::Sessions => "Sessions ▾".to_string(),
            Self::Trace => "Trace".to_string(),
            Self::Help => "Help".to_string(),
        }
    }
}

/// What the loader says the turn is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Waiting,
    Streaming,
    Tool(String),
    Approval(String),
}

impl Phase {
    fn detail(&self) -> String {
        match self {
            Self::Idle => String::new(),
            Self::Waiting => "waiting for the model".to_string(),
            Self::Streaming => "streaming".to_string(),
            Self::Tool(name) => format!("running {name}"),
            Self::Approval(tool) => format!("waiting for approval to run {tool}"),
        }
    }
}

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

/// Where things were drawn on the last frame, for mouse hit-testing.
#[derive(Debug, Default, Clone)]
struct Regions {
    transcript: Rect,
    composer: Rect,
    buttons: Vec<(Rect, Button)>,
    /// Wrapped-line range of each entry in the transcript.
    entry_ranges: Vec<(usize, usize)>,
}

/// All UI state for one session. Several flags are independent toggles, so a struct of
/// bools is the clearest shape here.
#[allow(clippy::struct_excessive_bools)]
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
    model: String,
    mode: String,
    busy: bool,
    turn_started: Option<Instant>,
    first_token_seen: bool,
    tool_clock: Vec<(String, Instant)>,
    phase: Phase,
    frame_index: usize,
    permission: Option<PendingPermission>,
    permission_tx: Option<Sender<char>>,
    ctrl_c_armed: Option<Instant>,
    pending_action: Option<Action>,
    focus: Focus,
    selected: Option<usize>,
    scroll: usize,
    follow: bool,
    top_button: usize,
    selector: Option<Selector>,
    show_help: bool,
    show_trace: bool,
    traces: Vec<String>,
    regions: Regions,
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
        let mut state = Self::bare(Some(history_path));
        state.history = history.iter().cloned().collect();
        state.refresh_from_cli(cli);
        state
    }

    fn bare(history_path: Option<PathBuf>) -> Self {
        Self {
            transcript: Transcript::default(),
            footer: String::new(),
            draft: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_cursor: None,
            history_path,
            completions: Vec::new(),
            mention_paths: Vec::new(),
            model: String::new(),
            mode: String::new(),
            busy: false,
            turn_started: None,
            first_token_seen: false,
            tool_clock: Vec::new(),
            phase: Phase::Idle,
            frame_index: 0,
            permission: None,
            permission_tx: None,
            ctrl_c_armed: None,
            pending_action: None,
            focus: Focus::Composer,
            selected: None,
            scroll: 0,
            follow: true,
            top_button: 0,
            selector: None,
            show_help: false,
            show_trace: false,
            traces: Vec::new(),
            regions: Regions::default(),
            quit: false,
            should_persist: false,
        }
    }

    /// Recomputes the values derived from the CLI: footer, model, mode, completions, and
    /// mentions.
    fn refresh_from_cli(&mut self, cli: &LiveCli) {
        self.footer = cli.input_footer();
        self.model = cli.model().to_string();
        self.mode = cli.permission_mode().as_str().to_string();
        self.completions = cli.repl_completion_candidates().unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_default();
        self.mention_paths = mentions::workspace_files(&cwd);
    }

    /// Adds an observability line: to the trace log and to the transcript.
    fn trace(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.traces.push(format!("{} {text}", clock_stamp()));
        if self.traces.len() > MAX_TRACES {
            let excess = self.traces.len() - MAX_TRACES;
            self.traces.drain(0..excess);
        }
        self.transcript.push(Entry::Trace(text));
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

    /// The grey completion of the draft from history, when the cursor is at the end.
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

    fn scroll_by(&mut self, delta: isize) {
        self.follow = false;
        let next = isize::try_from(self.scroll).unwrap_or(0) + delta;
        self.scroll = usize::try_from(next.max(0)).unwrap_or(0);
    }

    fn open_selector(&mut self, selector: Selector) {
        self.selector = Some(selector);
    }
}

fn clock_stamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let day = seconds % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3_600, (day / 60) % 60, day % 60)
}

fn format_count(tokens: u32) -> String {
    if tokens < 1_000 {
        tokens.to_string()
    } else {
        format!("{:.1}k", f64::from(tokens) / 1_000.0)
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
    stdout.execute(EnableMouseCapture)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn teardown_terminal(terminal: &mut CrosstermTerminal) {
    let result = (|| -> io::Result<()> {
        terminal.show_cursor()?;
        terminal.backend_mut().execute(DisableMouseCapture)?;
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
    state: &mut TuiState,
    run_slash_command: &mut F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&mut LiveCli, SlashCommand, &mut Vec<u8>) -> Result<bool, Box<dyn std::error::Error>>,
{
    let (ui_tx, ui_rx) = channel::<UiEvent>();
    let (turn_tx, turn_rx) = channel::<String>();
    let (turn_done_tx, turn_done_rx) = channel::<(crate::BuiltRuntime, crate::HookAbortMonitor)>();

    loop {
        while let Ok(event) = ui_rx.try_recv() {
            apply_ui_event(state, event);
        }

        if !state.busy {
            if let Ok(line) = turn_rx.try_recv() {
                dispatch_line(cli, state, &ui_tx, &turn_done_tx, line, run_slash_command);
            }
        }

        if let Ok((runtime, _hook_abort_monitor)) = turn_done_rx.try_recv() {
            if let Err(error) = cli.replace_runtime(runtime) {
                state.transcript.push(Entry::Notice(format!(
                    "error: failed to update session: {error}"
                )));
            }
            state.refresh_from_cli(cli);
        }

        if let Some(action) = state.pending_action.take() {
            apply_action(cli, state, action);
        }

        if state.quit {
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
                Ok(Event::Mouse(mouse)) => handle_mouse(mouse, state),
                Ok(_) => {}
                Err(error) => eprintln!("warning: failed to read terminal event: {error}"),
            },
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => eprintln!("warning: failed to poll terminal events: {error}"),
        }
    }

    // A turn still running at exit finishes first, so the session stays consistent.
    if state.busy {
        while let Ok(event) = ui_rx.recv_timeout(Duration::from_millis(50)) {
            apply_ui_event(state, event);
            if !state.busy {
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
    state: &mut TuiState,
    ui_tx: &Sender<UiEvent>,
    turn_done_tx: &Sender<(crate::BuiltRuntime, crate::HookAbortMonitor)>,
    line: String,
    run_slash_command: &mut F,
) where
    F: FnMut(&mut LiveCli, SlashCommand, &mut Vec<u8>) -> Result<bool, Box<dyn std::error::Error>>,
{
    let trimmed = line.trim().to_string();

    if matches!(trimmed.as_str(), "/exit" | "/quit") {
        state.quit = true;
        state.should_persist = true;
        return;
    }

    if let Some(command) = trimmed.strip_prefix('!') {
        state.transcript.push(Entry::User(trimmed.clone()));
        let output = capture_shell_command(command.trim());
        state.transcript.push(Entry::Notice(output));
        return;
    }

    let parsed = match SlashCommand::parse(&trimmed) {
        Ok(parsed) => parsed,
        Err(error) => {
            state.transcript.push(Entry::Notice(error.to_string()));
            return;
        }
    };

    if let Some(command) = parsed {
        let mut buffer: Vec<u8> = Vec::new();
        match run_slash_command(cli, command, &mut buffer) {
            Ok(handled) => {
                if !buffer.is_empty() {
                    state
                        .transcript
                        .push(Entry::Notice(String::from_utf8_lossy(&buffer).into_owned()));
                }
                if handled {
                    state.refresh_from_cli(cli);
                }
            }
            Err(error) => state
                .transcript
                .push(Entry::Notice(format!("error: {error}"))),
        }
        return;
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let (prompt, attached) = mentions::expand(&trimmed, &cwd);
    if !attached.is_empty() {
        state.transcript.push(Entry::Notice(format!(
            "\x1b[2mAttached {}\x1b[0m",
            attached.join(", ")
        )));
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

/// Starts a streaming turn on a worker thread. The worker owns a freshly built runtime and
/// ships UI events and the finished runtime back to the UI loop.
fn start_turn(
    cli: &mut LiveCli,
    state: &mut TuiState,
    display: String,
    prompt: String,
    ui_tx: Sender<UiEvent>,
    turn_done_tx: Sender<(crate::BuiltRuntime, crate::HookAbortMonitor)>,
) {
    let (runtime, hook_abort_monitor) = match cli.prepare_turn_runtime(false) {
        Ok(pair) => pair,
        Err(error) => {
            state
                .transcript
                .push(Entry::Notice(format!("error: {error}")));
            return;
        }
    };

    let model = cli.model().to_string();
    let permission_mode = cli.permission_mode();
    let (permission_tx, permission_rx) = channel::<char>();

    state.transcript.push(Entry::User(display));
    state.busy = true;
    state.turn_started = Some(Instant::now());
    state.first_token_seen = false;
    state.phase = Phase::Waiting;
    state.permission_tx = Some(permission_tx);
    state.follow = true;
    state.trace(format!("request sent to {model}"));

    std::thread::spawn(move || {
        let mut runtime = runtime;
        let started = Instant::now();

        // Each tool result reaches the UI as soon as it completes.
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

        let cost_usd = match pricing_for_model(&model) {
            Some(pricing) => runtime
                .usage()
                .cumulative_usage()
                .estimate_cost_usd_with_pricing(pricing)
                .total_cost_usd(),
            None => 0.0,
        };
        let _ = ui_tx.send(UiEvent::CostUsd(cost_usd));

        // Hand the runtime back so the session keeps accumulating across turns.
        let _ = turn_done_tx.send((runtime, hook_abort_monitor));
    });
}

// ---------------------------------------------------------------------------
// Actions that need the CLI
// ---------------------------------------------------------------------------

fn apply_action(cli: &mut LiveCli, state: &mut TuiState, action: Action) {
    match action {
        Action::CyclePermission => {
            let next = crate::next_permission_mode(cli.permission_mode().as_str());
            match cli.switch_permission_mode(next) {
                Ok(previous) => {
                    state.trace(format!("permission mode {previous} -> {next}"));
                    state.refresh_from_cli(cli);
                }
                Err(error) => state
                    .transcript
                    .push(Entry::Notice(format!("error: {error}"))),
            }
        }
        Action::ClearScreen => {
            state.transcript.clear();
            state.selected = None;
            state.scroll = 0;
            state.follow = true;
            state.transcript.push(Entry::Banner(cli.startup_banner()));
        }
        Action::Complete => complete_draft(state),
    }
}

/// Completes the word at the cursor: a slash command for a leading `/`, or a workspace
/// path for `@`. A single match is inserted; several matches insert their common prefix
/// and are listed.
fn complete_draft(state: &mut TuiState) {
    let draft = state.draft.clone();
    let (start, candidates): (usize, Vec<String>) =
        if draft.starts_with('/') && !draft.contains(' ') {
            (
                0,
                state
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
            let Some(prefix) = draft[start..].strip_prefix('@') else {
                return;
            };
            (
                start,
                state
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
            let listing = candidates
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("  ");
            state
                .transcript
                .push(Entry::Notice(format!("\x1b[2m{listing}\x1b[0m")));
            return;
        }
        common
    };
    state.set_draft(format!("{}{}", &draft[..start], replacement));
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
// Worker-side sink and permission prompter
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

/// Permission prompter that sends the request to the UI and waits for the user's choice,
/// routed back over a per-turn channel. The worker never reads the terminal itself.
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
// Keys
// ---------------------------------------------------------------------------

/// Returns `true` when the TUI should exit.
fn handle_key(key: KeyEvent, state: &mut TuiState, turn_tx: &Sender<String>) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if state.selector.is_some() {
        return handle_selector_key(key, state, turn_tx);
    }

    if state.show_help {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::F(1))
            || key.code == KeyCode::Char('?')
        {
            state.show_help = false;
        }
        return false;
    }

    if let Some(prompt) = state.permission.take() {
        let choice = match key.code {
            KeyCode::Char('y' | 'Y') => Some('y'),
            KeyCode::Char('a' | 'A') => Some('a'),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => Some('n'),
            KeyCode::Char('c') if ctrl => Some('n'),
            _ => None,
        };
        match choice {
            Some(choice) => {
                if let Some(tx) = state.permission_tx.take() {
                    let _ = tx.send(choice);
                }
                let decision = match choice {
                    'y' => "allowed once",
                    'a' => "allowed for this session",
                    _ => "denied",
                };
                state.trace(format!("{} {decision}", prompt.tool));
                state.phase = Phase::Tool(prompt.tool);
            }
            None => state.permission = Some(prompt),
        }
        return false;
    }

    // Global keys work in every focus and during a turn.
    match key.code {
        KeyCode::Char('c') if ctrl => {
            handle_ctrl_c(state);
            return state.quit;
        }
        KeyCode::Char('d') if ctrl => {
            if state.draft.is_empty() && !state.busy {
                state.quit = true;
                state.should_persist = true;
                return true;
            }
            return false;
        }
        KeyCode::Char('k') if ctrl => {
            open_palette(state);
            return false;
        }
        KeyCode::Char('l') if ctrl => {
            state.pending_action = Some(Action::ClearScreen);
            return false;
        }
        KeyCode::Char('t') if ctrl => {
            state.show_trace = !state.show_trace;
            return false;
        }
        KeyCode::F(1) => {
            state.show_help = true;
            return false;
        }
        KeyCode::F(2) => {
            open_model_selector(state);
            return false;
        }
        KeyCode::F(3) => {
            open_permission_selector(state);
            return false;
        }
        KeyCode::F(4) => {
            state.show_trace = !state.show_trace;
            return false;
        }
        KeyCode::F(5) => {
            open_session_selector(state);
            return false;
        }
        KeyCode::BackTab => {
            state.pending_action = Some(Action::CyclePermission);
            return false;
        }
        KeyCode::PageUp => {
            state.scroll_by(-(page_size(state).cast_signed()));
            return false;
        }
        KeyCode::PageDown => {
            state.scroll_by(page_size(state).cast_signed());
            return false;
        }
        KeyCode::Tab if state.focus == Focus::Composer && completion_context(&state.draft) => {
            state.pending_action = Some(Action::Complete);
            return false;
        }
        KeyCode::Tab => {
            state.focus = match state.focus {
                Focus::Composer => Focus::Transcript,
                Focus::Transcript => Focus::Buttons,
                Focus::Buttons => Focus::Composer,
            };
            return false;
        }
        KeyCode::Esc if state.focus != Focus::Composer => {
            state.focus = Focus::Composer;
            return false;
        }
        _ => {}
    }

    match state.focus {
        Focus::Composer => handle_composer_key(key, state, turn_tx),
        Focus::Transcript => handle_transcript_key(key, state),
        Focus::Buttons => handle_buttons_key(key, state),
    }
    false
}

fn page_size(state: &TuiState) -> usize {
    usize::from(state.regions.transcript.height.max(4)).saturating_sub(2)
}

fn completion_context(draft: &str) -> bool {
    draft.starts_with('/') || draft.contains('@')
}

fn handle_ctrl_c(state: &mut TuiState) {
    if !state.draft.is_empty() {
        state.draft.clear();
        state.cursor = 0;
        state.history_cursor = None;
        state.ctrl_c_armed = None;
        return;
    }
    let armed = state
        .ctrl_c_armed
        .is_some_and(|at| at.elapsed() < EXIT_CONFIRM_WINDOW);
    if armed {
        state.quit = true;
        state.should_persist = true;
        return;
    }
    state.ctrl_c_armed = Some(Instant::now());
    state.transcript.push(Entry::Notice(
        "\x1b[2mPress Ctrl-C again to exit\x1b[0m".to_string(),
    ));
}

fn handle_composer_key(key: KeyEvent, state: &mut TuiState, turn_tx: &Sender<String>) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if state.busy {
        return;
    }

    if key.code == KeyCode::Enter {
        if ctrl
            || key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            state.insert_at_cursor("\n");
            state.history_cursor = None;
            return;
        }
        let trimmed = state.draft.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        state.push_history(trimmed.clone());
        state.draft.clear();
        state.cursor = 0;
        state.ctrl_c_armed = None;
        if turn_tx.send(trimmed).is_err() {
            state.transcript.push(Entry::Notice(
                "error: internal error: TUI event channel closed".to_string(),
            ));
        }
        return;
    }

    match key.code {
        KeyCode::Char('?') if state.draft.is_empty() => state.show_help = true,
        KeyCode::Backspace => {
            state.backspace();
            state.history_cursor = None;
        }
        KeyCode::Left => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Right => {
            if let Some(suggestion) = state.history_suggestion() {
                state.insert_at_cursor(&suggestion);
            } else {
                state.cursor = (state.cursor + 1).min(state.draft.chars().count());
            }
        }
        KeyCode::Home => state.cursor = 0,
        KeyCode::End => state.cursor = state.draft.chars().count(),
        KeyCode::Up => {
            let len = state.history.len();
            if len > 0 {
                let position = match state.history_cursor {
                    None => len - 1,
                    Some(current) => current.saturating_sub(1),
                };
                state.history_cursor = Some(position);
                let entry = state.history[position].clone();
                state.set_draft(entry);
            }
        }
        KeyCode::Down => {
            if let Some(current) = state.history_cursor {
                let next = current + 1;
                if next >= state.history.len() {
                    state.history_cursor = None;
                    state.set_draft(String::new());
                } else {
                    state.history_cursor = Some(next);
                    let entry = state.history[next].clone();
                    state.set_draft(entry);
                }
            }
        }
        KeyCode::Esc => {
            state.draft.clear();
            state.cursor = 0;
            state.history_cursor = None;
        }
        KeyCode::Char(character) if !ctrl => {
            state.insert_at_cursor(&character.to_string());
            state.history_cursor = None;
        }
        _ => {}
    }
}

fn handle_transcript_key(key: KeyEvent, state: &mut TuiState) {
    let count = state.transcript.entries().len();
    match key.code {
        KeyCode::Up => {
            state.follow = false;
            state.selected = Some(match state.selected {
                Some(index) => index.saturating_sub(1),
                None => count.saturating_sub(1),
            });
        }
        KeyCode::Down => {
            state.selected = match state.selected {
                Some(index) if index + 1 < count => Some(index + 1),
                _ => None,
            };
            if state.selected.is_none() {
                state.follow = true;
            }
        }
        KeyCode::Enter => match state.selected {
            Some(index) => state.transcript.toggle_expanded(index),
            None => state.focus = Focus::Composer,
        },
        KeyCode::Home => {
            state.follow = false;
            state.scroll = 0;
        }
        KeyCode::End => state.follow = true,
        KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.focus = Focus::Composer;
            state.insert_at_cursor(&character.to_string());
        }
        _ => {}
    }
}

fn handle_buttons_key(key: KeyEvent, state: &mut TuiState) {
    match key.code {
        KeyCode::Left => {
            state.top_button = (state.top_button + BUTTONS.len() - 1) % BUTTONS.len();
        }
        KeyCode::Right => state.top_button = (state.top_button + 1) % BUTTONS.len(),
        KeyCode::Enter | KeyCode::Char(' ') => activate_button(BUTTONS[state.top_button], state),
        KeyCode::Down => state.focus = Focus::Composer,
        _ => {}
    }
}

fn activate_button(button: Button, state: &mut TuiState) {
    match button {
        Button::Model => open_model_selector(state),
        Button::Mode => open_permission_selector(state),
        Button::Sessions => open_session_selector(state),
        Button::Trace => state.show_trace = !state.show_trace,
        Button::Help => state.show_help = true,
    }
}

fn open_model_selector(state: &mut TuiState) {
    if state.busy {
        state.transcript.push(Entry::Notice(
            "wait for the turn to finish to change the model".to_string(),
        ));
        return;
    }
    let mut items: Vec<Item> = state
        .completions
        .iter()
        .filter_map(|candidate| candidate.strip_prefix("/model "))
        .map(|model| Item {
            label: model.to_string(),
            detail: String::new(),
            value: format!("/model {model}"),
        })
        .collect();
    if items.is_empty() {
        items.push(Item {
            label: state.model.clone(),
            detail: "current model".to_string(),
            value: format!("/model {}", state.model),
        });
    }
    state.open_selector(Selector::new("Model", Purpose::Model, items));
}

fn open_session_selector(state: &mut TuiState) {
    if state.busy {
        state.transcript.push(Entry::Notice(
            "wait for the turn to finish to switch sessions".to_string(),
        ));
        return;
    }
    let items: Vec<Item> = state
        .completions
        .iter()
        .filter_map(|candidate| candidate.strip_prefix("/resume "))
        .map(|session| Item {
            label: session.to_string(),
            detail: "saved session".to_string(),
            value: format!("/resume {session}"),
        })
        .collect();
    if items.is_empty() {
        state
            .transcript
            .push(Entry::Notice("no saved sessions yet".to_string()));
        return;
    }
    state.open_selector(Selector::new("Sessions", Purpose::Session, items));
}

fn open_permission_selector(state: &mut TuiState) {
    let items = [
        ("read-only", "reads only, no edits or commands"),
        ("workspace-write", "edits inside the workspace"),
        ("danger-full-access", "no restrictions"),
    ]
    .into_iter()
    .map(|(mode, detail)| Item {
        label: mode.to_string(),
        detail: detail.to_string(),
        value: format!("/permissions {mode}"),
    })
    .collect();
    state.open_selector(Selector::new("Permission mode", Purpose::Permission, items));
}

fn open_palette(state: &mut TuiState) {
    let items = state
        .completions
        .iter()
        .filter(|candidate| !candidate.contains(' '))
        .map(|command| Item {
            label: command.clone(),
            detail: String::new(),
            value: command.clone(),
        })
        .collect();
    state.open_selector(Selector::new("Commands", Purpose::Command, items));
}

fn handle_selector_key(key: KeyEvent, state: &mut TuiState, turn_tx: &Sender<String>) -> bool {
    let Some(selector) = state.selector.as_mut() else {
        return false;
    };
    match key.code {
        KeyCode::Up => selector.move_by(-1),
        KeyCode::Down => selector.move_by(1),
        KeyCode::PageUp => selector.move_by(-10),
        KeyCode::PageDown => selector.move_by(10),
        KeyCode::Home => selector.selected = 0,
        KeyCode::End => selector.selected = selector.items.len().saturating_sub(1),
        KeyCode::Esc => state.selector = None,
        KeyCode::Enter => {
            let line = selector.chosen().map(|item| item.value.clone());
            state.selector = None;
            if let Some(line) = line {
                if turn_tx.send(line).is_err() {
                    state.transcript.push(Entry::Notice(
                        "error: internal error: TUI event channel closed".to_string(),
                    ));
                }
            }
        }
        _ => {}
    }
    false
}

fn handle_mouse(mouse: MouseEvent, state: &mut TuiState) {
    if state.selector.is_some() || state.show_help {
        return;
    }
    let point = Position::new(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::ScrollUp if state.regions.transcript.contains(point) => {
            state.scroll_by(-(SCROLL_STEP.cast_signed()));
        }
        MouseEventKind::ScrollDown if state.regions.transcript.contains(point) => {
            state.scroll_by(SCROLL_STEP.cast_signed());
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some((_, button)) = state
                .regions
                .buttons
                .iter()
                .find(|(rect, _)| rect.contains(point))
                .copied()
            {
                state.focus = Focus::Buttons;
                state.top_button = BUTTONS.iter().position(|b| *b == button).unwrap_or(0);
                activate_button(button, state);
            } else if state.regions.transcript.contains(point) {
                state.focus = Focus::Transcript;
                let line = state.scroll + usize::from(mouse.row - state.regions.transcript.y);
                state.selected = state
                    .regions
                    .entry_ranges
                    .iter()
                    .position(|(start, end)| line >= *start && line < *end);
                state.follow = false;
            } else if state.regions.composer.contains(point) {
                state.focus = Focus::Composer;
            }
        }
        _ => {}
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
    CostUsd(f64),
    TurnEnd {
        error: Option<String>,
    },
}

fn apply_ui_event(state: &mut TuiState, event: UiEvent) {
    match event {
        UiEvent::Text(delta) => {
            if !state.first_token_seen {
                state.first_token_seen = true;
                let elapsed = state.turn_started.map_or(Duration::ZERO, |at| at.elapsed());
                state.trace(format!(
                    "first token after {}",
                    transcript::format_duration(elapsed)
                ));
            }
            state.phase = Phase::Streaming;
            state.transcript.push_text(&delta);
        }
        UiEvent::ToolStart { name, input } => {
            state.trace(format!("tool {name} started"));
            state.tool_clock.push((name.clone(), Instant::now()));
            state.transcript.start_tool(&name, &input);
            state.phase = Phase::Tool(name);
        }
        UiEvent::ToolResult {
            name,
            output,
            is_error,
        } => {
            let duration = take_tool_clock(state, &name);
            state.trace(format!(
                "tool {name} {} in {}",
                if is_error { "failed" } else { "finished" },
                transcript::format_duration(duration)
            ));
            state
                .transcript
                .finish_tool(&name, &output, is_error, duration);
            state.phase = Phase::Waiting;
        }
        UiEvent::Usage(usage) => {
            state.trace(format!(
                "model usage: {} in · {} out tokens",
                format_count(usage.input_tokens),
                format_count(usage.output_tokens)
            ));
        }
        UiEvent::MessageStop => {
            state.transcript.end_stream();
            state.phase = Phase::Waiting;
        }
        UiEvent::Notice(text) => state.transcript.push(Entry::Notice(text)),
        UiEvent::PermissionRequest {
            tool,
            required_mode,
            current_mode,
            reason,
            input,
        } => {
            let mut text = format!(
                "\n\x1b[1m{tool}\x1b[0m needs {required_mode} (current mode: {current_mode})\n"
            );
            if let Some(reason) = &reason {
                let _ = writeln!(text, "  \x1b[2m{reason}\x1b[0m");
            }
            let _ = write!(
                text,
                "  \x1b[2m{}\x1b[0m",
                crate::truncate_for_summary(&input, 160)
            );
            state.transcript.push(Entry::Notice(text));
            state.trace(format!("permission requested for {tool} ({required_mode})"));
            state.phase = Phase::Approval(tool.clone());
            state.permission = Some(PendingPermission { tool });
        }
        UiEvent::CostUsd(_) => {}
        UiEvent::TurnEnd { error } => {
            state.transcript.end_stream();
            let elapsed = state.turn_started.map_or(Duration::ZERO, |at| at.elapsed());
            state.busy = false;
            state.turn_started = None;
            state.permission = None;
            state.permission_tx = None;
            state.phase = Phase::Idle;
            match error {
                Some(error) => {
                    state
                        .transcript
                        .push(Entry::Notice(format!("error: {error}")));
                    state.trace(format!(
                        "turn failed after {}",
                        transcript::format_duration(elapsed)
                    ));
                }
                None => state.trace(format!(
                    "turn finished in {}",
                    transcript::format_duration(elapsed)
                )),
            }
        }
    }
}

fn take_tool_clock(state: &mut TuiState, name: &str) -> Duration {
    let position = state.tool_clock.iter().rposition(|(tool, _)| tool == name);
    position
        .map(|index| state.tool_clock.remove(index).1.elapsed())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn draw(terminal: &mut CrosstermTerminal, state: &mut TuiState) {
    state.frame_index = state.frame_index.wrapping_add(1);
    let _ = terminal.draw(|frame| render_frame(frame, state));
}

fn composer_height(state: &TuiState) -> u16 {
    let rows = if state.permission.is_some() {
        1
    } else {
        state.draft.split('\n').count().clamp(1, 6)
    };
    // Rule, input rows, rule, footer.
    u16::try_from(rows).unwrap_or(1) + 3
}

fn render_frame(frame: &mut Frame, state: &mut TuiState) {
    let area = frame.area();
    let composer = composer_height(state).min(area.height.saturating_sub(3));
    let loader = u16::from(state.busy);
    let [top, main, loader_area, composer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(loader),
        Constraint::Length(composer),
    ])
    .areas(area);

    render_top_bar(frame, top, state);

    let (transcript_area, trace_area) = if state.show_trace {
        let [left, right] = Layout::horizontal([
            Constraint::Min(20),
            Constraint::Length(TRACE_PANEL_WIDTH.min(main.width.saturating_sub(20))),
        ])
        .areas(main);
        (left, Some(right))
    } else {
        (main, None)
    };
    render_transcript(frame, transcript_area, state);
    if let Some(trace_area) = trace_area {
        render_trace_panel(frame, trace_area, state);
    }

    if state.busy {
        render_loader(frame, loader_area, state);
    }
    render_composer(frame, composer_area, state);

    if let Some(selector) = &state.selector {
        widgets::render_selector(frame, main, selector);
    }
    if state.show_help {
        render_help(frame, main);
    }
}

fn render_top_bar(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    let title = format!(" Frontal Code v{} ", env!("CARGO_PKG_VERSION"));
    let mut x = area.x + u16::try_from(title.chars().count()).unwrap_or(0);
    let mut spans = vec![Span::styled(
        title,
        Style::default().add_modifier(Modifier::BOLD),
    )];
    state.regions.buttons.clear();
    for (index, button) in BUTTONS.iter().enumerate() {
        let label = button.label(&state.mode);
        let focused = state.focus == Focus::Buttons && state.top_button == index;
        let width = u16::try_from(label.chars().count() + 2).unwrap_or(0);
        if x.saturating_add(width) <= area.x + area.width {
            state
                .regions
                .buttons
                .push((Rect::new(x, area.y, width, 1), *button));
        }
        spans.push(widgets::button(&label, focused));
        spans.push(Span::raw(" "));
        x = x.saturating_add(width).saturating_add(1);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Lays the transcript out as wrapped lines, then shows the window the scroll position
/// selects. The selected entry is highlighted.
fn render_transcript(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    state.regions.transcript = area;
    let text_width = usize::from(area.width.saturating_sub(1)).max(1);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (index, entry) in state.transcript.entries().iter().enumerate() {
        let start = lines.len();
        let mut rendered = transcript::wrap(
            transcript::render_entry(entry, state.frame_index),
            text_width,
        );
        if state.selected == Some(index) {
            for line in &mut rendered {
                line.style = line.style.bg(Color::Indexed(236));
            }
        }
        lines.extend(rendered);
        ranges.push((start, lines.len()));
        lines.push(Line::default());
    }
    if !state.transcript.streaming().is_empty() {
        for paragraph in state.transcript.streaming().split('\n') {
            lines.push(Line::from(paragraph.to_string()));
        }
    }
    state.regions.entry_ranges = ranges;

    let height = usize::from(area.height).max(1);
    let total = lines.len();
    let max_scroll = total.saturating_sub(height);
    if state.follow {
        state.scroll = max_scroll;
    }
    if let Some(index) = state.selected {
        if let Some((start, end)) = state.regions.entry_ranges.get(index).copied() {
            if start < state.scroll {
                state.scroll = start;
            } else if end > state.scroll + height {
                state.scroll = end.saturating_sub(height);
            }
            state.follow = false;
        }
    }
    state.scroll = state.scroll.min(max_scroll);
    if state.scroll >= max_scroll {
        state.follow = true;
    }

    let visible: Vec<Line<'static>> = lines.into_iter().skip(state.scroll).take(height).collect();
    let text_area = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };
    frame.render_widget(Paragraph::new(visible), text_area);

    let mut scrollbar_state = ScrollbarState::new(max_scroll.max(1)).position(state.scroll);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        area,
        &mut scrollbar_state,
    );
}

fn render_trace_panel(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" trace · logs ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let height = usize::from(inner.height);
    let start = state.traces.len().saturating_sub(height);
    let lines: Vec<Line<'static>> = state.traces[start..]
        .iter()
        .map(|trace| {
            Line::from(Span::styled(
                trace.clone(),
                Style::default().fg(Color::Indexed(177)),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The loader: the animated logo, "Thinking...", the elapsed time, and what the turn is
/// doing now.
fn render_loader(frame: &mut Frame, area: Rect, state: &TuiState) {
    let secs = state
        .turn_started
        .map_or(0, |started| started.elapsed().as_secs());
    let mut spans = vec![
        Span::styled(
            format!("{} ", widgets::loader_logo(state.frame_index)),
            Style::default().fg(Color::Indexed(33)),
        ),
        Span::styled(
            format!("Thinking... {secs}s"),
            Style::default().fg(Color::Indexed(33)),
        ),
    ];
    let detail = state.phase.detail();
    if !detail.is_empty() {
        spans.push(Span::styled(
            format!(" · {detail}"),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_composer(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    state.regions.composer = area;
    let rule = Line::from(Span::styled(
        "─".repeat(usize::from(area.width)),
        Style::default().fg(Color::DarkGray),
    ));
    let mut lines = vec![rule.clone()];
    let mut cursor_position = None;

    if let Some(permission) = &state.permission {
        lines.push(Line::from(format!(
            "Allow?  [y] yes   [n] no   [a] always for {}  > ",
            permission.tool
        )));
    } else if state.draft.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("❯ "),
            Span::styled(
                crate::input::PLACEHOLDER.to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        cursor_position = Some((2, 0));
    } else {
        let total_rows = state.draft.split('\n').count();
        let suggestion = state.history_suggestion();
        for (index, text) in state.draft.split('\n').enumerate() {
            let prefix = if index == 0 { "❯ " } else { "  " };
            let mut spans = vec![Span::raw(prefix), Span::raw(text.to_string())];
            if index == total_rows - 1 {
                if let Some(suggestion) = &suggestion {
                    spans.push(Span::styled(
                        suggestion.clone(),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            lines.push(Line::from(spans));
        }
        let before: String = state.draft.chars().take(state.cursor).collect();
        let cursor_row = before.matches('\n').count();
        let cursor_col = before
            .rsplit('\n')
            .next()
            .map_or(0, |line| line.chars().count());
        cursor_position = Some((2 + cursor_col, cursor_row));
    }

    lines.push(rule);
    let footer = ansi::lines(&state.footer)
        .into_iter()
        .next()
        .unwrap_or_default();
    lines.push(footer);
    frame.render_widget(Paragraph::new(lines), area);

    if state.permission.is_none() && state.focus == Focus::Composer && state.selector.is_none() {
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

/// Shortcuts for the full-screen interface (the REPL's list lives in `render_shortcuts`).
const TUI_SHORTCUTS: &str = "Shortcuts
  Enter                Send             Shift/Alt+Enter   New line
  Up/Down              History in the composer, entries in the transcript
  Tab                  Focus: composer, transcript, buttons (completes / and @ in the composer)
  Esc                  Back to the composer
  PageUp / PageDown    Scroll the conversation
  Home / End           Top of the conversation / follow the latest output
  Enter (transcript)   Expand or collapse a tool result
  Left/Right (buttons) Move between buttons; Enter presses one
  F1 or ?              Help            F2 Model    F3 Mode    F4 Trace    F5 Sessions
  Ctrl-K               Command palette
  Ctrl-T               Trace panel     Ctrl-L      Clear the screen
  Shift+Tab            Cycle the permission mode
  Ctrl-C               Clear the line; press twice on an empty prompt to exit
  Ctrl-D               Exit on an empty prompt
  !<command>           Run a shell command
  @<path>              Attach a workspace file
  Mouse                Click buttons and entries; the wheel scrolls";

fn render_help(frame: &mut Frame, area: Rect) {
    let text = TUI_SHORTCUTS;
    let lines: Vec<Line<'static>> = text
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect();
    let width = area.width.saturating_sub(4).min(92);
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .min(area.height);
    let popup = widgets::centered(area, width, height);
    frame.render_widget(ratatui::widgets::Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Help · Esc close "),
        ),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn bare() -> TuiState {
        TuiState::bare(None)
    }

    fn frame_text(state: &mut TuiState, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| render_frame(frame, state))
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    #[test]
    fn composer_uses_the_repl_frame() {
        let mut state = bare();
        state.footer = "Frontal Code · claude · API key".to_string();
        let lines = frame_text(&mut state, 70, 8);
        let bottom = lines.len() - 1;
        assert!(lines[bottom].starts_with("Frontal Code · claude"));
        assert!(lines[bottom - 1].starts_with('─'));
        assert!(lines[bottom - 2].starts_with("❯ Ask Frontal Code"));
        assert!(lines[bottom - 3].starts_with('─'));
    }

    #[test]
    fn top_bar_shows_the_buttons() {
        let mut state = bare();
        state.mode = "read-only".to_string();
        let lines = frame_text(&mut state, 100, 6);
        assert!(lines[0].contains("Model"));
        assert!(lines[0].contains("Mode: read-only"));
        assert!(lines[0].contains("Trace"));
        assert!(lines[0].contains("Help"));
        assert!(lines[0].contains("Sessions"));
        assert_eq!(state.regions.buttons.len(), 5);
    }

    #[test]
    fn loader_appears_while_a_turn_runs() {
        let mut state = bare();
        state.busy = true;
        state.phase = Phase::Tool("Read".to_string());
        let lines = frame_text(&mut state, 90, 8);
        assert!(lines.iter().any(|line| line.contains("Thinking...")));
        assert!(lines.iter().any(|line| line.contains("running Read")));
    }

    #[test]
    fn tab_cycles_focus_through_regions() {
        let (turn_tx, _turn_rx) = channel::<String>();
        let mut state = bare();
        handle_key(key(KeyCode::Tab), &mut state, &turn_tx);
        assert_eq!(state.focus, Focus::Transcript);
        handle_key(key(KeyCode::Tab), &mut state, &turn_tx);
        assert_eq!(state.focus, Focus::Buttons);
        handle_key(key(KeyCode::Tab), &mut state, &turn_tx);
        assert_eq!(state.focus, Focus::Composer);
    }

    #[test]
    fn transcript_selection_moves_and_expands_tools() {
        let (turn_tx, _turn_rx) = channel::<String>();
        let mut state = bare();
        state.transcript.push(Entry::User("first".to_string()));
        state.transcript.start_tool("bash", r#"{"command":"ls"}"#);
        state.transcript.finish_tool(
            "bash",
            r#"{"stdout":"a\nb"}"#,
            false,
            Duration::from_millis(1),
        );
        state.focus = Focus::Transcript;
        handle_key(key(KeyCode::Up), &mut state, &turn_tx);
        assert_eq!(state.selected, Some(1));
        handle_key(key(KeyCode::Enter), &mut state, &turn_tx);
        assert!(matches!(
            &state.transcript.entries()[1],
            Entry::Tool(tool) if tool.expanded
        ));
    }

    #[test]
    fn selector_choice_is_sent_as_a_line() {
        let (turn_tx, turn_rx) = channel::<String>();
        let mut state = bare();
        open_permission_selector(&mut state);
        handle_key(key(KeyCode::Down), &mut state, &turn_tx);
        handle_key(key(KeyCode::Enter), &mut state, &turn_tx);
        assert!(state.selector.is_none());
        assert_eq!(
            turn_rx.try_recv().ok().as_deref(),
            Some("/permissions workspace-write")
        );
    }

    #[test]
    fn permission_keys_work_while_busy() {
        let (turn_tx, _turn_rx) = channel::<String>();
        let (choice_tx, choice_rx) = channel::<char>();
        let mut state = bare();
        state.busy = true;
        state.permission = Some(PendingPermission {
            tool: "Write".to_string(),
        });
        state.permission_tx = Some(choice_tx);
        handle_key(key(KeyCode::Char('y')), &mut state, &turn_tx);
        assert_eq!(choice_rx.try_recv().ok(), Some('y'));
        assert!(state.permission.is_none());
    }

    #[test]
    fn ctrl_c_on_an_empty_prompt_needs_a_second_press() {
        let (turn_tx, _turn_rx) = channel::<String>();
        let mut state = bare();
        assert!(!handle_key(ctrl('c'), &mut state, &turn_tx));
        assert!(!state.quit);
        assert!(handle_key(ctrl('c'), &mut state, &turn_tx));
        assert!(state.quit);
    }

    #[test]
    fn scrolling_moves_the_view_and_follow_resumes_at_the_bottom() {
        let mut state = bare();
        for index in 0..40 {
            state
                .transcript
                .push(Entry::Notice(format!("line {index}")));
        }
        let _ = frame_text(&mut state, 60, 10);
        assert!(state.follow);
        let before = state.scroll;
        state.scroll_by(-5);
        assert!(!state.follow);
        let _ = frame_text(&mut state, 60, 10);
        assert!(state.scroll < before);
    }

    #[test]
    fn tab_completes_slash_commands_and_workspace_paths() {
        let mut state = bare();
        state.completions = vec![
            "/model".to_string(),
            "/model opus".to_string(),
            "/status".to_string(),
        ];
        state.draft = "/mo".to_string();
        complete_draft(&mut state);
        assert_eq!(state.draft, "/model");

        let mut state = bare();
        state.mention_paths = vec!["src/main.rs".to_string()];
        state.draft = "look at @src/ma".to_string();
        complete_draft(&mut state);
        assert_eq!(state.draft, "look at @src/main.rs");
    }

    #[test]
    fn history_suggestion_is_the_rest_of_the_latest_match() {
        let mut state = bare();
        state.history = vec!["!echo one".to_string(), "/help".to_string()];
        state.draft = "!ech".to_string();
        state.cursor = 4;
        assert_eq!(state.history_suggestion().as_deref(), Some("o one"));
    }

    #[test]
    fn common_prefix_of_candidates() {
        let candidates = vec!["/model opus".to_string(), "/model sonnet".to_string()];
        assert_eq!(common_prefix(&candidates), "/model ");
        assert_eq!(common_prefix(&[]), "");
    }

    #[test]
    fn counts_are_compact() {
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_500), "1.5k");
    }
}
