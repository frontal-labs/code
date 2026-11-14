//! Syntax highlighting for code blocks and file contents in the TUI.
//!
//! Uses `syntect`'s bundled grammars and a dark theme. The grammar set is loaded once, on
//! first use, because parsing the bundled definitions takes a noticeable moment.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

const THEME: &str = "base16-ocean.dark";

struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
}

fn assets() -> &'static Assets {
    static ASSETS: OnceLock<Assets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let themes = ThemeSet::load_defaults();
        let theme = themes
            .themes
            .get(THEME)
            .or_else(|| themes.themes.values().next())
            .cloned()
            .unwrap_or_default();
        Assets {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme,
        }
    })
}

/// Highlights `code` using the grammar for a fence language such as `rust` or `py`.
/// Unknown languages fall back to plain text.
pub fn code_block(code: &str, language: &str) -> Vec<Line<'static>> {
    let assets = assets();
    let syntax = find_syntax(&assets.syntaxes, language);
    highlight(code, syntax, &assets.theme)
}

/// Highlights a file's contents, choosing the grammar from its extension.
pub fn file_contents(content: &str, path: &str) -> Vec<Line<'static>> {
    let assets = assets();
    let extension = path.rsplit('.').next().unwrap_or_default();
    let syntax = assets
        .syntaxes
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| assets.syntaxes.find_syntax_plain_text());
    highlight(content, syntax, &assets.theme)
}

fn find_syntax<'a>(syntaxes: &'a SyntaxSet, language: &str) -> &'a SyntaxReference {
    let token = language.trim();
    if token.is_empty() {
        return syntaxes.find_syntax_plain_text();
    }
    syntaxes
        .find_syntax_by_token(token)
        .or_else(|| syntaxes.find_syntax_by_extension(token))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

fn highlight(code: &str, syntax: &SyntaxReference, theme: &Theme) -> Vec<Line<'static>> {
    let assets = assets();
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();
    for line in code.lines() {
        let with_newline = format!("{line}\n");
        match highlighter.highlight_line(&with_newline, &assets.syntaxes) {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        Span::styled(
                            text.trim_end_matches('\n').to_string(),
                            convert_style(style.foreground, style.font_style),
                        )
                    })
                    .filter(|span| !span.content.is_empty())
                    .collect();
                lines.push(Line::from(spans));
            }
            Err(_) => lines.push(Line::from(line.to_string())),
        }
    }
    lines
}

fn convert_style(foreground: syntect::highlighting::Color, font: FontStyle) -> Style {
    let mut style = Style::default().fg(Color::Rgb(foreground.r, foreground.g, foreground.b));
    if font.contains(FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if font.contains(FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if font.contains(FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_keywords_get_their_own_colored_spans() {
        let lines = code_block("fn main() {}", "rust");
        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        assert!(
            spans.len() > 1,
            "keywords and identifiers should be separate spans"
        );
        assert!(spans
            .iter()
            .any(|span| matches!(span.style.fg, Some(Color::Rgb(..)))));
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "fn main() {}");
    }

    #[test]
    fn unknown_language_keeps_the_text() {
        let lines = code_block("hello world", "not-a-language");
        let text: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "hello world");
    }

    #[test]
    fn file_extension_selects_the_grammar() {
        let lines = file_contents("def main():\n    pass", "tool.py");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].spans.len() > 1);
    }
}
