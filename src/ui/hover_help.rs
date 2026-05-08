use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::config::HelpLanguage;
use crate::ui::help_glossary::{HelpTopic, help_lines, language_label};
use crate::ui::theme;

const RIGHT_GUTTER: u16 = 2;
const MIN_TOOLTIP_WIDTH: u16 = 32;
const MAX_TOOLTIP_WIDTH: u16 = 88;
const KEY_LEGEND_MIN_WIDTH: u16 = 96;
const KEY_LEGEND_MAX_WIDTH: u16 = 176;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverHelpView {
    pub topic: HelpTopic,
    pub language: HelpLanguage,
    pub column: u16,
    pub row: u16,
}

pub fn render_hover_help(frame: &mut Frame<'_>, view: HoverHelpView) {
    if view.topic == HelpTopic::DashboardFooter {
        render_footer_key_legend(frame, view);
        return;
    }

    let area = tooltip_rect(frame.area(), view);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let title = format!(" Help · {} · H/L ", language_label(view.language));
    let lines = tooltip_lines(view);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::border_active()))
                    .title(Span::styled(
                        title,
                        Style::default()
                            .fg(theme::text_primary())
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .style(Style::default().fg(theme::text_primary()))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_footer_key_legend(frame: &mut Frame<'_>, view: HoverHelpView) {
    let area = footer_key_legend_rect(frame.area(), view);
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(footer_key_legend_lines(view.language))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::border_active())),
            )
            .style(Style::default().fg(theme::text_primary()))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn footer_key_legend_rect(viewport: Rect, view: HoverHelpView) -> Rect {
    let max_width = viewport
        .width
        .saturating_sub(RIGHT_GUTTER.saturating_mul(2))
        .max(1);
    let width = KEY_LEGEND_MAX_WIDTH
        .min(max_width)
        .max(KEY_LEGEND_MIN_WIDTH.min(max_width));
    let height = footer_key_legend_height(viewport, width, view.language);
    let right_limit = viewport
        .x
        .saturating_add(viewport.width)
        .saturating_sub(RIGHT_GUTTER);
    let mut x = view.column.saturating_sub(1).max(viewport.x);
    if x.saturating_add(width) > right_limit {
        x = right_limit.saturating_sub(width).max(viewport.x);
    }
    let mut y = view.row.saturating_sub(height.saturating_add(1));
    if y < viewport.y {
        y = view.row.saturating_add(1);
    }
    if y.saturating_add(height) > viewport.y.saturating_add(viewport.height) {
        y = viewport
            .y
            .saturating_add(viewport.height.saturating_sub(height));
    }
    Rect::new(x, y, width, height.min(viewport.height))
}

fn footer_key_legend_height(viewport: Rect, width: u16, language: HelpLanguage) -> u16 {
    let content_width = width.saturating_sub(2).max(1) as usize;
    let rows: usize = footer_key_legend_text_lines(language)
        .iter()
        .map(|line| UnicodeWidthStr::width(*line).div_ceil(content_width).max(1))
        .sum();
    (rows as u16)
        .saturating_add(2)
        .clamp(3, viewport.height.max(3))
}

fn tooltip_rect(viewport: Rect, view: HoverHelpView) -> Rect {
    let lines = tooltip_text_lines(view);
    let width = tooltip_width(viewport, &lines);
    let height = tooltip_height(viewport, width, &lines);
    if viewport.width < 64 || viewport.height < height.saturating_add(3) {
        return bottom_drawer_rect(viewport, height);
    }

    let right_limit = viewport
        .x
        .saturating_add(viewport.width)
        .saturating_sub(RIGHT_GUTTER);
    let mut x = view.column.saturating_add(2);
    if x.saturating_add(width) > right_limit {
        x = view.column.saturating_sub(width.saturating_add(1));
    }
    x = x.max(viewport.x);
    if x.saturating_add(width) > right_limit {
        x = right_limit.saturating_sub(width).max(viewport.x);
    }
    let mut y = view.row.saturating_add(1);
    if y.saturating_add(height) > viewport.y.saturating_add(viewport.height) {
        y = view.row.saturating_sub(height.saturating_add(1));
    }
    Rect::new(x, y.max(viewport.y), width, height)
}

fn tooltip_width(viewport: Rect, lines: &[&'static str]) -> u16 {
    let max_width = viewport
        .width
        .saturating_sub(RIGHT_GUTTER)
        .clamp(1, MAX_TOOLTIP_WIDTH);
    let min_width = MIN_TOOLTIP_WIDTH.min(max_width);
    let desired = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(*line) as u16)
        .max()
        .unwrap_or(20)
        .saturating_add(4);
    desired.clamp(min_width, max_width)
}

fn tooltip_height(viewport: Rect, width: u16, lines: &[&'static str]) -> u16 {
    let content_width = width.saturating_sub(2).max(1) as usize;
    let wrapped_rows: usize = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(*line).div_ceil(content_width).max(1))
        .sum();
    (wrapped_rows as u16)
        .saturating_add(2)
        .clamp(3, viewport.height.max(3))
}

fn bottom_drawer_rect(viewport: Rect, height: u16) -> Rect {
    let height = height.min(viewport.height);
    let width = viewport.width.saturating_sub(RIGHT_GUTTER);
    Rect::new(
        viewport.x,
        viewport
            .y
            .saturating_add(viewport.height.saturating_sub(height)),
        width,
        height,
    )
}

fn tooltip_lines(view: HoverHelpView) -> Vec<Line<'static>> {
    tooltip_text_lines(view)
        .into_iter()
        .map(|line| Line::from(Span::raw(line.to_string())))
        .collect()
}

