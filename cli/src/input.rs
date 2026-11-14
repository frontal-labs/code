use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{
    Cmd, CompletionType, ConditionalEventHandler, Config, Context, EditMode, Editor, Event,
    EventContext, EventHandler, Helper, KeyCode, KeyEvent, Modifiers, RepeatCount,
};

pub(crate) const PLACEHOLDER: &str =
    "Ask Frontal Code to build features, fix bugs, or work on your code";

/// A second Ctrl-C within this window exits; the first one only shows a hint.
const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Submit(String),
    Cancel,
    Exit,
    /// Ctrl-L: clear the screen and redraw the header.
    Clear,
    /// `?` on an empty line: show the shortcuts panel.
    Shortcuts,
    /// Shift+Tab: move to the next permission mode.
    CyclePermission,
}

/// Keys whose effect needs the REPL, not the editor. The key handler records one of these
/// and interrupts the read; `read_line` turns it into a `ReadOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    Shortcuts,
    Clear,
    CyclePermission,
}

struct SlashCommandHelper {
    completions: Vec<String>,
    mentions: Vec<String>,
    history: Vec<String>,
    current_line: RefCell<String>,
}

impl SlashCommandHelper {
    fn new(completions: Vec<String>) -> Self {
        Self {
            completions: normalize_completions(completions),
            mentions: Vec::new(),
            history: Vec::new(),
            current_line: RefCell::new(String::new()),
        }
    }

    fn reset_current_line(&self) {
        self.current_line.borrow_mut().clear();
    }

    fn current_line(&self) -> String {
        self.current_line.borrow().clone()
    }

    fn set_current_line(&self, line: &str) {
        let mut current = self.current_line.borrow_mut();
        current.clear();
        current.push_str(line);
    }

    fn set_completions(&mut self, completions: Vec<String>) {
        self.completions = normalize_completions(completions);
    }
}

impl Completer for SlashCommandHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        if let Some((start, prefix)) = mention_prefix(line, pos) {
            let matches = self
                .mentions
                .iter()
                .filter(|path| path.starts_with(prefix))
                .map(|path| Pair {
                    display: path.clone(),
                    replacement: format!("@{path}"),
                })
                .collect();
            return Ok((start, matches));
        }

        let Some(prefix) = slash_command_prefix(line, pos) else {
            return Ok((0, Vec::new()));
        };

        let matches = self
            .completions
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| Pair {
                display: candidate.clone(),
                replacement: candidate.clone(),
            })
            .collect();

        Ok((0, matches))
    }
}

impl Hinter for SlashCommandHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        if line.is_empty() {
            return Some(PLACEHOLDER.to_string());
        }
        if pos != line.len() {
            return None;
        }
        // Suggest the most recent history entry that continues what is typed.
        self.history
            .iter()
            .rev()
            .find(|entry| entry.len() > line.len() && entry.starts_with(line))
            .map(|entry| entry[line.len()..].to_string())
    }
}

impl Highlighter for SlashCommandHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        self.set_current_line(line);
        Cow::Borrowed(line)
    }

    fn highlight_char(&self, line: &str, _pos: usize, _kind: CmdKind) -> bool {
        self.set_current_line(line);
        false
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[90m{hint}\x1b[0m"))
    }
}

impl Validator for SlashCommandHelper {}
impl Helper for SlashCommandHelper {}

/// Records a pending action and interrupts the read. When `only_when_empty` is set and the
/// line already has text, the key keeps its normal meaning (for example `?` types a question mark).
struct RecordAction {
    action: PendingAction,
    pending: Arc<Mutex<Option<PendingAction>>>,
    only_when_empty: bool,
}

impl ConditionalEventHandler for RecordAction {
    fn handle(
        &self,
        _evt: &Event,
        _n: RepeatCount,
        _positive: bool,
        ctx: &EventContext,
    ) -> Option<Cmd> {
        if self.only_when_empty && !ctx.line().is_empty() {
            return None;
        }
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(self.action);
        }
        Some(Cmd::Interrupt)
    }
}

pub struct LineEditor {
    prompt: String,
    footer: String,
    editor: Editor<SlashCommandHelper, DefaultHistory>,
    pending: Arc<Mutex<Option<PendingAction>>>,
    history_path: Option<PathBuf>,
    last_ctrl_c: Option<Instant>,
}

