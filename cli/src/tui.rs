//! Full-screen TUI renderer for the Frontal Code REPL, built on ratatui.
//!
//! Layout (top to bottom):
//! - **Header** — app name, model, access label, branch, permission mode, ctx tokens.
//! - **Scrollback** — conversation history from [`app::App::messages`].
//! - **Live region** — streaming text / tool status / thinking animation.
//! - **Input** — single-line text box; Enter submits, Esc clears, Ctrl+Q quits.
//! - **Status bar** — key hints and error messages.
//!
//! The TUI owns raw terminal mode for the duration of the REPL session. When
//! it exits (Ctrl+Q or `/quit`), it restores the terminal to its prior state.

use std::io::{self, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::app::{App, Message, TurnStatus};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A message the TUI wants to surface to the caller (submitted prompt, exit
/// request, permission-mode cycle, etc.).
pub enum TuiAction {
    /// The user submitted a prompt for the runtime to process.
    Submit(String),
    /// The user pressed Ctrl+Q or typed `/quit`.
    Exit,
    /// The user wants to cycle the permission mode.
    CyclePermission,
}

/// Configuration for the TUI session.
pub struct TuiConfig {
    /// Model name to display in the header.
    pub model: String,
    /// Provider access label (e.g. "anthropic", "local").
    pub access_label: String,
    /// Current permission mode string.
    pub permission_mode: String,
    /// Git branch name, if any.
    pub branch: Option<String>,
    /// Whether the working tree is dirty.
    pub dirty: bool,
    /// Estimated context tokens.
    pub estimated_tokens: u32,
    /// Whether we should show the startup hint banner.
    pub show_banner: bool,
}

impl TuiConfig {
    fn new() -> Self {
        Self {
            model: String::new(),
            access_label: String::new(),
            permission_mode: "default".into(),
            branch: None,
            dirty: false,
            estimated_tokens: 0,
            show_banner: true,
        }
    }
}

/// The TUI session. Run [`Tui::run`] to enter the full-screen event loop.
pub struct Tui {
    config: TuiConfig,
    app: std::sync::Arc<std::sync::RwLock<App>>,
    input: String,
    scroll_offset: u16,
    scroll_at_bottom: bool,
    input_cursor_col: u16,
    input_width: u16,
}

enum TuiEvent {
    /// A frame tick for animation.
    Tick,
    /// A crossterm key event.
    Key(KeyEvent),
    /// A crossterm mouse event.
    Mouse(MouseEvent),
}

impl Tui {
    /// Create a new TUI session. Does NOT enter raw mode — call [`run`] for that.
    #[must_use]
    pub fn new(app: std::sync::Arc<std::sync::RwLock<App>>, config: TuiConfig) -> Self {
        Self {
            config,
            app,
            input: String::new(),
            scroll_offset: 0,
            scroll_at_bottom: true,
            input_cursor_col: 0,
            input_width: 80,
        }
    }

    /// Enter the full-screen TUI loop. Returns when the user exits (Ctrl+Q
    /// or `/quit`). The `on_submit` closure is called for each submitted
    /// prompt; the TUI keeps running while turns are in flight.
    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        mut on_submit: impl FnMut(&str),
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = io::stdout().flush();

        // Spawn a thread that pumps crossterm events into our channel.
        let (tx, rx) = mpsc::channel::<TuiEvent>();
        let event_thread = std::thread::spawn(move || {
            loop {
                if crossterm::event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    match crossterm::event::read() {
                        Ok(Event::Key(key)) => {
                            if tx.send(TuiEvent::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(Event::Mouse(mouse)) => {
                            if tx.send(TuiEvent::Mouse(mouse)).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                // Frame tick every ~100 ms for the thinking animation.
                if tx.send(TuiEvent::Tick).is_err() {
                    break;
                }
            }
        });

        // Fix the `&mut self` borrow problem: the draw closure captures `self`,
        // so `paint` must not take `&mut self` for the parts that don't mutate
        // the TUI itself (scrolling happens on event handling, not in paint).
        let mut exited = false;
        let result: Result<(), Box<dyn std::error::Error>> = loop {
            // Drain all pending events before rendering.
            while let Ok(event) = rx.try_recv() {
                match self.handle_event(event) {
                    Some(TuiAction::Submit(text)) => {
                        on_submit(&text);
                    }
                    Some(TuiAction::Exit) => {
                        exited = true;
                        break;
                    }
                    // No action (CyclePermission is handled via the App).
                    _ => {}
                }
            }
            if exited {
                break Ok(());
            }

            // Render.
            terminal.draw(|f| {
                let area = f.area();
                self.paint(f, area);
            })?;

            // Sleep briefly to avoid busy-spinning; the event thread's ticks
            // wake us on the next loop iteration.
            std::thread::sleep(Duration::from_millis(16));
        };

        // Drop the event thread.
        drop(rx);
        let _ = event_thread.join();

        result
    }

    fn handle_event(&mut self, event: TuiEvent) -> Option<TuiAction> {
        match event {
            TuiEvent::Tick => {
                if let Ok(mut app) = self.app.write() {
                    app.handle(&crate::app::AppEvent::Frame);
                }
                None
            }
            TuiEvent::Mouse(mouse) => self.handle_mouse(mouse),
            TuiEvent::Key(key) => self.handle_key(key),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<TuiAction> {
        // Scroll the scrollback with the mouse wheel.
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            if let Ok(app) = self.app.read() {
                let total = u16::try_from(app.messages.len()).unwrap_or(u16::MAX);
                let max_scroll = total.saturating_sub(1);
                if mouse.kind == MouseEventKind::ScrollUp {
                    self.scroll_at_bottom = false;
                    self.scroll_offset = self.scroll_offset.saturating_add(3).min(max_scroll);
                } else {
                    self.scroll_offset = self.scroll_offset.saturating_sub(3);
                    if self.scroll_offset == 0 {
                        self.scroll_at_bottom = true;
                    }
                }
            }
        }
        None
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        let code = key.code;
        let modifiers = key.modifiers;

        // Ctrl+Q: quit
        if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('q') {
            return Some(TuiAction::Exit);
        }

        // Ctrl+L: clear
        if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('l') {
            if let Ok(mut app) = self.app.write() {
                app.handle(&crate::app::AppEvent::Clear);
            }
            return None;
        }

        // Esc: clear input
        if code == KeyCode::Esc {
            self.input.clear();
            return None;
        }

        // Enter: submit
        if code == KeyCode::Enter {
            let trimmed = self.input.trim().to_string();
            if trimmed.is_empty() {
                return None;
            }
            self.input.clear();

            if trimmed == "/quit" || trimmed == "/exit" {
                return Some(TuiAction::Exit);
            }

            if let Ok(mut app) = self.app.write() {
                app.handle(&crate::app::AppEvent::UserSubmit {
                    text: trimmed.clone(),
                    attached: vec![],
                });
            }
            return Some(TuiAction::Submit(trimmed));
        }

        // Left/Right arrow: move cursor in input
        if code == KeyCode::Left {
            self.input_cursor_col = self.input_cursor_col.saturating_sub(1);
            return None;
        }
        if code == KeyCode::Right {
            let max_col = u16::try_from(self.input.len()).unwrap_or(u16::MAX);
            self.input_cursor_col = (self.input_cursor_col + 1).min(max_col);
            return None;
        }

        // Backspace
        if code == KeyCode::Backspace {
            if self.input_cursor_col > 0 {
                self.input.remove((self.input_cursor_col - 1) as usize);
                self.input_cursor_col -= 1;
            }
            return None;
        }

        // Regular character input
        if let KeyCode::Char(c) = code {
            let pos = (self.input_cursor_col as usize).min(self.input.len());
            self.input.insert(pos, c);
            self.input_cursor_col += 1;
            return None;
        }

        None
    }

    /// Paint the full frame.
    fn paint(&mut self, f: &mut Frame, area: Rect) {
        let app = self
            .app
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Layout: header / scrollback / live / input / status
        let header_h = 3;
        let input_h = 3;
        let status_h = 1;
        let live_h: u16 = if app.is_turn_active() {
            self.pending_display_lines(&app)
        } else {
            1
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_h),
                Constraint::Min(1), // scrollback
                Constraint::Length(live_h),
                Constraint::Length(input_h),
                Constraint::Length(status_h),
            ])
            .split(area);

        self.paint_header(f, chunks[0], &app);
        self.paint_scrollback(f, chunks[1], &app);
        self.paint_live(f, chunks[2], &app);
        self.paint_input(f, chunks[3]);
        self.paint_status(f, chunks[4], &app);
    }

    /// Number of display lines the live region needs.
    fn pending_display_lines(&self, app: &App) -> u16 {
        let width = self.input_width.max(20);
        let mut lines: Vec<String> = Vec::new();

        match &app.turn_status {
            Some(TurnStatus::Thinking) => {
                let spinner = spinner_frame(app.frame);
                lines.push(format!("{spinner} thinking…"));
            }
            Some(TurnStatus::ToolRunning { name }) => {
                lines.push(format!("⚙ {name}"));
            }
            Some(TurnStatus::Streaming) => {
                if !app.pending_text.is_empty() {
                    let wrapped = wrap_text(&app.pending_text, width as usize);
                    lines.extend(wrapped);
                }
            }
            None => lines.push(" ".into()),
        }

        u16::try_from(lines.len().max(1)).unwrap_or(u16::MAX)
    }

    fn paint_header(&self, f: &mut Frame, area: Rect, app: &App) {
        let width = area.width as usize;
        let branch_str = self
            .config
            .branch
            .as_deref()
            .map(|b| {
                let marker = if self.config.dirty { "*" } else { "" };
                format!(" branch {b}{marker}")
            })
            .unwrap_or_default();

        let line1 = truncate_str(
            &format!(
                "Frontal Code · {} {}",
                self.config.model, self.config.access_label
            ),
            width,
        );
        let line2 = truncate_str(
            &format!(
                "{} · {} · ctx {}",
                branch_str.trim_start(),
                app.permission_mode,
                format_token_count(self.config.estimated_tokens)
            ),
            width,
        );

        let text = vec![
            Line::from(Span::styled(
                line1,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(line2, Style::default().fg(Color::Gray))),
        ];

        let block = Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(Color::DarkGray));
        f.render_widget(Paragraph::new(text).block(block), area);
    }

    fn paint_scrollback(&self, f: &mut Frame, area: Rect, app: &App) {
        if app.messages.is_empty() && !self.config.show_banner {
            f.render_widget(
                Paragraph::new("  No messages yet. Type a prompt below."),
                area,
            );
            return;
        }

        let width = area.width.max(5) as usize;
        let mut lines: Vec<Line> = Vec::new();

        // Show a hint banner when there are no messages.
        if app.messages.is_empty() && self.config.show_banner {
            lines.push(Line::from(vec![Span::styled(
                "  Frontal Code TUI",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(Span::styled(
                "  Type a prompt and press Enter. Ctrl+Q to quit. Scroll to browse history.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::default());
        }

        for msg in &app.messages {
            match msg {
                Message::User { text, attached } => {
                    lines.push(Line::from(vec![Span::styled(
                        "  You",
                        Style::default().fg(Color::Cyan),
                    )]));
                    for wrapped in wrap_text(text, width - 4) {
                        lines.push(Line::from(Span::raw(format!("  {wrapped}"))));
                    }
                    if !attached.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("  📎 {}", attached.join(", ")),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    lines.push(Line::default());
                }
                Message::Assistant {
                    text,
                    tools,
                    elapsed_secs,
                    usage: _,
                    auto_compaction,
                } => {
                    lines.push(Line::from(vec![Span::styled(
                        "  Assistant",
                        Style::default().fg(Color::Green),
                    )]));
                    for wrapped in wrap_text(text, width - 4) {
                        lines.push(Line::from(Span::raw(format!("  {wrapped}"))));
                    }
                    if !tools.is_empty() {
                        for tool in tools {
                            lines.push(Line::from(Span::styled(
                                format!("  ⚙ {tool}"),
                                Style::default().fg(Color::Yellow),
                            )));
                        }
                    }
                    if *elapsed_secs > 0.0 {
                        lines.push(Line::from(Span::styled(
                            format!("  ({elapsed_secs:.1}s)"),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    if let Some(count) = auto_compaction {
                        lines.push(Line::from(Span::styled(
                            format!("  (auto-compacted {count} messages)"),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    lines.push(Line::default());
                }
            }
        }

        // Scroll: show the last N lines that fit in the area.
        let total_lines = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let visible = area.height.max(1);
        let max_scroll = total_lines.saturating_sub(visible);
        let scroll_offset = self.scroll_offset.min(max_scroll);
        let scroll_offset = if self.scroll_at_bottom {
            max_scroll
        } else {
            scroll_offset
        };

        // Render only the visible window of lines.
        let start = scroll_offset as usize;
        let end = (scroll_offset + visible) as usize;
        let visible_lines: Vec<Line> = lines[start..end.min(lines.len())].to_vec();
        let text = Text::from(visible_lines);
        let block = Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .style(Style::default());
        f.render_widget(
            Paragraph::new(text).block(block).wrap(Wrap { trim: true }),
            area,
        );
    }

    fn paint_live(&self, f: &mut Frame, area: Rect, app: &App) {
        let width = area.width.max(1) as usize;
        let mut lines: Vec<Line> = Vec::new();

        match &app.turn_status {
            Some(TurnStatus::Thinking) => {
                let spinner = spinner_frame(app.frame);
                lines.push(Line::from(Span::styled(
                    format!("  {spinner} thinking…"),
                    Style::default().fg(Color::Gray),
                )));
            }
            Some(TurnStatus::ToolRunning { name }) => {
                let spinner = spinner_frame(app.frame);
                lines.push(Line::from(Span::styled(
                    format!("  {spinner} ⚙ {name}"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            Some(TurnStatus::Streaming) => {
                if app.pending_text.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "  streaming…",
                        Style::default().fg(Color::Gray),
                    )));
                } else {
                    let text_lines: Vec<Line> = wrap_text(&app.pending_text, width - 2)
                        .into_iter()
                        .map(|w| Line::from(Span::raw(format!("  {w}"))))
                        .collect();
                    lines.extend(text_lines);
                }
            }
            None => {
                lines.push(Line::from(Span::styled(
                    "  Ready",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        if let Some(ref err) = app.last_error {
            lines.push(Line::from(Span::styled(
                format!("  ✗ {err}"),
                Style::default().fg(Color::Red),
            )));
        }

        let block = Block::default().borders(Borders::NONE);
        f.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
    }

    fn paint_input(&self, f: &mut Frame, area: Rect) {
        let input_line = if self.input.is_empty() {
            Line::from(Span::styled(
                "  Ask Frontal Code to build features, fix bugs, or work on your code",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::raw(format!("  {}", self.input)))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Input ", Style::default().fg(Color::Cyan)))
            .style(Style::default().bg(Color::Black));

        f.render_widget(
            Paragraph::new(input_line)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn paint_status(&self, f: &mut Frame, area: Rect, app: &App) {
        let width = area.width.max(1) as usize;
        let status = if app.is_turn_active() {
            "working…"
        } else if app.last_error.is_some() {
            "error"
        } else {
            "idle"
        };

        let line = format!("  {status} · Enter submit · Esc clear · Ctrl+Q quit");
        let text = Line::from(Span::styled(
            truncate_str(&line, width),
            Style::default().fg(Color::DarkGray),
        ));
        f.render_widget(Paragraph::new(text), area);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const SPINNERS: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

fn spinner_frame(frame: u32) -> &'static str {
    SPINNERS[(frame as usize) % SPINNERS.len()]
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            if current.is_empty() {
                // Hard-break words longer than the width.
                if word.len() > width {
                    let mut remaining = word;
                    while remaining.len() > width {
                        lines.push(remaining[..width].to_string());
                        remaining = &remaining[width..];
                    }
                    current = remaining.to_string();
                } else {
                    current.push_str(word);
                }
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.clone());
                // Hard-break very long words.
                if word.len() > width {
                    let mut remaining = word;
                    while remaining.len() > width {
                        lines.push(remaining[..width].to_string());
                        remaining = &remaining[width..];
                    }
                    current = remaining.to_string();
                } else {
                    current = word.to_string();
                }
            }
        }
        if !current.is_empty() {
            lines.push(current);
        } else if raw_line.trim().is_empty() {
            lines.push(String::new());
        }
    }
    if text.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        "…".repeat(max.max(1))
    } else {
        format!("{}…", &s[..max - 1])
    }
}

fn format_token_count(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", f64::from(tokens) / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", f64::from(tokens) / 1_000.0)
    } else {
        tokens.to_string()
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the TUI REPL loop. Takes the `App` (wrapped in an `RwLock` so the TUI
/// render thread can read it while the turn thread writes to it) and an
/// `on_submit` closure invoked for each submitted prompt.
///
/// The caller is responsible for running the turn (e.g. `LiveCli::run_turn`)
/// from within `on_submit` or in a separate thread; the TUI keeps reading
/// `app` and painting between turns.
///
/// Returns `Ok(())` when the user exits normally, or `Err` on I/O failure.
pub fn run(
    app: std::sync::Arc<std::sync::RwLock<App>>,
    config: TuiConfig,
    on_submit: impl FnMut(&str),
) -> Result<(), Box<dyn std::error::Error>> {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Enter raw mode and alternate screen.
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;

    let result = run_inner(app, config, on_submit, &mut terminal);

    // Leave alternate screen and restore raw mode.
    let _ = io::stdout().execute(LeaveAlternateScreen);
    let _ = disable_raw_mode();

    result
}

fn run_inner(
    app: std::sync::Arc<std::sync::RwLock<App>>,
    config: TuiConfig,
    on_submit: impl FnMut(&str),
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tui = Tui::new(app, config);
    tui.run(terminal, on_submit)
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

    #[test]
    fn wrap_text_short() {
        let lines = wrap_text("hello world", 80);
        assert_eq!(lines, vec!["hello world".to_string()]);
    }

    #[test]
    fn wrap_text_long_word() {
        let word = "a".repeat(50);
        let lines = wrap_text(&word, 10);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "a".repeat(10));
    }

    #[test]
    fn wrap_text_empty() {
        let lines = wrap_text("", 80);
        assert_eq!(lines, vec![String::new()]);
    }

    #[test]
    fn wrap_text_multiline() {
        let lines = wrap_text("line one\nline two", 80);
        assert_eq!(lines, vec!["line one".to_string(), "line two".to_string()]);
    }

    #[test]
    fn wrap_text_exact_width() {
        // "abc def" is exactly 7 chars, so it fits on one line at width 7.
        let lines = wrap_text("abc def", 7);
        assert_eq!(lines, vec!["abc def".to_string()]);
    }

    #[test]
    fn wrap_text_with_spaces() {
        let lines = wrap_text("hello    world", 80);
        assert_eq!(lines, vec!["hello world".to_string()]);
    }

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hi", 10), "hi");
    }

    #[test]
    fn truncate_str_long() {
        let s = truncate_str("hello world", 8);
        // 7 chars kept + 1 ellipsis char = 8 chars total (10 bytes in UTF-8).
        assert_eq!(s, "hello w…");
        assert_eq!(s.chars().count(), 8);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn truncate_str_very_short() {
        let s = truncate_str("hello", 1);
        assert_eq!(s, "…");
    }

    #[test]
    fn format_token_count_small() {
        assert_eq!(format_token_count(500), "500");
    }

    #[test]
    fn format_token_count_thousands() {
        assert_eq!(format_token_count(1500), "1.5k");
    }

    #[test]
    fn format_token_count_millions() {
        assert_eq!(format_token_count(2_500_000), "2.5M");
    }

    #[test]
    fn spinner_frame_cycles() {
        assert_eq!(spinner_frame(0), SPINNERS[0]);
        assert_eq!(spinner_frame(7), SPINNERS[7]);
        assert_eq!(spinner_frame(8), SPINNERS[0]);
    }

    #[test]
    fn pending_display_lines_idle() {
        let tui = Tui::new(
            std::sync::Arc::new(std::sync::RwLock::new(app())),
            TuiConfig::new(),
        );
        let app = tui.app.read().unwrap();
        assert_eq!(tui.pending_display_lines(&app), 1);
    }

    #[test]
    fn pending_display_lines_thinking() {
        let a = app();
        let app = App {
            turn_status: Some(TurnStatus::Thinking),
            ..a
        };
        let tui = Tui::new(
            std::sync::Arc::new(std::sync::RwLock::new(app)),
            TuiConfig::new(),
        );
        let app = tui.app.read().unwrap();
        assert_eq!(tui.pending_display_lines(&app), 1);
    }

    #[test]
    fn pending_display_lines_streaming() {
        let a = app();
        let mut app = App {
            turn_status: Some(TurnStatus::Streaming),
            ..a
        };
        app.pending_text = "hello world".into();
        let tui = Tui::new(
            std::sync::Arc::new(std::sync::RwLock::new(app)),
            TuiConfig::new(),
        );
        let app = tui.app.read().unwrap();
        assert!(tui.pending_display_lines(&app) >= 1);
    }
}