fn tooltip_text_lines(view: HoverHelpView) -> Vec<&'static str> {
    let mut out = help_lines(view.topic, view.language).to_vec();
    out.push("");
    out.push(match view.language {
        HelpLanguage::Ko => "H: 도움말 켜기/끄기 · L: 한국어/영어 · S: Settings에서 저장",
        HelpLanguage::En => "H: toggle help · L: switch language · S: save defaults in Settings",
    });
    out
}

fn footer_key_legend_text_lines(language: HelpLanguage) -> &'static [&'static str] {
    match language {
        HelpLanguage::Ko => &[
            "Move      ↑/↓ item · PgUp/PgDn page · Home/End · Tab focus",
            "Layout    [ ] resize · / cycle · = reset · wheel scroll · click select · click severity bulk hide · click version git",
            "Actions   t target · u runtime · s snapshot · y copy · c clear · p accept · d dismiss",
            "Overlays  S settings · P provider-setup · m metrics · n anomalies · a actions · i insights · Q fx · K keys · ? help · q quit",
        ],
        HelpLanguage::En => &[
            "Move      ↑/↓ item · PgUp/PgDn page · Home/End · Tab focus",
            "Layout    [ ] resize · / cycle · = reset · wheel scroll · click select · click severity bulk hide · click version git",
            "Actions   t target · u runtime · s snapshot · y copy · c clear · p accept · d dismiss",
            "Overlays  S settings · P provider-setup · m metrics · n anomalies · a actions · i insights · Q fx · K keys · ? help · q quit",
        ],
    }
}

fn footer_key_legend_lines(language: HelpLanguage) -> Vec<Line<'static>> {
    footer_key_legend_text_lines(language)
        .iter()
        .map(|line| {
            let (label, rest) = split_legend_label(line);
            Line::from(vec![
                Span::styled(
                    label.to_string(),
                    Style::default()
                        .fg(theme::text_primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(rest.to_string()),
            ])
        })
        .collect()
}

fn split_legend_label(line: &'static str) -> (&'static str, &'static str) {
    let split = line.find("  ").unwrap_or(line.len());
    line.split_at(split)
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
    fn tooltip_rect_leaves_a_right_gutter_near_viewport_edge() {
        let viewport = Rect::new(0, 0, 80, 24);
        let rect = tooltip_rect(
            viewport,
            HoverHelpView {
                topic: HelpTopic::PaneState,
                language: HelpLanguage::En,
                column: 76,
                row: 8,
            },
        );

        assert!(
            rect.x + rect.width <= viewport.width.saturating_sub(2),
            "hover help should leave a small right gutter; rect={rect:?}"
        );
    }

    #[test]
    fn bottom_drawer_leaves_a_right_gutter() {
        let viewport = Rect::new(0, 0, 63, 18);
        let rect = bottom_drawer_rect(viewport, 8);

        assert!(
            rect.x + rect.width <= viewport.width.saturating_sub(2),
            "bottom drawer should leave a small right gutter; rect={rect:?}"
        );
    }

    #[test]
    fn tooltip_width_uses_display_columns_for_korean_text() {
        let viewport = Rect::new(0, 0, 80, 24);
        let lines = ["가나다라마바사아자차카타파하가나다라마바사아자차카타파하"];

        assert_eq!(tooltip_width(viewport, &lines), 60);
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

    #[test]
    fn tooltip_height_uses_display_columns_for_korean_text() {
        let viewport = Rect::new(0, 0, 80, 20);
        let lines = ["가나다라마바사아자차카타파하"];

        assert_eq!(tooltip_height(viewport, 12, &lines), 5);
    }

    #[test]
    fn footer_key_legend_includes_full_dashboard_key_reference() {
        let lines = footer_key_legend_lines(HelpLanguage::En);
        let joined = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("click severity bulk hide"));
        assert!(joined.contains("Q fx"));
        assert!(joined.contains("S settings"));
    }

    #[test]
    fn footer_key_legend_uses_wide_rect() {
        let viewport = Rect::new(0, 0, 200, 40);
        let rect = footer_key_legend_rect(
            viewport,
            HoverHelpView {
                topic: HelpTopic::DashboardFooter,
                language: HelpLanguage::En,
                column: 1,
                row: 39,
            },
        );

        assert!(
            rect.width >= 160,
            "keys legend should be roughly twice the normal tooltip width; rect={rect:?}"
        );
        assert!(rect.x + rect.width <= viewport.width.saturating_sub(2));
    }

    #[test]
    fn footer_key_legend_omits_generic_hover_chrome_lines() {
        let lines = footer_key_legend_lines(HelpLanguage::En);
        let joined = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(!joined.contains("Key legend:"));
        assert!(!joined.contains("H:"));
        assert!(!joined.contains("L:"));
        assert!(!joined.contains("S: save"));
        assert!(joined.contains("Move"));
        assert!(joined.contains("Layout"));
        assert!(joined.contains("Actions"));
        assert!(joined.contains("Overlays"));
    }

    #[test]
    fn footer_key_legend_render_omits_title_and_bottom_hint() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(160, 32)).unwrap();
        terminal
            .draw(|frame| {
                render_hover_help(
                    frame,
                    HoverHelpView {
                        topic: HelpTopic::DashboardFooter,
                        language: HelpLanguage::En,
                        column: 1,
                        row: 31,
                    },
                );
            })
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(rendered.contains("Move"));
        assert!(!rendered.contains("Help ·"));
        assert!(!rendered.contains("H: toggle help"));
        assert!(!rendered.contains("S: save defaults"));
    }
}