impl LineEditor {
    #[must_use]
    pub fn new(prompt: impl Into<String>, completions: Vec<String>) -> Self {
        let config = Config::builder()
            .completion_type(CompletionType::List)
            .edit_mode(EditMode::Emacs)
            .build();
        let mut editor = Editor::<SlashCommandHelper, DefaultHistory>::with_config(config)
            .expect("rustyline editor should initialize");
        editor.set_helper(Some(SlashCommandHelper::new(completions)));

        let pending = Arc::new(Mutex::new(None));
        bind_action(
            &mut editor,
            KeyEvent(KeyCode::Char('?'), Modifiers::NONE),
            PendingAction::Shortcuts,
            &pending,
            true,
        );
        bind_action(
            &mut editor,
            KeyEvent(KeyCode::Char('L'), Modifiers::CTRL),
            PendingAction::Clear,
            &pending,
            false,
        );
        bind_action(
            &mut editor,
            KeyEvent(KeyCode::BackTab, Modifiers::NONE),
            PendingAction::CyclePermission,
            &pending,
            false,
        );
        editor.bind_sequence(KeyEvent(KeyCode::Char('J'), Modifiers::CTRL), Cmd::Newline);
        editor.bind_sequence(KeyEvent(KeyCode::Enter, Modifiers::SHIFT), Cmd::Newline);
        editor.bind_sequence(KeyEvent(KeyCode::Enter, Modifiers::ALT), Cmd::Newline);

        Self {
            prompt: prompt.into(),
            footer: String::new(),
            editor,
            pending,
            history_path: None,
            last_ctrl_c: None,
        }
    }

    /// Loads prompt history from `path` and saves it there after every submitted line.
    pub fn enable_history(&mut self, path: PathBuf) {
        let _ = self.editor.load_history(&path);
        let entries: Vec<String> = self.editor.history().iter().cloned().collect();
        if let Some(helper) = self.editor.helper_mut() {
            helper.history = entries;
        }
        self.history_path = Some(path);
    }

    pub fn push_history(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        if entry.trim().is_empty() {
            return;
        }

        let _ = self.editor.add_history_entry(entry.clone());
        if let Some(helper) = self.editor.helper_mut() {
            helper.history.push(entry);
        }
        if let Some(path) = &self.history_path {
            let _ = self.editor.save_history(path);
        }
    }

    /// Sets the line drawn under the input box, already styled.
    pub fn set_footer(&mut self, footer: impl Into<String>) {
        self.footer = footer.into();
    }

    pub fn set_completions(&mut self, completions: Vec<String>) {
        if let Some(helper) = self.editor.helper_mut() {
            helper.set_completions(completions);
        }
    }

    /// Workspace-relative file paths offered after `@`.
    pub fn set_mention_candidates(&mut self, paths: Vec<String>) {
        if let Some(helper) = self.editor.helper_mut() {
            helper.mentions = paths;
        }
    }

