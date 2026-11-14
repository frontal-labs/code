//! Reusable TUI widgets: a selector (dropdown and palette), buttons, the loader, and the
//! centered overlay helper.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

/// What a selector's choice is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Model,
    Permission,
    Session,
    Command,
}

/// One row in a selector.
#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    pub detail: String,
    /// The text the chosen item sends: a model name, a mode, a session id, or a command.
    pub value: String,
}

/// A keyboard-driven list: a dropdown when opened from a button, a palette when opened
/// with Ctrl-K.
#[derive(Debug, Clone)]
pub struct Selector {
    pub title: String,
    pub purpose: Purpose,
    pub items: Vec<Item>,
    pub selected: usize,
}

impl Selector {
    pub fn new(title: &str, purpose: Purpose, items: Vec<Item>) -> Self {
        Self {
            title: title.to_string(),
            purpose,
            items,
            selected: 0,
        }
    }

    /// Moves the highlight by `delta`, wrapping at both ends.
    pub fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let len = isize::try_from(self.items.len()).unwrap_or(isize::MAX);
        let next = (isize::try_from(self.selected).unwrap_or(0) + delta).rem_euclid(len);
        self.selected = usize::try_from(next).unwrap_or(0);
    }

    pub fn chosen(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }
}

/// The 1-line loader logo: two checkered squares that swap each frame, so the mark
/// seems to breathe while the model works.
pub fn loader_logo(frame_index: usize) -> &'static str {
    const FRAMES: [&str; 2] = ["▀▄▀", "▄▀▄"];
    FRAMES[(frame_index / 4) % FRAMES.len()]
}

/// A button label. Focused buttons are shown in reverse video.
pub fn button(label: &str, focused: bool) -> Span<'static> {
    let style = if focused {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    Span::styled(format!(" {label} "), style)
}

/// A rectangle of `width` by `height` centered in `area`, clamped to fit.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Draws a selector over `area`.
pub fn render_selector(frame: &mut Frame, area: Rect, selector: &Selector) {
    let width = area.width.saturating_sub(4).clamp(20, 80);
    let height = u16::try_from(selector.items.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .min(area.height.saturating_sub(2))
        .max(3);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);

    let rows: Vec<ListItem> = selector
        .items
        .iter()
        .map(|item| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:<28}", truncate(&item.label, 28)),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    truncate(&item.detail, usize::from(width).saturating_sub(34)),
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]))
        })
        .collect();
    let list = List::new(rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(
                    " {} · ↑↓ move · Enter choose · Esc close ",
                    selector.title
                )),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(selector.selected));
    frame.render_stateful_widget(list, popup, &mut state);
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// A dim one-line hint, e.g. the key help under a selector.
pub fn hint(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().add_modifier(Modifier::DIM),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(count: usize) -> Vec<Item> {
        (0..count)
            .map(|index| Item {
                label: format!("item {index}"),
                detail: String::new(),
                value: index.to_string(),
            })
            .collect()
    }

    #[test]
    fn selector_moves_and_wraps() {
        let mut selector = Selector::new("t", Purpose::Model, items(3));
        selector.move_by(1);
        assert_eq!(selector.selected, 1);
        selector.move_by(-2);
        assert_eq!(selector.selected, 2);
        selector.move_by(1);
        assert_eq!(selector.selected, 0);
        assert_eq!(selector.chosen().map(|item| item.value.as_str()), Some("0"));
    }

    #[test]
    fn empty_selector_is_safe() {
        let mut selector = Selector::new("t", Purpose::Command, Vec::new());
        selector.move_by(1);
        assert!(selector.chosen().is_none());
    }

    #[test]
    fn loader_logo_alternates() {
        assert_ne!(loader_logo(0), loader_logo(4));
        assert_eq!(loader_logo(0), loader_logo(8));
    }

    #[test]
    fn centered_fits_inside_the_area() {
        let area = Rect::new(0, 0, 40, 10);
        let rect = centered(area, 60, 20);
        assert!(rect.width <= area.width && rect.height <= area.height);
        assert_eq!(rect.x, 0);
    }
}
