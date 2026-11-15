//! Desktop notifications fired when an interactive turn ends.
//!
//! macOS uses `terminal-notifier` when it is on `PATH` (it supports a custom
//! content image) and falls back to `osascript display notification` otherwise.
//! Neither requires a new dependency: both ship with the system or are a one-line
//! `brew install`. The brand mark is embedded in the binary and extracted to a
//! stable cache file on first use, so the notification carries our logo even
//! when the checkout no longer has the source asset.
//!
//! Banners are suppressed automatically on macOS when the frontmost app is a
//! terminal, since the output is already visible there.
//! `FRONTAL_CODE_FORCE_NOTIFY` (truthy: `1`, `true`, `yes`) overrides that check
//! so a banner always fires.
//!
//! Failures are intentionally swallowed: a broken notification must never fail
//! a turn. Set `FRONTAL_CODE_NO_NOTIFY` to a truthy value (`1`, `true`, `yes`)
//! to disable all notifications; `0`/`false`/`no`/empty leave them enabled.

#[cfg(target_os = "macos")]
mod imp {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    /// The brand mark, embedded at build time so the notification works even
    /// when the source tree is not on disk (installed binaries, packaged apps).
    const LOGO: &[u8] = include_bytes!("../assets/logo.png");

    /// Where the extracted PNG lives so `terminal-notifier` can reference it by path.
    /// Reused across launches; rewritten only when the embedded bytes change.
    fn icon_path() -> Option<PathBuf> {
        let home = env::var_os("HOME")?;
        let dir = PathBuf::from(home).join(".frontal-code");
        let path = dir.join("notify-logo.png");
        let _ = fs::create_dir_all(&dir);
        if fs::read(&path).ok() != Some(LOGO.to_vec()) && fs::write(&path, LOGO).is_err() {
            return None;
        }
        Some(path)
    }

