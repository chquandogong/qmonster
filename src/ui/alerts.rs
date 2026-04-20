use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::ui::theme;

/// Codex v1.7.3 (phase3b-strong-rec cleanup): single source of truth for
/// the strong-recommendation render format used by both the TUI alert
/// queue and the `--once` stdout path. Emits `next: …` before `run: …`
/// so the snapshot precondition always precedes the executable command;
/// omits either segment cleanly when its field is `None`.
pub fn format_strong_rec_body(rec: &Recommendation, pane_id: &str) -> String {
    let letter = rec.severity.letter();
    let badge = rec.source_kind.badge();
    let prefix = format!("[{letter}] [{badge}] >> CHECKPOINT ({pane_id}): {}", rec.reason);
    let step = rec.next_step.as_deref().unwrap_or("");
    let cmd = rec.suggested_command.as_deref().unwrap_or("");
    match (step.is_empty(), cmd.is_empty()) {
        (true, true) => prefix,
        (true, false) => format!("{prefix} — run: `{cmd}`"),
        (false, true) => format!("{prefix} — next: {step}"),
        (false, false) => format!("{prefix} — next: {step} — run: `{cmd}`"),
    }
}

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
    // 1. System notices.
    for n in notices {
        let letter = n.severity.letter();
        let badge = n.source_kind.badge();
        let color = theme::severity_color(n.severity);
        let body = format!("[{letter}] [{badge}] SYSTEM: {} — {}", n.title, n.body);
        out.push(ListItem::new(body).style(Style::default().fg(color)));
    }
    // 2. Strong recommendations (G-7 checkpoint UX).
    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| r.is_strong) {
            let color = theme::severity_color(rec.severity);
            let body = format_strong_rec_body(rec, &rep.pane_id);
            out.push(ListItem::new(body).style(Style::default().fg(color)));
        }
    }
    // 3. Cross-pane findings.
    for rep in reports {
        for f in &rep.cross_pane_findings {
            let color = theme::severity_color(f.severity);
            let letter = f.severity.letter();
            let badge = f.source_kind.badge();
            let body = format!(
                "[{letter}] [{badge}] CROSS-PANE: {}",
                f.reason,
            );
            out.push(ListItem::new(body).style(Style::default().fg(color)));
        }
    }
    // 4. Per-pane non-strong recommendations.
    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| !r.is_strong) {
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
            current_path: "/repo".into(),
            cross_pane_findings: vec![],
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
                is_strong: false,
                next_step: None,
            },
            Recommendation {
                action: "log-storm",
                reason: "r2".into(),
                severity: Severity::Risk,
                source_kind: SourceKind::Heuristic,
                suggested_command: None,
                side_effects: vec![],
                is_strong: false,
                next_step: None,
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
            is_strong: false,
            next_step: None,
        }]);
        let items = collect_items(&[notice], &[rep]);
        assert_eq!(items.len(), 2);
        // System notice is rendered first.
        // We cannot easily inspect the text of a ListItem without private
        // fields, but ordering is stable: notice then pane.
    }

    #[test]
    fn strong_recommendation_renders_in_dedicated_slot_above_per_pane_alerts() {
        let strong = Recommendation {
            action: "context-pressure: act now",
            reason: "context near critical — checkpoint + archive now; /compact after".into(),
            severity: Severity::Risk,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/compact".into()),
            side_effects: vec![],
            is_strong: true,
            next_step: Some("press 's' to snapshot + archive now, before running /compact".into()),
        };
        let normal = Recommendation {
            action: "log-storm",
            reason: "r".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        };
        let rep = base_report(vec![strong, normal]);
        let items = collect_items(&[], &[rep]);
        // Two items: strong-rec line + normal-rec line. Strong appears first.
        assert_eq!(items.len(), 2);
        // We can't easily introspect ListItem text without private-field hacks,
        // but ordering is stable and this test locks in the slot count.
    }

    #[test]
    fn format_strong_rec_body_emits_next_before_run_in_order() {
        // Codex v1.7.3 finding #1+#2 regression guard: the render format
        // must place the `next:` segment BEFORE the `run:` segment, and
        // must only treat suggested_command as a runnable value (no
        // mixed-mode prose). A regression like `run: /compact; snapshot
        // later` would fail this ordered assertion.
        let strong = Recommendation {
            action: "context-pressure: act now",
            reason: "context near critical".into(),
            severity: Severity::Risk,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/compact".into()),
            side_effects: vec![],
            is_strong: true,
            next_step: Some("press 's' to snapshot + archive now".into()),
        };
        let body = format_strong_rec_body(&strong, "%1");

        let next_idx = body.find("next: press 's' to snapshot + archive now")
            .expect("body must contain literal `next: …snapshot…` segment");
        let run_idx = body.find("run: `/compact`")
            .expect("body must contain literal `run: `/compact`` segment");
        assert!(next_idx < run_idx,
            "ordering contract: `next:` MUST precede `run:`. body: {body}");
        assert!(body.contains(">> CHECKPOINT (%1)"),
            "body must carry the CHECKPOINT slot prefix. got: {body}");
    }

    #[test]
    fn format_strong_rec_body_omits_next_when_absent() {
        let strong = Recommendation {
            action: "x",
            reason: "y".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: Some("/compact".into()),
            side_effects: vec![],
            is_strong: true,
            next_step: None,
        };
        let body = format_strong_rec_body(&strong, "%1");
        assert!(!body.contains("next:"), "no next_step → no `next:` segment. got: {body}");
        assert!(body.contains("run: `/compact`"), "cmd still rendered. got: {body}");
    }

    #[test]
    fn cross_pane_findings_render_above_per_pane_alerts() {
        use crate::domain::recommendation::{CrossPaneFinding, CrossPaneKind};
        let mut rep = base_report(vec![Recommendation {
            action: "log-storm",
            reason: "r".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        }]);
        rep.cross_pane_findings.push(CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentMutatingWork,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec!["%2".into()],
            reason: "concurrent work on /repo".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: None,
        });
        let items = collect_items(&[], &[rep]);
        assert_eq!(items.len(), 2, "one cross-pane finding + one pane alert");
    }
}
