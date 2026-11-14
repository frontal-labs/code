//! Markdown rendering for assistant replies in the TUI.
//!
//! Parses with `pulldown-cmark` and builds styled lines directly: headings, emphasis,
//! inline code, links, lists, block quotes, rules, tables, and fenced code blocks. Code
//! blocks are highlighted with `syntect` (see [`super::highlight`]).

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::highlight;

const CODE_COLOR: Color = Color::Indexed(180);
const LINK_COLOR: Color = Color::Indexed(75);
const QUOTE_COLOR: Color = Color::Indexed(245);

/// Renders markdown `source` to styled lines.
pub fn render(source: &str) -> Vec<Line<'static>> {
    let mut renderer = Renderer::default();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    for event in Parser::new_ext(source, options) {
        renderer.event(event);
    }
    renderer.finish()
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    styles: Vec<Style>,
    list_stack: Vec<Option<u64>>,
    quote_depth: usize,
    code: Option<(String, String)>,
    link: Option<String>,
    table: Option<Vec<Vec<String>>>,
    table_row: Vec<String>,
    table_cell: String,
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles
            .iter()
            .fold(Style::default(), |acc, style| acc.patch(*style))
    }

    fn push_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    fn pop_style(&mut self) {
        self.styles.pop();
    }

    fn flush_line(&mut self) {
        let mut spans = Vec::new();
        if self.quote_depth > 0 {
            spans.push(Span::styled(
                "│ ".repeat(self.quote_depth),
                Style::default().fg(QUOTE_COLOR),
            ));
        }
        spans.append(&mut self.current);
        self.lines.push(Line::from(spans));
    }

    fn blank_line(&mut self) {
        if self.lines.last().is_some_and(|line| !line.spans.is_empty()) {
            self.lines.push(Line::default());
        }
    }

    fn text(&mut self, text: &str) {
        if let Some((_, buffer)) = &mut self.code {
            buffer.push_str(text);
            return;
        }
        if self.table.is_some() {
            self.table_cell.push_str(text);
            return;
        }
        let mut style = self.style();
        if self.link.is_some() {
            style = style.fg(LINK_COLOR).add_modifier(Modifier::UNDERLINED);
        }
        self.current.push(Span::styled(text.to_string(), style));
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                if self.table.is_some() {
                    self.table_cell.push_str(&code);
                } else {
                    self.current.push(Span::styled(
                        code.to_string(),
                        Style::default().fg(CODE_COLOR),
                    ));
                }
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush_line(),
            Event::Rule => {
                self.blank_line();
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().add_modifier(Modifier::DIM),
                )));
            }
            Event::TaskListMarker(done) => {
                self.current
                    .push(Span::raw(if done { "[x] " } else { "[ ] " }));
            }
            Event::Html(html) | Event::InlineHtml(html) => self.text(&html),
            Event::FootnoteReference(name) => self.text(&format!("[{name}]")),
            Event::InlineMath(math) | Event::DisplayMath(math) => self.text(&math),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.blank_line();
                let style = match level {
                    HeadingLevel::H1 => {
                        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    }
                    HeadingLevel::H2 => Style::default().add_modifier(Modifier::BOLD),
                    _ => Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
                };
                self.push_style(style);
            }
            Tag::BlockQuote(_) => {
                self.blank_line();
                self.quote_depth += 1;
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.blank_line();
                let language = match kind {
                    CodeBlockKind::Fenced(language) => language.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
            }
            Tag::List(start) => {
                if self.list_stack.is_empty() {
                    self.blank_line();
                }
                self.list_stack.push(start);
            }
            Tag::Item => {
                let depth = self.list_stack.len().saturating_sub(1);
                let marker = match self.list_stack.last_mut() {
                    Some(Some(next)) => {
                        let marker = format!("{next}. ");
                        *next += 1;
                        marker
                    }
                    _ => "• ".to_string(),
                };
                self.current
                    .push(Span::raw(format!("{}{marker}", "  ".repeat(depth))));
            }
            Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { dest_url, .. } => self.link = Some(dest_url.to_string()),
            Tag::Table(_) => {
                self.blank_line();
                self.table = Some(Vec::new());
            }
            Tag::TableHead | Tag::TableRow => self.table_row = Vec::new(),
            Tag::TableCell => self.table_cell = String::new(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
                self.blank_line();
            }
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_line();
                self.blank_line();
            }
            TagEnd::BlockQuote(_) => {
                self.pop_style();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.blank_line();
            }
            TagEnd::CodeBlock => {
                if let Some((language, code)) = self.code.take() {
                    let label = if language.is_empty() {
                        "code".to_string()
                    } else {
                        language.clone()
                    };
                    self.lines.push(Line::from(Span::styled(
                        format!("╭─ {label}"),
                        Style::default().add_modifier(Modifier::DIM),
                    )));
                    for highlighted in highlight::code_block(&code, &language) {
                        let mut spans = vec![Span::styled(
                            "│ ",
                            Style::default().add_modifier(Modifier::DIM),
                        )];
                        spans.extend(highlighted.spans);
                        self.lines.push(Line::from(spans));
                    }
                    self.lines.push(Line::from(Span::styled(
                        "╰─".to_string(),
                        Style::default().add_modifier(Modifier::DIM),
                    )));
                    self.blank_line();
                }
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.blank_line();
            }
            TagEnd::Item => self.flush_line(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.link = None;
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.table_cell);
                self.table_row.push(cell);
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                let row = std::mem::take(&mut self.table_row);
                if let Some(table) = &mut self.table {
                    table.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(rows) = self.table.take() {
                    self.render_table(&rows);
                }
                self.blank_line();
            }
            _ => {}
        }
    }

    fn render_table(&mut self, rows: &[Vec<String>]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|column| {
                rows.iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| cell.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        for (index, row) in rows.iter().enumerate() {
            let mut text = String::from("│");
            for (column, width) in widths.iter().enumerate() {
                let cell = row.get(column).map_or("", String::as_str);
                text.push(' ');
                text.push_str(cell);
                text.push_str(&" ".repeat(width.saturating_sub(cell.chars().count())));
                text.push_str(" │");
            }
            let style = if index == 0 {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            self.lines.push(Line::from(Span::styled(text, style)));
            if index == 0 && rows.len() > 1 {
                let rule: String = widths
                    .iter()
                    .map(|width| "─".repeat(width + 2))
                    .collect::<Vec<_>>()
                    .join("┼");
                self.lines.push(Line::from(Span::styled(
                    format!("├{rule}┤"),
                    Style::default().add_modifier(Modifier::DIM),
                )));
            }
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.current.is_empty() {
            self.flush_line();
        }
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
        }
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn renders_headings_lists_and_emphasis() {
        let lines = render("# Title\n\n- one\n- **two**");
        let rendered = text(&lines);
        assert_eq!(rendered[0], "Title");
        assert!(rendered.iter().any(|line| line == "• one"));
        assert!(rendered.iter().any(|line| line == "• two"));
        let bold = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == "two")
            .expect("bold span");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn fenced_code_is_framed_and_highlighted() {
        let lines = render("```rust\nfn main() {}\n```");
        let rendered = text(&lines);
        assert_eq!(rendered[0], "╭─ rust");
        assert!(rendered.iter().any(|line| line == "│ fn main() {}"));
        assert_eq!(rendered.last().map(String::as_str), Some("╰─"));
        let code_line = lines
            .iter()
            .find(|line| line.spans.iter().any(|span| span.content.contains("main")))
            .expect("code line");
        assert!(
            code_line.spans.len() > 2,
            "keywords should be colored separately"
        );
    }

    #[test]
    fn inline_code_and_quotes_are_styled() {
        let lines = render("> quoted `x` text");
        let rendered = text(&lines);
        assert!(rendered[0].starts_with("│ "));
        assert!(rendered[0].contains("quoted"));
        assert!(lines[0].spans.iter().any(|span| span.content == "x"));
    }

    #[test]
    fn tables_are_drawn_with_separators() {
        let rendered = text(&render("| a | b |\n| - | - |\n| 1 | 2 |"));
        assert!(rendered.iter().any(|line| line.starts_with("│ a")));
        assert!(rendered.iter().any(|line| line.starts_with("├")));
    }
}
