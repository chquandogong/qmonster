use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::config::HelpLanguage;
use crate::ui::help_glossary::{HelpTopic, help_lines, language_label};
use crate::ui::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverHelpView {
    pub topic: HelpTopic,
    pub language: HelpLanguage,
    pub column: u16,
    pub row: u16,
}

pub fn render_hover_help(frame: &mut Frame<'_>, view: HoverHelpView) {
    let area = tooltip_rect(frame.area(), view);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let title = format!(" Help · {} ", language_label(view.language));
    let lines: Vec<Line<'static>> = help_lines(view.topic, view.language)
        .iter()
        .map(|line| Line::from(Span::raw((*line).to_string())))
        .collect();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::BORDER_ACTIVE))
                    .title(Span::styled(
                        title,
                        Style::default()
                            .fg(theme::TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .style(Style::default().fg(theme::TEXT_PRIMARY))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn tooltip_rect(viewport: Rect, view: HoverHelpView) -> Rect {
    let lines = help_lines(view.topic, view.language);
    let width = lines
        .iter()
        .map(|line| line.chars().count() as u16)
        .max()
        .unwrap_or(20)
        .saturating_add(4)
        .clamp(24, viewport.width.min(72));
    let height = (lines.len() as u16)
        .saturating_add(2)
        .clamp(3, viewport.height.max(3));
    let mut x = view.column.saturating_add(2);
    if x.saturating_add(width) > viewport.x.saturating_add(viewport.width) {
        x = view.column.saturating_sub(width.saturating_add(1));
    }
    let mut y = view.row.saturating_add(1);
    if y.saturating_add(height) > viewport.y.saturating_add(viewport.height) {
        y = view.row.saturating_sub(height.saturating_add(1));
    }
    Rect::new(x.max(viewport.x), y.max(viewport.y), width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_rect_stays_inside_viewport_near_bottom_right() {
        let viewport = Rect::new(0, 0, 80, 24);
        let rect = tooltip_rect(
            viewport,
            HoverHelpView {
                topic: HelpTopic::PaneMetrics,
                language: HelpLanguage::En,
                column: 78,
                row: 23,
            },
        );

        assert!(rect.x + rect.width <= viewport.width);
        assert!(rect.y + rect.height <= viewport.height);
    }

    #[test]
    fn tooltip_rect_height_accounts_for_wrapped_content() {
        let viewport = Rect::new(0, 0, 42, 18);
        let view = HoverHelpView {
            topic: HelpTopic::PaneRuntime,
            language: HelpLanguage::Ko,
            column: 2,
            row: 2,
        };

        let rect = tooltip_rect(viewport, view);
        let content_width = rect.width.saturating_sub(2).max(1) as usize;
        let wrapped_rows: usize = help_lines(view.topic, view.language)
            .iter()
            .map(|line| line.chars().count().div_ceil(content_width).max(1))
            .sum();

        assert!(
            rect.height as usize >= wrapped_rows.saturating_add(2).min(viewport.height as usize),
            "tooltip must reserve enough height for wrapped help text; rect={rect:?}, wrapped_rows={wrapped_rows}"
        );
    }
}