    /// Escape a string for inclusion in a double-quoted `AppleScript` literal.
    /// `"` becomes `\"` and backslashes become `\\`; newlines are folded to
    /// spaces so the notification body stays one line.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn escape_applescript(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\n', '\r'], " ")
    }

    /// Build the `terminal-notifier` argv when the binary is on `PATH`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn terminal_notifier_args(
        title: &str,
        subtitle: &str,
        body: &str,
        icon: Option<&PathBuf>,
    ) -> Vec<String> {
        let mut args = vec![
            "-title".to_string(),
            title.to_string(),
            "-message".to_string(),
            body.to_string(),
        ];
        if !subtitle.is_empty() {
            args.push("-subtitle".to_string());
            args.push(subtitle.to_string());
        }
        if let Some(icon) = icon {
            args.push("-contentImage".to_string());
            args.push(icon.to_string_lossy().into_owned());
        }
        args
    }

    /// Build the `osascript -e` script for the fallback path.
    fn osascript_source(title: &str, subtitle: &str, body: &str) -> String {
        let mut source = String::from("display notification \"");
        source.push_str(&escape_applescript(body));
        source.push_str("\" with title \"");
        source.push_str(&escape_applescript(title));
        source.push('"');
        if !subtitle.is_empty() {
            source.push_str(" subtitle \"");
            source.push_str(&escape_applescript(subtitle));
            source.push('"');
        }
        source
    }

    /// Whether `FRONTAL_CODE_NO_NOTIFY` holds a truthy value. Falsy values (`0`,
    /// `false`, `no`, empty) leave notifications enabled.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn notify_disabled(value: Option<&str>) -> bool {
        match value {
            Some(value) => {
                let lowered = value.trim().to_ascii_lowercase();
                !lowered.is_empty() && lowered != "0" && lowered != "false" && lowered != "no"
            }
            None => false,
        }
    }

    /// Spawn `program` with `args`; return `true` when it ran and exited 0.
    fn spawn_ok(program: &str, args: &[String]) -> bool {
        let status = Command::new(program).args(args).output();
        status.is_ok_and(|out| out.status.success())
    }

    /// Bundle IDs of well-known macOS terminals.
    const KNOWN_TERMINALS: &[&str] = &[
        "com.apple.Terminal",
        "com.googlecode.iterm2",
        "com.mitchellh.ghostty",
        "dev.warp.Warp-Stable",
        "dev.warp.Warp",
        "io.alacritty.Alacritty",
        "com.github.wez.wezterm",
        "net.kovidgoyal.kitty",
        "co.zeit.hyper",
        "com.tilenumbers.Tabby",
        "dev.zed.Zed",
    ];

    /// Process names of terminals that may not expose a reliable bundle ID.
    const TERMINAL_NAMES: &[&str] = &[
        "Terminal",
        "iTerm2",
        "Ghostty",
        "Warp",
        "Alacritty",
        "WezTerm",
        "Hyper",
        "kitty",
        "Tabby",
        "gnome-terminal",
        "konsole",
        "xterm",
        "rxvt",
    ];

    /// Whether a given frontmost app (identified by bundle ID and process name)
    /// is a terminal. Used to suppress banners when the user is already looking
    /// at the terminal where the output is shown.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_terminal_app(bundle_id: &str, name: &str) -> bool {
        if KNOWN_TERMINALS
            .iter()
            .any(|id| id.eq_ignore_ascii_case(bundle_id))
        {
            return true;
        }
        TERMINAL_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
    }

    /// Upper bound for the frontmost-app probe. The check must never hold up
    /// a turn-end, so a hung `osascript` is abandoned after this window and
    /// treated as "not active" (fail-open: the banner still fires).
    const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

    /// Whether the user is actively looking at a terminal window right now.
    ///
    /// When the frontmost app is a terminal, the user can already see the
    /// output, so a banner would be redundant. We compare the frontmost app's
    /// bundle ID against a set of known terminal bundle IDs and fall back to
    /// the process name so unusual or custom terminals are caught too.
    fn terminal_active() -> bool {
        let script = "tell application \"System Events\"\n\
                      set frontApp to first application process whose frontmost is true\n\
                      set bid to \"\"\n\
                      try\n set bid to bundle identifier of frontApp\n end try\n\
                      set pname to name of frontApp\n\
                      return bid & \"|\" & pname\n\
                      end tell";
        let output = run_with_timeout(PROBE_TIMEOUT, move || {
            Command::new("osascript")
                .arg("-e")
                .arg(script)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output()
        });
        let Some(Ok(output)) = output else {
            return false;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let (bundle_id, name) = match text.split_once('|') {
            Some((b, n)) => (b.trim(), n.trim()),
            None => (text.trim(), ""),
        };
        is_terminal_app(bundle_id, name)
    }

    /// Run `work`, returning `None` if it outlives `timeout`. The work runs on
    /// a detached thread because the OS does not offer a portable way to kill
    /// a spawned child; abandoning merely stops waiting on it, which is all
    /// this probe needs.
    fn run_with_timeout(
        timeout: std::time::Duration,
        work: impl FnOnce() -> std::io::Result<std::process::Output> + Send + 'static,
    ) -> Option<std::io::Result<std::process::Output>> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        rx.recv_timeout(timeout).ok()
    }

    pub fn notify(title: &str, subtitle: &str, body: &str) -> bool {
        if let Some(value) = env::var_os("FRONTAL_CODE_NO_NOTIFY") {
            if notify_disabled(Some(value.to_string_lossy().as_ref())) {
                return false;
            }
        }
        // Skip the banner when the user is already looking at the terminal;
        // the output is on screen, so a notification is redundant.
        // FRONTAL_CODE_FORCE_NOTIFY (truthy) overrides this check.
        let force = match env::var_os("FRONTAL_CODE_FORCE_NOTIFY") {
            Some(value) => notify_disabled(Some(value.to_string_lossy().as_ref())),
            None => false,
        };
        if !force && terminal_active() {
            return false;
        }
        let icon = icon_path();

        // Prefer terminal-notifier: it is the only common macOS notifier that
        // accepts a custom content image, which is how the logo reaches the
        // notification banner. Fall back to osascript when it is absent.
        if let Ok(path) = env::var("PATH") {
            for dir in path.split(':') {
                let candidate = PathBuf::from(dir).join("terminal-notifier");
                if candidate.is_file() {
                    let args = terminal_notifier_args(title, subtitle, body, icon.as_ref());
                    if spawn_ok(candidate.to_string_lossy().as_ref(), &args) {
                        return true;
                    }
                    break;
                }
            }
        }

        let source = osascript_source(title, subtitle, body);
        spawn_ok("osascript", &["-e".to_string(), source])
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn notify(_title: &str, _subtitle: &str, _body: &str) -> bool {
        false
    }

    /// Non-macOS: no banner exists to suppress, so the terminal is never "active".
    pub(crate) fn terminal_active() -> bool {
        false
    }
}

pub use imp::notify;

/// Fires a "turn finished" desktop notification.
///
/// `body` is the first line of the assistant's final message (or a short error
/// summary when the turn failed); `model` is shown as the subtitle so the user
/// can tell which session answered. Returns `true` when a notification was
/// actually dispatched, which is useful for tests and future telemetry.
pub fn notify_turn_end(model: &str, body: &str, error: Option<&str>) -> bool {
    let body = match error {
        Some(error) => truncate_one_line(error, 120),
        None => truncate_one_line(body, 120),
    };
    if body.is_empty() {
        return false;
    }
    notify("Frontal Code", model, &body)
}