    pub fn read_line(&mut self) -> io::Result<ReadOutcome> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return self.read_line_fallback();
        }

        if let Some(helper) = self.editor.helper_mut() {
            helper.reset_current_line();
        }
        if let Ok(mut pending) = self.pending.lock() {
            *pending = None;
        }

        self.draw_frame()?;
        let result = self.editor.readline(&self.prompt);
        self.clear_frame()?;
        match result {
            Ok(line) => {
                self.last_ctrl_c = None;
                Ok(ReadOutcome::Submit(line))
            }
            Err(ReadlineError::Interrupted) => {
                if let Some(action) = self.take_pending() {
                    self.finish_interrupted_read()?;
                    return Ok(match action {
                        PendingAction::Shortcuts => ReadOutcome::Shortcuts,
                        PendingAction::Clear => ReadOutcome::Clear,
                        PendingAction::CyclePermission => ReadOutcome::CyclePermission,
                    });
                }
                let has_input = !self.current_line().is_empty();
                self.finish_interrupted_read()?;
                if has_input {
                    self.last_ctrl_c = None;
                    return Ok(ReadOutcome::Cancel);
                }
                // Empty prompt: the first Ctrl-C only arms the exit; a second one soon after exits.
                let armed = self
                    .last_ctrl_c
                    .is_some_and(|at| at.elapsed() < EXIT_CONFIRM_WINDOW);
                if armed {
                    crate::analytics::mark_interrupted();
                    return Ok(ReadOutcome::Exit);
                }
                self.last_ctrl_c = Some(Instant::now());
                let mut stdout = io::stdout();
                writeln!(stdout, "\x1b[2mPress Ctrl-C again to exit\x1b[0m")?;
                Ok(ReadOutcome::Cancel)
            }
            Err(ReadlineError::Eof) => {
                self.finish_interrupted_read()?;
                Ok(ReadOutcome::Exit)
            }
            Err(error) => Err(io::Error::other(error)),
        }
    }

    fn take_pending(&self) -> Option<PendingAction> {
        self.pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.take())
    }

    /// Draws a rule above and below the input row, with the footer under the rule, then
    /// moves the cursor back to the input row so the prompt is printed inside the box.
    fn draw_frame(&self) -> io::Result<()> {
        let width = crossterm::terminal::size()
            .ok()
            .map(|(cols, _)| usize::from(cols))
            .filter(|cols| *cols > 0)
            .unwrap_or(80);
        let rule = format!("\x1b[90m{}\x1b[0m", "─".repeat(width));
        let mut stdout = io::stdout();
        let footer = clip_visible(&self.footer, width);
        write!(stdout, "{rule}\n\n{rule}\n{footer}\x1b[2A\r")?;
        stdout.flush()
    }

    /// Clears the bottom rule and footer once the line is submitted.
    fn clear_frame(&self) -> io::Result<()> {
        let mut stdout = io::stdout();
        write!(stdout, "\x1b[J")?;
        stdout.flush()
    }

    fn current_line(&self) -> String {
        self.editor
            .helper()
            .map_or_else(String::new, SlashCommandHelper::current_line)
    }

    fn finish_interrupted_read(&mut self) -> io::Result<()> {
        if let Some(helper) = self.editor.helper_mut() {
            helper.reset_current_line();
        }
        let mut stdout = io::stdout();
        writeln!(stdout)
    }

    fn read_line_fallback(&self) -> io::Result<ReadOutcome> {
        let mut stdout = io::stdout();
        write!(stdout, "{}", self.prompt)?;
        stdout.flush()?;

        let mut buffer = String::new();
        let bytes_read = io::stdin().read_line(&mut buffer)?;
        if bytes_read == 0 {
            return Ok(ReadOutcome::Exit);
        }

        while matches!(buffer.chars().last(), Some('\n' | '\r')) {
            buffer.pop();
        }
        Ok(ReadOutcome::Submit(buffer))
    }
}

fn bind_action(
    editor: &mut Editor<SlashCommandHelper, DefaultHistory>,
    key: KeyEvent,
    action: PendingAction,
    pending: &Arc<Mutex<Option<PendingAction>>>,
    only_when_empty: bool,
) {
    editor.bind_sequence(
        key,
        EventHandler::Conditional(Box::new(RecordAction {
            action,
            pending: Arc::clone(pending),
            only_when_empty,
        })),
    );
}

/// Cuts `text` to `width` visible columns, keeping ANSI styling intact and ending with an
/// ellipsis when something was dropped.
fn clip_visible(text: &str, width: usize) -> String {
    let limit = width.saturating_sub(1);
    let mut out = String::new();
    let mut visible = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            for next in chars.by_ref() {
                out.push(next);
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if visible >= limit {
            out.push('…');
            out.push_str("\x1b[0m");
            return out;
        }
        out.push(c);
        visible += 1;
    }
    out
}

fn slash_command_prefix(line: &str, pos: usize) -> Option<&str> {
    if pos != line.len() {
        return None;
    }

    let prefix = &line[..pos];
    if !prefix.starts_with('/') {
        return None;
    }

    Some(prefix)
}

/// For a word that starts with `@` right before the cursor, returns where the word starts
/// and the text typed after the `@`.
fn mention_prefix(line: &str, pos: usize) -> Option<(usize, &str)> {
    if pos != line.len() {
        return None;
    }
    let start = line.rfind(char::is_whitespace).map_or(0, |index| index + 1);
    let word = &line[start..];
    let rest = word.strip_prefix('@')?;
    Some((start, rest))
}

