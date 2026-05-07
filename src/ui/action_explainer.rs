//! Phase D (v1.38) — Action Explainer modal renderer.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::action_explainer::ActionExplainView;
use crate::ui::dashboard::close_button_rect;
use crate::ui::labels::source_kind_label;
use crate::ui::theme;

const LABEL_W: usize = 14;

pub fn render_explainer_lines(view: &ActionExplainView, _body: Rect) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(Line::from(""));
    out.push(field("Target pane", &view.target_label));
    out.push(field(&view.payload_label, &view.payload_text));
    out.push(field(
        "Why now",
        &format!("{} [{}]", view.why, source_kind_label(view.why_source)),
    ));
    if let Some(sev) = view.severity {
        out.push(field("Severity", &severity_label(sev)));
    }
    out.push(field("Audit chain", &view.audit_chain));
    if let Some(w) = &view.mode_warning {
        out.push(field("Mode now", w));
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "Press Enter to confirm \u{B7} Esc / same key / click [x] to cancel".to_string(),
        Style::default().fg(theme::TEXT_DIM),
    )));
    out
}

fn field(label: &str, value: &str) -> Line<'static> {
    Line::from(format!("{label:<LABEL_W$}: {value}"))
}

fn severity_label(sev: crate::domain::recommendation::Severity) -> String {
    use crate::domain::recommendation::Severity;
    match sev {
        Severity::Safe => "SAFE",
        Severity::Good => "GOOD",
        Severity::Concern => "CONCERN",
        Severity::Warning => "WARNING",
        Severity::Risk => "RISK",
    }
    .to_string()
}

pub fn render_action_explainer_modal(frame: &mut Frame<'_>, view: &ActionExplainView) {
    let viewport = frame.area();
    let area = explainer_modal_area(viewport);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!("Action: {}", view.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));
    let lines = render_explainer_lines(view, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
        close_button_rect(area),
    );
}

/// Centered ~70% × 60% modal area, clamped to a 60×14 minimum so the
/// explainer renders predictably on small terminals. Promoted to
/// `pub(crate)` (v1.38 Bug A fix) so the tui_loop mouse guard can
/// derive the `[x]` close-button rect from the viewport without
/// re-implementing the geometry.
pub(crate) fn explainer_modal_area(viewport: Rect) -> Rect {
    let width = (viewport.width * 70 / 100).max(60).min(viewport.width);
    let height = (viewport.height * 60 / 100).max(14).min(viewport.height);
    let x = viewport.x + viewport.width.saturating_sub(width) / 2;
    let y = viewport.y + viewport.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_lines_contains_title_target_payload_why_audit() {
        use crate::app::action_explainer::ActionExplainView;
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        use ratatui::layout::Rect;

        let view = ActionExplainView {
            title: "Accept prompt-send proposal".into(),
            target_label: "codex:1:review \u{B7} %57".into(),
            payload_label: "What to send".into(),
            payload_text: "/compact".into(),
            why: "cache: drift detected — /compact will let cache rebuild".into(),
            why_source: SourceKind::ProjectCanonical,
            severity: Some(Severity::Concern),
            audit_chain: "Execute \u{2192} PromptSendAccepted \u{2192} PromptSendCompleted".into(),
            mode_warning: Some("observe_only \u{26A0} prompt sends are blocked".into()),
        };
        let body = Rect::new(0, 0, 80, 16);
        let lines = render_explainer_lines(&view, body);
        let dump: String = lines.iter().map(|l| line_to_string(l) + "\n").collect();
        assert!(
            dump.contains("Target pane"),
            "missing target pane row;\n{dump}"
        );
        assert!(dump.contains("codex:1:review"), "missing target value");
        assert!(dump.contains("What to send"), "missing payload label");
        assert!(dump.contains("/compact"), "missing payload text");
        assert!(dump.contains("Why now"), "missing why row");
        assert!(dump.contains("cache: drift"), "missing why text");
        assert!(dump.contains("Audit chain"), "missing audit row");
        assert!(
            dump.contains("Press Enter to confirm"),
            "missing confirm hint"
        );
    }

    fn line_to_string(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }
}
