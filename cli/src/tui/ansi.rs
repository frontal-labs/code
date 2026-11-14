//! Converts the REPL's ANSI-styled text into ratatui lines.
//!
//! The TUI shows the same strings the line-based REPL prints (banner, tool headlines,
//! results, summaries, shortcuts), so the two interfaces share one format. This module
//! understands the SGR codes those strings use and drops every other control sequence.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Splits `text` into lines, converting SGR styling into span styles.
pub fn lines(text: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = text.split('\n').map(line).collect();
    // A trailing newline ends the last line rather than starting an empty one.
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn line(text: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut buffer = String::new();
    let mut chars = text.chars().peekable();

    while let Some(character) = chars.next() {
        if character == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            let mut params = String::new();
            let mut terminator = None;
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    terminator = Some(next);
                    break;
                }
                params.push(next);
            }
            if terminator == Some('m') {
                if !buffer.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buffer), style));
                }
                style = apply_sgr(style, &params);
            }
            continue;
        }
        buffer.push(character);
    }

    if !buffer.is_empty() || spans.is_empty() {
        spans.push(Span::styled(buffer, style));
    }
    Line::from(spans)
}

fn apply_sgr(mut style: Style, params: &str) -> Style {
    let codes: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params
            .split(';')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };

    let mut index = 0;
    while index < codes.len() {
        match codes[index] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            30..=37 => style = style.fg(Color::Indexed(index_color(codes[index] - 30))),
            39 => style = style.fg(Color::Reset),
            40..=47 => style = style.bg(Color::Indexed(index_color(codes[index] - 40))),
            49 => style = style.bg(Color::Reset),
            90..=97 => style = style.fg(Color::Indexed(index_color(codes[index] - 90) + 8)),
            100..=107 => style = style.bg(Color::Indexed(index_color(codes[index] - 100) + 8)),
            38 | 48 => {
                let is_fg = codes[index] == 38;
                if codes.get(index + 1) == Some(&5) {
                    if let Some(&value) = codes.get(index + 2) {
                        let color = Color::Indexed(u8::try_from(value).unwrap_or(0));
                        style = if is_fg {
                            style.fg(color)
                        } else {
                            style.bg(color)
                        };
                    }
                    index += 2;
                } else if codes.get(index + 1) == Some(&2) {
                    let red = codes.get(index + 2).copied().unwrap_or(0);
                    let green = codes.get(index + 3).copied().unwrap_or(0);
                    let blue = codes.get(index + 4).copied().unwrap_or(0);
                    let color = Color::Rgb(
                        u8::try_from(red).unwrap_or(0),
                        u8::try_from(green).unwrap_or(0),
                        u8::try_from(blue).unwrap_or(0),
                    );
                    style = if is_fg {
                        style.fg(color)
                    } else {
                        style.bg(color)
                    };
                    index += 4;
                }
            }
            _ => {}
        }
        index += 1;
    }
    style
}

/// Maps the eight basic ANSI color numbers to their 256-color index.
fn index_color(basic: u16) -> u8 {
    u8::try_from(basic).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::lines;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn keeps_plain_text_and_drops_trailing_newline() {
        let converted = lines("one\ntwo\n");
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[1].spans[0].content, "two");
    }

    #[test]
    fn applies_bold_dim_and_indexed_colors() {
        let converted = lines("\x1b[1mbold\x1b[0m \x1b[2mdim\x1b[0m \x1b[38;5;33mblue\x1b[0m");
        let spans = &converted[0].spans;
        assert_eq!(spans[0].content, "bold");
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(spans[2].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(spans[4].style.fg, Some(Color::Indexed(33)));
    }

    #[test]
    fn strips_cursor_sequences_it_does_not_render() {
        let converted = lines("\x1b[2Kready");
        assert_eq!(converted[0].spans[0].content, "ready");
    }
}