fn normalize_completions(completions: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    completions
        .into_iter()
        .filter(|candidate| candidate.starts_with('/'))
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{mention_prefix, slash_command_prefix, LineEditor, SlashCommandHelper};
    use rustyline::completion::Completer;
    use rustyline::highlight::Highlighter;
    use rustyline::history::{DefaultHistory, History};
    use rustyline::Context;

    #[test]
    fn extracts_terminal_slash_command_prefixes_with_arguments() {
        assert_eq!(slash_command_prefix("/he", 3), Some("/he"));
        assert_eq!(slash_command_prefix("/help me", 8), Some("/help me"));
        assert_eq!(
            slash_command_prefix("/session switch ses", 19),
            Some("/session switch ses")
        );
        assert_eq!(slash_command_prefix("hello", 5), None);
        assert_eq!(slash_command_prefix("/help", 2), None);
    }

    #[test]
    fn completes_matching_slash_commands() {
        let helper = SlashCommandHelper::new(vec![
            "/help".to_string(),
            "/hello".to_string(),
            "/status".to_string(),
        ]);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (start, matches) = helper
            .complete("/he", 3, &ctx)
            .expect("completion should work");

        assert_eq!(start, 0);
        assert_eq!(
            matches
                .into_iter()
                .map(|candidate| candidate.replacement)
                .collect::<Vec<_>>(),
            vec!["/help".to_string(), "/hello".to_string()]
        );
    }

    #[test]
    fn completes_matching_slash_command_arguments() {
        let helper = SlashCommandHelper::new(vec![
            "/model".to_string(),
            "/model opus".to_string(),
            "/model sonnet".to_string(),
            "/session switch alpha".to_string(),
        ]);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (start, matches) = helper
            .complete("/model o", 8, &ctx)
            .expect("completion should work");

        assert_eq!(start, 0);
        assert_eq!(
            matches
                .into_iter()
                .map(|candidate| candidate.replacement)
                .collect::<Vec<_>>(),
            vec!["/model opus".to_string()]
        );
    }

    #[test]
    fn ignores_non_slash_command_completion_requests() {
        let helper = SlashCommandHelper::new(vec!["/help".to_string()]);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (_, matches) = helper
            .complete("hello", 5, &ctx)
            .expect("completion should work");

        assert!(matches.is_empty());
    }

    #[test]
    fn completes_workspace_paths_after_at_sign() {
        assert_eq!(mention_prefix("look at @cli/sr", 15), Some((8, "cli/sr")));
        assert_eq!(mention_prefix("@", 1), Some((0, "")));
        assert_eq!(mention_prefix("plain text", 10), None);
        assert_eq!(mention_prefix("@cli/sr x", 9), None);

        let mut helper = SlashCommandHelper::new(Vec::new());
        helper.mentions = vec!["cli/src/main.rs".to_string(), "README.md".to_string()];
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (start, matches) = helper
            .complete("open @cli/", 10, &ctx)
            .expect("completion should work");

        assert_eq!(start, 5);
        assert_eq!(
            matches
                .into_iter()
                .map(|candidate| candidate.replacement)
                .collect::<Vec<_>>(),
            vec!["@cli/src/main.rs".to_string()]
        );
    }

    #[test]
    fn clips_footer_to_width_and_keeps_styling() {
        assert_eq!(super::clip_visible("abcdef", 4), "abc…\x1b[0m");
        assert_eq!(
            super::clip_visible("\x1b[90mab\x1b[0m", 10),
            "\x1b[90mab\x1b[0m"
        );
    }

    #[test]
    fn tracks_current_buffer_through_highlighter() {
        let helper = SlashCommandHelper::new(Vec::new());
        let _ = helper.highlight("draft", 5);

        assert_eq!(helper.current_line(), "draft");
    }

    #[test]
    fn push_history_ignores_blank_entries() {
        let mut editor = LineEditor::new("> ", vec!["/help".to_string()]);
        editor.push_history("   ");
        editor.push_history("/help");

        assert_eq!(editor.editor.history().len(), 1);
    }

    #[test]
    fn set_completions_replaces_and_normalizes_candidates() {
        let mut editor = LineEditor::new("> ", vec!["/help".to_string()]);
        editor.set_completions(vec![
            "/model opus".to_string(),
            "/model opus".to_string(),
            "status".to_string(),
        ]);

        let helper = editor.editor.helper().expect("helper should exist");
        assert_eq!(helper.completions, vec!["/model opus".to_string()]);
    }
}
