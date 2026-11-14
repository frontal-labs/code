//! Desktop notifications fired when an interactive turn ends.
//!
//! macOS uses `terminal-notifier` when it is on `PATH` (it supports a custom
//! content image) and falls back to `osascript display notification` otherwise.
//! Neither requires a new dependency: both ship with the system or are a one-line
//! `brew install`. The brand mark is embedded in the binary and extracted to a
//! stable cache file on first use, so the notification carries our logo even
//! when the checkout no longer has the source asset.
//!
//! Failures are intentionally swallowed: a broken notification must never fail
//! a turn. Set `FCODE_NO_NOTIFY=1` to disable all notifications.

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

    /// Escape a string for inclusion in a single-quoted `AppleScript` literal.
    /// `"` becomes `\"` and backslashes become `\\`; newlines are folded to
    /// spaces so the notification body stays one line.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn escape_applescript(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\\\"")
            .replace(['\n', '\r'], " ")
    }

    /// Build the `terminal-notifier` argv when the binary is on `PATH`.
    fn terminal_notifier_args(
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
            args.push("-content-image".to_string());
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

    /// Spawn `program` with `args`; return `true` when it ran and exited 0.
    fn spawn_ok(program: &str, args: &[String]) -> bool {
        let status = Command::new(program).args(args).output();
        status.is_ok_and(|out| out.status.success())
    }

    pub fn notify(title: &str, subtitle: &str, body: &str) -> bool {
        if env::var("FCODE_NO_NOTIFY").is_ok() {
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
    let flat: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
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
        assert_eq!(truncate_one_line("a\nb\nc", 10), "a b c");
        assert_eq!(truncate_one_line("short", 10), "short");
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
        let escaped = imp::escape_applescript("say \"hi\" \\ there");
        assert!(escaped.contains("\\\""));
        assert!(escaped.contains("\\\\"));
    }
}