/// Collapse a value to a single line, capped at `limit` characters with an
/// ellipsis. Notification bodies should never wrap or swallow the whole banner.
fn truncate_one_line(value: &str, limit: usize) -> String {
    // Only the first line may appear in a notification banner; a second line
    // is detail the banner has no room for.
    let first_line = value.split(['\n', '\r']).next().unwrap_or("");
    let flat: String = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = flat.chars();
    let truncated: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_and_collapses_whitespace() {
        assert_eq!(truncate_one_line("hello   world", 5), "hello…");
        assert_eq!(truncate_one_line("short", 10), "short");
    }

    #[test]
    fn truncate_one_line_selects_only_the_first_line() {
        // Only the first line reaches the banner; later lines are excluded even
        // when the limit would have left room for them.
        assert_eq!(truncate_one_line("a\nb\nc", 10), "a");
        assert_eq!(truncate_one_line("Done.\nAdditional details", 120), "Done.");
        assert_eq!(truncate_one_line("Title line\nmore\ntail", 5), "Title…");
        // CRLF line endings are treated as a line break too.
        assert_eq!(truncate_one_line("head\r\ntail", 10), "head");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn notify_disabled_semantics() {
        // Only explicit truthy values disable notifications.
        assert!(imp::notify_disabled(Some("1")));
        assert!(imp::notify_disabled(Some("true")));
        assert!(imp::notify_disabled(Some("TRUE")));
        assert!(imp::notify_disabled(Some("yes")));
        assert!(imp::notify_disabled(Some(" on ")));
        // Falsy / empty / unset leave notifications enabled.
        assert!(!imp::notify_disabled(Some("0")));
        assert!(!imp::notify_disabled(Some("false")));
        assert!(!imp::notify_disabled(Some("no")));
        assert!(!imp::notify_disabled(Some("")));
        assert!(!imp::notify_disabled(Some("  ")));
        assert!(!imp::notify_disabled(None));
    }

    #[test]
    fn notify_turn_end_shortens_long_bodies() {
        let long = "x".repeat(500);
        let body = truncate_one_line(&long, 120);
        assert_eq!(body.chars().count(), 121); // 120 + ellipsis
        assert!(body.ends_with('…'));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn escape_applescript_escapes_quotes_and_backslashes() {
        // A double quote gets exactly one escaping backslash; a backslash is
        // doubled. The full output is asserted so a regression to two backslashes
        // before a quote (which would terminate the AppleScript string early)
        // is caught.
        assert_eq!(imp::escape_applescript("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(imp::escape_applescript("a\\b"), "a\\\\b");
        assert_eq!(
            imp::escape_applescript("say \"hi\" \\ there"),
            "say \\\"hi\\\" \\\\".to_string() + " there",
        );
        assert_eq!(imp::escape_applescript("line1\nline2"), "line1 line2");
        assert_eq!(imp::escape_applescript("no special"), "no special");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn terminal_notifier_args_uses_content_image_spelling() {
        let icon = std::path::PathBuf::from("/tmp/logo.png");
        let args = imp::terminal_notifier_args("Title", "model", "body", Some(&icon));
        // terminal-notifier uses the camelCase `-contentImage` option (not the
        // kebab-case `-content-image`), which is how the logo is attached.
        assert!(args
            .windows(2)
            .any(|w| w == ["-contentImage", "/tmp/logo.png"]));
        assert!(!args.iter().any(|a| a == "-content-image"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn is_terminal_app_matches_known_bundle_ids() {
        // Known terminals by bundle ID.
        assert!(imp::is_terminal_app("com.apple.Terminal", ""));
        assert!(imp::is_terminal_app("com.googlecode.iterm2", ""));
        assert!(imp::is_terminal_app("com.mitchellh.ghostty", ""));
        assert!(imp::is_terminal_app("dev.warp.Warp-Stable", ""));
        assert!(imp::is_terminal_app("io.alacritty.Alacritty", ""));
        assert!(imp::is_terminal_app("com.github.wez.wezterm", ""));
        assert!(imp::is_terminal_app("net.kovidgoyal.kitty", ""));
        // Zed embeds a terminal and reports itself as the frontmost app.
        assert!(imp::is_terminal_app("dev.zed.Zed", ""));
        // Matching is case-insensitive.
        assert!(imp::is_terminal_app("COM.APPLE.TERMINAL", ""));
        // Non-terminals do not match.
        assert!(!imp::is_terminal_app("com.apple.Safari", ""));
        assert!(!imp::is_terminal_app("com.apple.finder", ""));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn is_terminal_app_matches_process_names() {
        // Fallback to process name when the bundle ID is unknown or empty.
        assert!(imp::is_terminal_app("", "Terminal"));
        assert!(imp::is_terminal_app("", "iTerm2"));
        assert!(imp::is_terminal_app("", "Ghostty"));
        assert!(imp::is_terminal_app("", "Alacritty"));
        assert!(imp::is_terminal_app("", "kitty"));
        assert!(imp::is_terminal_app("", "xterm"));
        // Case-insensitive.
        assert!(imp::is_terminal_app("", "terminal"));
        // Non-terminals do not match.
        assert!(!imp::is_terminal_app("", "Safari"));
        assert!(!imp::is_terminal_app("", "zed"));
        assert!(!imp::is_terminal_app("", ""));
    }
}
