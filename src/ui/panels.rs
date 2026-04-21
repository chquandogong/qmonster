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
/// Gemini G-6 (v1.8.3): when the profile carries a non-empty
/// `side_effects` list, a `side_effects (<n>):` header line and one
/// `- <effect>` line per entry are appended so the operator sees
/// the aggregate trade-off cost BEFORE applying the profile.
/// `claude-default` keeps `side_effects: vec![]` so the section is
/// omitted; `claude-script-low-token` populates it 1:1 with its
/// lever list.
///
/// Returns an empty `Vec` when the rec has no profile payload.
pub fn format_profile_lines(rec: &Recommendation) -> Vec<String> {
    let Some(profile) = &rec.profile else {
        return Vec::new();
    };
    let mut lines =
        Vec::with_capacity(profile.levers.len() + profile.side_effects.len() + 2);
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
    // G-6: render the side_effects list immediately after the lever
    // rows so the operator scans cost before committing to the
    // profile. Omit the section entirely when empty (so the
    // healthy-state `claude-default` stays visually compact).
    if !profile.side_effects.is_empty() {
        lines.push(format!("side_effects ({}):", profile.side_effects.len()));
        for effect in &profile.side_effects {
            lines.push(format!("- {effect}"));
        }
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

    #[test]
    fn format_profile_lines_appends_side_effects_section_when_profile_has_side_effects() {
        // Gemini G-6 (v1.8.3): when a profile carries non-empty
        // side_effects, the renderer appends a `side_effects (<n>):`
        // header followed by `- <effect>` rows so the operator sees
        // the aggregate trade-off cost BEFORE applying. This test
        // fails if the renderer ever drops the section or mis-orders
        // the placement (must come AFTER all lever rows).
        use crate::domain::profile::{ProfileLever, ProviderProfile};
        use crate::domain::recommendation::{Recommendation, Severity};
        let profile = ProviderProfile {
            name: "claude-script-low-token",
            levers: vec![ProfileLever {
                key: "--bare",
                value: "enabled",
                citation: "Claude Code docs",
                source_kind: SourceKind::ProviderOfficial,
            }],
            side_effects: vec![
                "--bare suppresses verbose status output".into(),
                "CLAUDE_CODE_DISABLE_AUTO_MEMORY=1 disables provider auto-memory".into(),
            ],
            source_kind: SourceKind::ProjectCanonical,
        };
        let rec = Recommendation {
            action: "provider-profile: claude-script-low-token",
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

        // Shape: 1 header + 1 lever + 1 side_effects header + 2 entries = 5.
        assert_eq!(lines.len(), 5, "1 header + 1 lever + 1 side-effects header + 2 entries");

        // side_effects section comes AFTER all lever rows. Find the
        // section header and verify it's past the lever row.
        let side_effects_header_idx = lines
            .iter()
            .position(|l| l.starts_with("side_effects"))
            .expect("side_effects header line must be present");
        assert!(
            side_effects_header_idx > 1,
            "side_effects section must come AFTER lever rows; got index {side_effects_header_idx}"
        );

        // Header declares the count.
        assert!(lines[side_effects_header_idx].contains("(2)"),
            "header reports count: {}", lines[side_effects_header_idx]);

        // Every entry after the header starts with `- ` and carries
        // the operator-visible trade-off text.
        let entries = &lines[side_effects_header_idx + 1..];
        assert_eq!(entries.len(), 2);
        for entry in entries {
            assert!(entry.starts_with("- "), "each side-effect entry starts with '- ': {entry}");
        }
        assert!(entries.iter().any(|e| e.contains("verbose status output")),
            "--bare side effect visible in rendered lines");
        assert!(entries.iter().any(|e| e.contains("auto-memory")),
            "DISABLE_AUTO_MEMORY side effect visible in rendered lines");
    }

    #[test]
    fn format_profile_lines_omits_side_effects_section_when_profile_side_effects_is_empty() {
        // claude-default has `side_effects: vec![]` by design
        // (healthy-state baseline has no operator-visible trade-offs).
        // The renderer must keep that profile visually compact — NO
        // `side_effects:` header line should appear.
        use crate::domain::profile::{ProfileLever, ProviderProfile};
        use crate::domain::recommendation::{Recommendation, Severity};
        let profile = ProviderProfile {
            name: "claude-default",
            levers: vec![ProfileLever {
                key: "BASH_MAX_OUTPUT_LENGTH",
                value: "30000",
                citation: "Claude Code docs",
                source_kind: SourceKind::ProviderOfficial,
            }],
            side_effects: vec![],
            source_kind: SourceKind::ProjectCanonical,
        };
        let rec = Recommendation {
            action: "x",
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
        assert!(
            !lines.iter().any(|l| l.starts_with("side_effects")),
            "empty side_effects → no section rendered. lines: {:?}",
            lines
        );
    }
}
