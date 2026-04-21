use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::domain::identity::IdentityConfidence;
use crate::domain::recommendation::Recommendation;
use crate::domain::signal::SignalSet;
use crate::ui::theme;

/// Codex v1.8.1 (Phase 4 P4-1 remediation): shared renderer for the
/// structured `ProviderProfile` payload carried by provider-profile
/// recommendations. Emits one line per lever, each carrying the
/// per-lever `SourceKind` badge + key = value + citation, so the
/// `ProjectCanonical` bundle (profile name) vs `ProviderOfficial`
/// levers (individual rows) authority split is visible end-to-end
/// on both the TUI pane panel and `--once` output. Caller prepends
/// its own leading indent.
///
/// Returns an empty `Vec` when the rec has no profile payload.
pub fn format_profile_lines(rec: &Recommendation) -> Vec<String> {
    let Some(profile) = &rec.profile else {
        return Vec::new();
    };
    let mut lines = Vec::with_capacity(profile.levers.len() + 1);
    lines.push(format!(
        "profile: {} ({} levers) [{}]",
        profile.name,
        profile.levers.len(),
        profile.source_kind.badge(),
    ));
    for lever in &profile.levers {
        lines.push(format!(
            "[{}] {} = {} — {}",
            lever.source_kind.badge(),
            lever.key,
            lever.value,
            lever.citation,
        ));
    }
    lines
}

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
        // v1.8.1: expose the structured ProviderProfile payload so the
        // operator can audit lever key/value/citation/SourceKind
        // directly in the panel (Codex P4-1 finding #1 closed).
        for line in format_profile_lines(rec) {
            items.push(ListItem::new(format!("  {line}")));
        }
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

    #[test]
    fn format_profile_lines_is_empty_when_rec_carries_no_profile() {
        use crate::domain::recommendation::{Recommendation, Severity};
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "r".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        assert!(format_profile_lines(&rec).is_empty(),
            "no profile -> no lever lines emitted; caller prints only the rec header");
    }

    #[test]
    fn format_profile_lines_exposes_lever_key_value_citation_and_source_kind_badge() {
        // Codex v1.8.1 (P4-1 finding #1 closed): the renderer must
        // surface every lever's key + value + citation + per-lever
        // SourceKind so the operator can audit authority directly on
        // the panel / --once without having to cross-reference the
        // reason prose. This test locks the exact line shape so a
        // regression that drops any of those four pieces fails here.
        use crate::domain::profile::{ProfileLever, ProviderProfile};
        use crate::domain::recommendation::{Recommendation, Severity};
        let profile = ProviderProfile {
            name: "claude-default",
            levers: vec![
                ProfileLever {
                    key: "BASH_MAX_OUTPUT_LENGTH",
                    value: "30000",
                    citation: "Claude Code docs — env vars, bash output cap",
                    source_kind: SourceKind::ProviderOfficial,
                },
                ProfileLever {
                    key: "includeGitInstructions",
                    value: "false",
                    citation: "Claude Code docs — settings.json",
                    source_kind: SourceKind::ProviderOfficial,
                },
            ],
            side_effects: vec![],
            source_kind: SourceKind::ProjectCanonical,
        };
        let rec = Recommendation {
            action: "provider-profile: claude-default",
            reason: "r".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: Some(profile),
        };
        let lines = format_profile_lines(&rec);
        assert_eq!(lines.len(), 3, "1 header + 2 lever lines");

        // Header: profile name + lever count + ProjectCanonical badge.
        assert!(lines[0].contains("claude-default"), "header names the profile: {}", lines[0]);
        assert!(lines[0].contains("2 levers"), "header reports lever count: {}", lines[0]);
        assert!(lines[0].contains("[PC]"),
            "header carries ProjectCanonical badge so operator sees bundle authority: {}",
            lines[0]);

        // Lever line #1: ProviderOfficial badge + key + value + citation.
        assert!(lines[1].contains("[PO]"), "lever carries ProviderOfficial badge: {}", lines[1]);
        assert!(lines[1].contains("BASH_MAX_OUTPUT_LENGTH"), "lever key visible: {}", lines[1]);
        assert!(lines[1].contains("30000"), "lever value visible: {}", lines[1]);
        assert!(lines[1].contains("bash output cap"), "lever citation visible: {}", lines[1]);

        // Lever line #2.
        assert!(lines[2].contains("[PO]"));
        assert!(lines[2].contains("includeGitInstructions"));
        assert!(lines[2].contains("false"));
        assert!(lines[2].contains("settings.json"));
    }
}
