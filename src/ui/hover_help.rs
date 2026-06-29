use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::config::HelpLanguage;
use crate::ui::help_glossary::{HelpTopic, help_lines, language_label};
use crate::ui::theme;

/// Unified right-edge gutter for every hover help surface (floating
/// tooltip, footer key legend, and the small-terminal bottom drawer
/// fallback). Keeping a single constant guarantees identical breathing
/// room between the rendered box and the viewport edge across surfaces.
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
    let inner_width = inner_content_width(area);
    let lines = tooltip_lines(view, inner_width);
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
            .style(Style::default().fg(theme::text_primary())),
        area,
    );
}

fn render_footer_key_legend(frame: &mut Frame<'_>, view: HoverHelpView) {
    let area = footer_key_legend_rect(frame.area(), view);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let inner_width = inner_content_width(area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(footer_key_legend_lines(view.language, inner_width))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::border_active())),
            )
            .style(Style::default().fg(theme::text_primary())),
        area,
    );
}

/// Inner-content cell budget for a bordered hover surface. The block
/// reserves 1 cell on each side for the vertical border, so usable
/// columns are `area.width - 2`. The manual wrap routine treats this
/// as a hard ceiling — never letting a double-width glyph straddle
/// the right border cell.
fn inner_content_width(area: Rect) -> usize {
    area.width.saturating_sub(2).max(1) as usize
}

