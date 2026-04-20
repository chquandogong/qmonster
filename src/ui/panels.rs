use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::domain::identity::IdentityConfidence;
use crate::domain::signal::SignalSet;
use crate::ui::theme;

/// Per-pane panel: header line shows identity + confidence, then a
/// signals chip row, then a metrics row with SourceKind badges, then
/// recommendations.
pub fn render_pane_panel(area: Rect, buf: &mut Buffer, report: &PaneReport) {
    let title = pane_panel_title(report);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_IDLE))
        .title(title);

    if report.dead {
        Paragraph::new("DEAD — alerts drained")
            .style(Style::default().fg(theme::TEXT_DIM))
            .block(block)
            .render(area, buf);
        return;
    }

    let items = panel_body(report);
    Widget::render(List::new(items).block(block), area, buf);
}

pub fn pane_panel_title(report: &PaneReport) -> String {
    let id = &report.identity.identity;
    format!(
        "{} {:?}:{}:{:?} [{}]",
        report.pane_id,
        id.provider,
        id.instance,
        id.role,
        confidence_letter(report.identity.confidence)
    )
}

pub fn panel_body(report: &PaneReport) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    let chips = signal_chips(&report.signals);
    if chips.is_empty() {
        items.push(ListItem::new("signals: —").style(Style::default().fg(theme::TEXT_DIM)));
    } else {
        items.push(ListItem::new(format!("signals: {}", chips.join(" "))));
    }

    let metrics = metric_row(&report.signals);
    if !metrics.is_empty() {
        items.push(ListItem::new(format!("metrics: {metrics}")));
    }

    for rec in report.recommendations.iter().take(6) {
        let letter = rec.severity.letter();
        let badge = rec.source_kind.badge();
        items.push(ListItem::new(format!(
            "[{letter}] [{badge}] {}",
            rec.action
        )));
    }
    items
}

pub fn signal_chips(s: &SignalSet) -> Vec<&'static str> {
    let mut chips = Vec::new();
    if s.waiting_for_input {
        chips.push("WAIT");
    }
    if s.permission_prompt {
        chips.push("PERM");
    }
    if s.log_storm {
        chips.push("STORM");
    }
    if s.repeated_output {
        chips.push("REPEAT");
    }
    if s.verbose_answer {
        chips.push("VERB");
    }
    if s.error_hint {
        chips.push("ERR");
    }
    if s.subagent_hint {
        chips.push("SUBAG");
    }
    chips
}

pub fn metric_row(s: &SignalSet) -> String {
    let mut parts = Vec::new();
    if let Some(m) = s.context_pressure.as_ref() {
        parts.push(format!(
            "CTX={:.0}% [{}]",
            m.value * 100.0,
            m.source_kind.badge()
        ));
    }
    if let Some(m) = s.token_count.as_ref() {
        parts.push(format!("TOKENS={} [{}]", m.value, m.source_kind.badge()));
    }
    if let Some(m) = s.cost_usd.as_ref() {
        parts.push(format!("COST=${:.2} [{}]", m.value, m.source_kind.badge()));
    }
    parts.join("  ")
}

fn confidence_letter(c: IdentityConfidence) -> &'static str {
    match c {
        IdentityConfidence::High => "H",
        IdentityConfidence::Medium => "M",
        IdentityConfidence::Low => "L",
        IdentityConfidence::Unknown => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{PaneIdentity, Provider, ResolvedIdentity, Role};
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, SignalSet};

    fn base_report() -> PaneReport {
        PaneReport {
            pane_id: "%1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: vec![],
            effects: vec![],
            dead: false,
            current_path: "/repo".into(),
            cross_pane_findings: vec![],
        }
    }

    #[test]
    fn panel_title_includes_identity_and_confidence() {
        let rep = base_report();
        assert_eq!(pane_panel_title(&rep), "%1 Claude:1:Main [H]");
    }

    #[test]
    fn signal_chips_reflect_booleans() {
        let s = SignalSet {
            waiting_for_input: true,
            log_storm: true,
            ..SignalSet::default()
        };
        assert_eq!(signal_chips(&s), vec!["WAIT", "STORM"]);
    }

    #[test]
    fn metric_row_renders_badge_per_metric() {
        let s = SignalSet {
            context_pressure: Some(
                MetricValue::new(0.71, SourceKind::Estimated).with_confidence(0.5),
            ),
            ..SignalSet::default()
        };
        let row = metric_row(&s);
        assert!(row.contains("CTX=71%"));
        assert!(row.contains("[ES]"));
    }
}
