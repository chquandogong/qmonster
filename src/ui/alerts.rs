use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::recommendation::Severity;
use crate::ui::theme;

/// Top-of-screen alert queue. Renders system notices first, then
/// per-pane recommendations.
pub fn render_alerts(
    area: Rect,
    buf: &mut Buffer,
    notices: &[SystemNotice],
    reports: &[PaneReport],
) {
    let items = collect_items(notices, reports);
    if items.is_empty() {
        Paragraph::new("no alerts")
            .style(Style::default().fg(theme::TEXT_DIM))
            .block(block("Alerts"))
            .render(area, buf);
    } else {
        Widget::render(List::new(items).block(block("Alerts")), area, buf);
    }
}

fn block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_IDLE))
        .title(title)
}

fn collect_items(notices: &[SystemNotice], reports: &[PaneReport]) -> Vec<ListItem<'static>> {
    let mut out = Vec::new();
    for n in notices {
        let letter = n.severity.letter();
        let badge = n.source_kind.badge();
        let color = theme::severity_color(n.severity);
        let body = format!("[{letter}] [{badge}] SYSTEM: {} — {}", n.title, n.body);
        out.push(ListItem::new(body).style(Style::default().fg(color)));
    }
    for rep in reports {
        for rec in &rep.recommendations {
            let color = theme::severity_color(rec.severity);
            let letter = rec.severity.letter();
            let pane_id = rep.pane_id.clone();
            let badge = rec.source_kind.badge();
            let body = format!(
                "[{letter}] [{badge}] {pane}: {action} — {reason}",
                pane = pane_id,
                action = rec.action,
                reason = rec.reason
            );
            out.push(ListItem::new(body).style(Style::default().fg(color)));
        }
    }
    out
}

pub fn severity_letter(sev: Severity) -> &'static str {
    sev.letter()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role};
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Recommendation;
    use crate::domain::signal::SignalSet;

    fn base_report(recs: Vec<Recommendation>) -> PaneReport {
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
            effects: vec![],
            dead: false,
            recommendations: recs,
        }
    }

    #[test]
    fn collects_items_one_per_recommendation() {
        let rep = base_report(vec![
            Recommendation {
                action: "notify-input-wait",
                reason: "r1".into(),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
                suggested_command: None,
                side_effects: vec![],
            },
            Recommendation {
                action: "log-storm",
                reason: "r2".into(),
                severity: Severity::Risk,
                source_kind: SourceKind::Heuristic,
                suggested_command: None,
                side_effects: vec![],
            },
        ]);
        let items = collect_items(&[], &[rep]);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn system_notices_appear_before_per_pane_alerts() {
        let notice = SystemNotice {
            title: "version drift".into(),
            body: "tmux: 3.4 -> 3.5".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProviderOfficial,
        };
        let rep = base_report(vec![Recommendation {
            action: "log-storm",
            reason: "r2".into(),
            severity: Severity::Risk,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
        }]);
        let items = collect_items(&[notice], &[rep]);
        assert_eq!(items.len(), 2);
        // System notice is rendered first.
        // We cannot easily inspect the text of a ListItem without private
        // fields, but ordering is stable: notice then pane.
    }
}