/// Width-aware line wrapper. Splits on spaces first, falls back to
/// per-character break for words wider than `content_width` so a
/// long unbroken token (long English identifier, URL, etc.) cannot
/// overflow either. Uses `UnicodeWidthChar` so double-width CJK glyphs
/// are budgeted as 2 cells, which is what `ratatui`'s built-in
/// `Wrap` mis-handles when a CJK character lands exactly on the
/// boundary (it then bleeds into the right border cell, which is the
/// visible "text past the right line" symptom this routine fixes).
fn wrap_to_width(text: &str, content_width: usize) -> Vec<String> {
    if content_width == 0 {
        return vec![String::new()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    for word in split_keep_spaces(text) {
        let word_width = UnicodeWidthStr::width(word);
        let word_is_only_spaces = word.chars().all(|c| c == ' ');

        if word_is_only_spaces {
            if current_width + word_width <= content_width {
                current.push_str(word);
                current_width += word_width;
            } else {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            continue;
        }

        if current_width + word_width > content_width && current_width > 0 {
            let trimmed = current.trim_end().to_string();
            lines.push(trimmed);
            current.clear();
            current_width = 0;
        }

        if word_width > content_width {
            for ch in word.chars() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if current_width + cw > content_width && current_width > 0 {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                current.push(ch);
                current_width += cw;
            }
        } else {
            current.push_str(word);
            current_width += word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Helper for `wrap_to_width`: chunk `text` into a sequence of words and
/// inter-word space runs, preserving the original spacing so the
/// wrapper can decide whether to keep or drop trailing spaces.
fn split_keep_spaces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        let start = idx;
        let in_space = bytes[idx] == b' ';
        while idx < bytes.len() && (bytes[idx] == b' ') == in_space {
            idx += 1;
        }
        out.push(&text[start..idx]);
    }
    out
}

fn footer_key_legend_rect(viewport: Rect, view: HoverHelpView) -> Rect {
    let max_width = viewport.width.saturating_sub(RIGHT_GUTTER).max(1);
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
        .map(|line| wrap_to_width(line, content_width).len())
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
        .map(|line| wrap_to_width(line, content_width).len().max(1))
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

fn tooltip_lines(view: HoverHelpView, content_width: usize) -> Vec<Line<'static>> {
    tooltip_text_lines(view)
        .into_iter()
        .flat_map(|line| wrap_to_width(line, content_width))
        .map(|line| Line::from(Span::raw(line)))
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

fn footer_key_legend_lines(language: HelpLanguage, content_width: usize) -> Vec<Line<'static>> {
    footer_key_legend_text_lines(language)
        .iter()
        .flat_map(|line| {
            let wrapped = wrap_to_width(line, content_width);
            wrapped
                .into_iter()
                .enumerate()
                .map(|(idx, wrapped_line)| {
                    if idx == 0 {
                        let (label, rest) = split_legend_label(&wrapped_line);
                        Line::from(vec![
                            Span::styled(
                                label.to_string(),
                                Style::default()
                                    .fg(theme::text_primary())
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(rest.to_string()),
                        ])
                    } else {
                        Line::from(Span::raw(wrapped_line))
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn split_legend_label(line: &str) -> (&str, &str) {
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
        let lines = footer_key_legend_lines(HelpLanguage::En, KEY_LEGEND_MAX_WIDTH as usize);
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
        let lines = footer_key_legend_lines(HelpLanguage::En, KEY_LEGEND_MAX_WIDTH as usize);
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

    #[test]
    fn wrap_to_width_breaks_korean_at_word_boundary() {
        // Reproduces the original bug input: a Korean+ASCII mix that is
        // exactly 1 display column wider than the box's inner content
        // area. Before the fix, ratatui's Wrap kept it on one line and
        // the trailing "로" double-wide glyph overran the right border.
        let text = "입력/승인 대기, Risk 추천, quota/cost 압박, 최근 anomaly, healthy 상태 순서로";
        let wrapped = wrap_to_width(text, 76);
        assert!(wrapped.len() >= 2, "expected wrap; got {wrapped:?}");
        for line in &wrapped {
            assert!(
                UnicodeWidthStr::width(line.as_str()) <= 76,
                "wrapped line exceeds budget: {line:?} ({} cols)",
                UnicodeWidthStr::width(line.as_str())
            );
        }
    }

    #[test]
    fn wrap_to_width_breaks_long_unbroken_token() {
        // A long ASCII token wider than the budget must still wrap.
        let text = "prefix abcdefghijklmnopqrstuvwxyz0123456789-extra suffix";
        let wrapped = wrap_to_width(text, 12);
        for line in &wrapped {
            assert!(
                UnicodeWidthStr::width(line.as_str()) <= 12,
                "wrapped line exceeds budget: {line:?} ({} cols)",
                UnicodeWidthStr::width(line.as_str())
            );
        }
    }

    fn all_topics() -> &'static [HelpTopic] {
        &[
            HelpTopic::AlertBulkHide,
            HelpTopic::AlertHeader,
            HelpTopic::AlertDismiss,
            HelpTopic::AlertSummary,
            HelpTopic::AlertDetail,
            HelpTopic::AlertCopy,
            HelpTopic::PaneHeader,
            HelpTopic::PaneState,
            HelpTopic::PanePath,
            HelpTopic::PaneCommand,
            HelpTopic::PaneStatus,
            HelpTopic::PaneSignals,
            HelpTopic::PaneMetrics,
            HelpTopic::PaneTokens,
            HelpTopic::PaneRuntime,
            HelpTopic::PaneRecommendation,
            HelpTopic::DashboardDivider,
            HelpTopic::DashboardFooter,
            HelpTopic::DashboardVersionBadge,
            HelpTopic::DashboardNowStrip,
            HelpTopic::DashboardFooterProposalChip,
            HelpTopic::DashboardFooterCopyChip,
            HelpTopic::DashboardFooterAuditChip,
        ]
    }

    /// Audit helper: render every (topic, language) pair at a given viewport
    /// width and return a list of `(topic, language, max_used_column)` for any
    /// frame whose rightmost non-space cell crosses the unified right gutter.
    fn overflow_report(width: u16, height: u16) -> Vec<(HelpTopic, HelpLanguage, u16)> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut hits = Vec::new();
        let gutter_col = width.saturating_sub(RIGHT_GUTTER);
        for topic in all_topics() {
            for language in [HelpLanguage::Ko, HelpLanguage::En] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| {
                        render_hover_help(
                            frame,
                            HoverHelpView {
                                topic: *topic,
                                language,
                                column: width.saturating_sub(2),
                                row: height.saturating_sub(2),
                            },
                        );
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer().clone();
                let mut max_col: Option<u16> = None;
                for row in 0..height {
                    for col in 0..width {
                        let cell = &buffer[(col, row)];
                        if cell.symbol() != " " && !cell.symbol().is_empty() {
                            max_col = Some(max_col.map_or(col, |m| m.max(col)));
                        }
                    }
                }
                if let Some(mc) = max_col
                    && mc >= gutter_col
                {
                    hits.push((*topic, language, mc));
                }
            }
        }
        hits
    }

    #[test]
    fn every_topic_renders_inside_right_gutter_at_common_widths() {
        for &(width, height) in &[(80u16, 24u16), (100, 28), (120, 32), (160, 40), (200, 48)] {
            let hits = overflow_report(width, height);
            assert!(
                hits.is_empty(),
                "hover help overflows the right gutter at {width}x{height}; hits={hits:?}"
            );
        }
    }

    /// Detect the historical "double-width CJK glyph overruns the right
    /// border" regression. For every (topic, language, viewport) combo,
    /// locate the rendered hover box and assert every cell on its right
    /// border column carries a vertical-border / corner character — i.e.
    /// no help text leaked into the column where the box frame lives.
    #[test]
    fn right_border_column_is_never_overwritten_by_content() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let scenarios: &[(u16, u16, u16, u16)] = &[
            (80, 24, 2, 1),
            (80, 24, 2, 5),
            (80, 24, 76, 23),
            (100, 28, 4, 4),
            (120, 32, 8, 8),
            (160, 40, 12, 12),
            (200, 48, 16, 16),
            // small terminals where the bottom-drawer fallback fires
            (60, 18, 30, 9),
            (50, 14, 20, 7),
            // footer legend row, narrow + wide
            (90, 32, 4, 31),
            (160, 32, 4, 31),
        ];

        for &(width, height, col, row) in scenarios {
            for topic in all_topics() {
                for language in [HelpLanguage::Ko, HelpLanguage::En] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    terminal
                        .draw(|frame| {
                            render_hover_help(
                                frame,
                                HoverHelpView {
                                    topic: *topic,
                                    language,
                                    column: col,
                                    row,
                                },
                            );
                        })
                        .unwrap();
                    let buffer = terminal.backend().buffer().clone();

                    // Locate the rendered box by scanning for the first top-left corner.
                    let mut box_rect: Option<(u16, u16, u16, u16)> = None;
                    'outer: for r in 0..height {
                        for c in 0..width {
                            if buffer[(c, r)].symbol() == "┌" {
                                // walk right until we find ┐ on the same row
                                for c2 in (c + 1)..width {
                                    if buffer[(c2, r)].symbol() == "┐" {
                                        // walk down to find the bottom border
                                        for r2 in (r + 1)..height {
                                            if buffer[(c, r2)].symbol() == "└" {
                                                box_rect = Some((c, r, c2, r2));
                                                break 'outer;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let Some((bx, by, br, bb)) = box_rect else {
                        // some topics + viewports legitimately produce no floating
                        // box (e.g. DashboardFooter renders its legend rect with
                        // borders too — covered by the dedicated scenarios above).
                        continue;
                    };

                    for r in (by + 1)..bb {
                        let sym = buffer[(br, r)].symbol();
                        assert_eq!(
                            sym, "│",
                            "right border overwritten at ({br},{r}) for topic {topic:?} {language:?} at {width}x{height} col={col} row={row}; saw {sym:?}"
                        );
                        let left_sym = buffer[(bx, r)].symbol();
                        assert_eq!(
                            left_sym, "│",
                            "left border overwritten at ({bx},{r}) for topic {topic:?} {language:?} at {width}x{height} col={col} row={row}; saw {left_sym:?}"
                        );
                    }
                }
            }
        }
    }

    /// Helper: render at a chosen hover (column, row) and return the largest
    /// column that contains a visible glyph. Used to spot wrap-time bleed
    /// when the bottom-drawer fallback kicks in, when the hover sits at the
    /// far right edge, or when the floating box is placed near the bottom
    /// of the viewport.
    fn rightmost_glyph_column(
        topic: HelpTopic,
        language: HelpLanguage,
        width: u16,
        height: u16,
        column: u16,
        row: u16,
    ) -> Option<u16> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render_hover_help(
                    frame,
                    HoverHelpView {
                        topic,
                        language,
                        column,
                        row,
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut max_col: Option<u16> = None;
        for r in 0..height {
            for c in 0..width {
                let cell = &buffer[(c, r)];
                if cell.symbol() != " " && !cell.symbol().is_empty() {
                    max_col = Some(max_col.map_or(c, |m| m.max(c)));
                }
            }
        }
        max_col
    }

    #[test]
    fn every_topic_respects_gutter_with_diverse_hover_anchors() {
        let anchors: &[(u16, u16, u16, u16)] = &[
            // tight floating box at the very bottom right
            (80, 12, 75, 11),
            // bottom drawer path: height too small for floating
            (80, 10, 40, 9),
            // bottom drawer path: viewport narrower than 64
            (60, 24, 30, 12),
            // hover column past the right edge (defensive)
            (80, 24, 79, 6),
            // hover anchored on the footer row, forcing legend layout
            (160, 32, 2, 31),
            // legend at a very narrow width where 96-col min would
            // not fit; the legend should still leave a right gutter
            (90, 32, 3, 31),
        ];
        for &(width, height, col, row) in anchors {
            for topic in all_topics() {
                for language in [HelpLanguage::Ko, HelpLanguage::En] {
                    if let Some(max_col) =
                        rightmost_glyph_column(*topic, language, width, height, col, row)
                    {
                        assert!(
                            max_col <= width.saturating_sub(RIGHT_GUTTER).saturating_sub(1),
                            "topic {topic:?} {language:?} at {width}x{height} col={col} row={row} wrote glyph at col {max_col} (gutter={RIGHT_GUTTER})"
                        );
                    }
                }
            }
        }
    }
}
