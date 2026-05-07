use crate::insights_report::format_insights_report_lines;
use crate::store::InsightsSnapshot;
use crate::ui::dashboard::{centered_rect, close_button_rect};
use crate::ui::theme;
use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

#[derive(Debug, Clone, Default)]
pub struct InsightsOverlay {
    open: bool,
    scroll: u16,
    snapshot: Option<InsightsSnapshot>,
    error: Option<String>,
    refreshed_label: Option<String>,
}

impl InsightsOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.scroll = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
    }

    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, max: u16) {
        self.scroll = self.scroll.saturating_add(1).min(max);
    }

    pub fn set_snapshot(&mut self, snapshot: InsightsSnapshot, refreshed_label: String) {
        self.snapshot = Some(snapshot);
        self.error = None;
        self.refreshed_label = Some(refreshed_label);
        self.scroll = 0;
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.snapshot = None;
        self.refreshed_label = None;
        self.scroll = 0;
    }

    pub fn line_count(&self) -> usize {
        self.lines().len()
    }

    fn lines(&self) -> Vec<String> {
        if let Some(error) = self.error.as_ref() {
            return vec![
                "Token Insights".into(),
                String::new(),
                format!("error: {error}"),
            ];
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return vec![
                "Token Insights".into(),
                String::new(),
                "No insights snapshot loaded.".into(),
                "Press r to refresh.".into(),
            ];
        };
        let mut lines = format_insights_report_lines(snapshot);
        if let Some(label) = self.refreshed_label.as_ref() {
            lines.insert(1, format!("refreshed: {label}"));
        }
        lines
    }
}

pub fn insights_modal_area(viewport: ratatui::layout::Rect) -> ratatui::layout::Rect {
    centered_rect(86, 78, viewport)
}

pub fn render_insights_modal(frame: &mut Frame<'_>, overlay: &InsightsOverlay) {
    let area = insights_modal_area(frame.area());
    frame.render_widget(Clear, area);
    let title = " Token Insights [i/Esc/q close] [r refresh] ";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(area),
    );

    let lines = overlay.lines();
    if lines.len() <= 4 && overlay.snapshot.is_none() {
        let paragraph = Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
        return;
    }

    let items: Vec<ListItem> = lines
        .into_iter()
        .skip(overlay.scroll as usize)
        .take(inner.height as usize)
        .map(ListItem::new)
        .collect();
    frame.render_widget(List::new(items), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights_report::empty_insights_snapshot;
    use crate::store::InsightsWindow;

    #[test]
    fn overlay_open_resets_scroll() {
        let mut overlay = InsightsOverlay::new();
        overlay.open();
        overlay.scroll_down(10);
        assert_eq!(overlay.scroll(), 1);
        overlay.close();
        overlay.open();
        assert_eq!(overlay.scroll(), 0);
    }

    #[test]
    fn snapshot_lines_include_action_ledger() {
        let mut overlay = InsightsOverlay::new();
        overlay.set_snapshot(
            empty_insights_snapshot(InsightsWindow {
                since_ms: 0,
                until_ms: 1,
            }),
            "12:00:00".into(),
        );

        let joined = overlay.lines().join("\n");
        assert!(joined.contains("Action Ledger"));
        assert!(joined.contains("refreshed: 12:00:00"));
    }
}
