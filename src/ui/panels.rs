use ratatui::prelude::*;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::domain::identity::{IdentityConfidence, Provider, Role};
use crate::domain::recommendation::Recommendation;
use crate::domain::signal::SignalSet;
use crate::ui::labels::source_kind_label;
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
    let mut lines = Vec::with_capacity(profile.levers.len() + profile.side_effects.len() + 2);
    lines.push(format!(
        "profile: {} ({} levers) [{}]",
        profile.name,
        profile.levers.len(),
        source_kind_label(profile.source_kind),
    ));
    for lever in &profile.levers {
        lines.push(format!(
            "[{}] {} = {} — {}",
            source_kind_label(lever.source_kind),
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

pub fn render_pane_list(
    area: Rect,
    buf: &mut Buffer,
    reports: &[PaneReport],
    state: &mut ListState,
    target_label: &str,
    focused: bool,
) {
    let title = format!("Panes · target {target_label} · selected pane expands below");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            theme::BORDER_ACTIVE
        } else {
            theme::BORDER_IDLE
        }))
        .title(title);

    if reports.is_empty() {
        Paragraph::new("no panes in the selected window")
            .style(Style::default().fg(theme::TEXT_DIM))
            .block(block)
            .render(area, buf);
        return;
    }

    let selected = state
        .selected()
        .unwrap_or(0)
        .min(reports.len().saturating_sub(1));
    state.select(Some(selected));

    let items: Vec<ListItem<'static>> = reports
        .iter()
        .enumerate()
        .map(|(idx, report)| pane_list_item(report, idx == selected, idx + 1 < reports.len()))
        .collect();

    StatefulWidget::render(
        List::new(items)
            .block(block)
            .highlight_style(highlight_style(focused))
            .highlight_symbol("▶ "),
        area,
        buf,
        state,
    );
}

fn highlight_style(focused: bool) -> Style {
    let style = Style::default().fg(theme::TEXT_PRIMARY);
    if focused {
        style.bg(theme::BADGE_BG).add_modifier(Modifier::BOLD)
    } else {
        style.add_modifier(Modifier::BOLD)
    }
}

fn pane_list_item(report: &PaneReport, expanded: bool, with_separator: bool) -> ListItem<'static> {
    let mut lines = vec![
        Line::styled(
            pane_panel_title(report),
            Style::default()
                .fg(pane_header_color(report))
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(aligned_field("path", &display_path(&report.current_path))),
        Line::from(aligned_field("status", &state_summary_line(report))),
    ];

    if let Some(line) = blocking_signal_line(&report.signals) {
        lines.push(line);
    }
    if let Some(line) = signal_badge_line("signals", secondary_signal_chips(&report.signals)) {
        lines.push(line);
    }

    if let Some(line) = metric_badge_line(&report.signals) {
        lines.push(line);
    }

    if expanded {
        for rec in report.recommendations.iter().take(3) {
            lines.push(Line::from(aligned_field(
                severity_label(rec.severity),
                &rec.reason,
            )));
            for detail in crate::ui::alerts::recommendation_detail_lines(rec) {
                lines.push(Line::from(format!("  {detail}")));
            }
            for line in format_profile_lines(rec) {
                lines.push(Line::from(format!("  {line}")));
            }
        }
        if report.recommendations.is_empty() {
            lines.push(Line::from(aligned_field(
                "status",
                "no active recommendations",
            )));
        }
    }

    if with_separator {
        lines.push(Line::styled(
            "────────────────────────────────────────",
            Style::default().fg(theme::TEXT_DIM),
        ));
    }

    ListItem::new(lines)
}

fn pane_header_color(report: &PaneReport) -> Color {
    report
        .recommendations
        .iter()
        .map(|rec| rec.severity)
        .max()
        .map(theme::severity_color)
        .unwrap_or_else(|| provider_color(report.identity.identity.provider))
}

fn provider_color(provider: Provider) -> Color {
    match provider {
        Provider::Claude => Color::Rgb(200, 175, 120),
        Provider::Codex => Color::Rgb(120, 175, 205),
        Provider::Gemini => Color::Rgb(140, 185, 145),
        Provider::Qmonster => Color::Rgb(175, 160, 210),
        Provider::Unknown => theme::TEXT_PRIMARY,
    }
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
        "{}:{} · {} {} · {}",
        report.session_name,
        report.window_index,
        provider_label(id.provider),
        role_label(id.role),
        report.pane_id,
    )
}

pub fn panel_body(report: &PaneReport) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    items.push(ListItem::new(aligned_field(
        "status",
        &state_summary_line(report),
    )));
    if let Some(line) = blocking_signal_line(&report.signals) {
        items.push(ListItem::new(line));
    }
    if let Some(line) = signal_badge_line("signals", secondary_signal_chips(&report.signals)) {
        items.push(ListItem::new(line));
    }

    if let Some(line) = metric_badge_line(&report.signals) {
        items.push(ListItem::new(line));
    }

    for rec in report.recommendations.iter().take(6) {
        items.push(ListItem::new(aligned_field(
            severity_label(rec.severity),
            &rec.reason,
        )));
        for detail in crate::ui::alerts::recommendation_detail_lines(rec) {
            items.push(ListItem::new(format!("  {detail}")));
        }
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
    let mut chips = blocking_signal_chips(s);
    chips.extend(secondary_signal_chips(s));
    chips
}

fn blocking_signal_chips(s: &SignalSet) -> Vec<&'static str> {
    let mut chips = Vec::new();
    if s.waiting_for_input {
        chips.push("waiting for input");
    }
    if s.permission_prompt {
        chips.push("approval needed");
    }
    chips
}

fn secondary_signal_chips(s: &SignalSet) -> Vec<&'static str> {
    let mut chips = Vec::new();
    if s.log_storm {
        chips.push("log storm");
    }
    if s.repeated_output {
        chips.push("repeated output");
    }
    if s.verbose_answer {
        chips.push("verbose output");
    }
    if s.error_hint {
        chips.push("error hint");
    }
    if s.subagent_hint {
        chips.push("subagent activity");
    }
    chips
}

pub fn metric_row(s: &SignalSet) -> String {
    let mut parts = Vec::new();
    if let Some(m) = s.context_pressure.as_ref() {
        parts.push(format!(
            "context {:.0}% [{}]",
            m.value * 100.0,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.token_count.as_ref() {
        parts.push(format!(
            "tokens {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.cost_usd.as_ref() {
        parts.push(format!(
            "cost ${:.2} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    parts.join("  ")
}

fn aligned_field(label: &str, value: &str) -> String {
    format!("{label:<8}: {value}")
}

fn state_summary_line(report: &PaneReport) -> String {
    format!(
        "{} confidence",
        confidence_label(report.identity.confidence)
    )
}

fn display_path(path: &str) -> String {
    if path.is_empty() {
        "unknown path".into()
    } else {
        path.into()
    }
}

fn confidence_label(c: IdentityConfidence) -> &'static str {
    match c {
        IdentityConfidence::High => "high",
        IdentityConfidence::Medium => "medium",
        IdentityConfidence::Low => "low",
        IdentityConfidence::Unknown => "unknown",
    }
}

fn provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Qmonster => "Qmonster",
        Provider::Unknown => "Unknown",
    }
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::Main => "main",
        Role::Review => "review",
        Role::Research => "research",
        Role::Monitor => "monitor",
        Role::Unknown => "unknown",
    }
}

fn severity_label(severity: crate::domain::recommendation::Severity) -> &'static str {
    match severity {
        crate::domain::recommendation::Severity::Safe => "SAFE",
        crate::domain::recommendation::Severity::Good => "GOOD",
        crate::domain::recommendation::Severity::Concern => "CONCERN",
        crate::domain::recommendation::Severity::Warning => "WARNING",
        crate::domain::recommendation::Severity::Risk => "RISK",
    }
}

fn metric_badge_line(signals: &SignalSet) -> Option<Line<'static>> {
    let mut spans = vec![Span::raw(format!("{:<8}: ", "metrics"))];
    let mut has_any = false;

    if let Some(metric) = signals.context_pressure.as_ref() {
        has_any = true;
        spans.push(Span::styled(
            format!(" CTX {:.0}% ", metric.value * 100.0),
            theme::severity_badge_style(context_metric_severity(metric.value)),
        ));
    }
    if let Some(metric) = signals.token_count.as_ref() {
        if has_any {
            spans.push(Span::raw(" "));
        }
        has_any = true;
        spans.push(Span::styled(
            format!(
                " TOKENS {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }
    if let Some(metric) = signals.cost_usd.as_ref() {
        if has_any {
            spans.push(Span::raw(" "));
        }
        has_any = true;
        spans.push(Span::styled(
            format!(
                " COST ${:.2} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        ));
    }

    has_any.then(|| Line::from(spans))
}

fn context_metric_severity(value: f32) -> crate::domain::recommendation::Severity {
    use crate::domain::recommendation::Severity;
    if value >= 0.85 {
        Severity::Risk
    } else if value >= 0.75 {
        Severity::Warning
    } else if value >= 0.60 {
        Severity::Concern
    } else {
        Severity::Good
    }
}

fn blocking_signal_line(signals: &SignalSet) -> Option<Line<'static>> {
    let chips = blocking_signal_chips(signals);
    if chips.is_empty() {
        return None;
    }
    let mut spans = vec![Span::raw(format!("{:<8}: ", "blocked"))];
    spans.push(Span::styled(
        " BLOCKED ",
        theme::severity_badge_style(crate::domain::recommendation::Severity::Warning)
            .add_modifier(Modifier::BOLD),
    ));
    for chip in chips {
        spans.push(Span::raw(" "));
        spans.push(signal_chip_span(
            chip,
            crate::domain::recommendation::Severity::Risk,
        ));
    }
    Some(Line::from(spans))
}

fn signal_badge_line(label: &str, chips: Vec<&'static str>) -> Option<Line<'static>> {
    if chips.is_empty() {
        return None;
    }
    let mut spans = vec![Span::raw(format!("{label:<8}: "))];
    for (idx, chip) in chips.into_iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(signal_chip_span(
            chip,
            crate::domain::recommendation::Severity::Concern,
        ));
    }
    Some(Line::from(spans))
}

fn signal_chip_span(
    chip: &'static str,
    severity: crate::domain::recommendation::Severity,
) -> Span<'static> {
    Span::styled(
        format!(" {} ", chip.to_ascii_uppercase()),
        theme::severity_badge_style(severity),
    )
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
            session_name: "qwork".into(),
            window_index: "1".into(),
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
        assert_eq!(pane_panel_title(&rep), "qwork:1 · Claude main · %1");
    }

    #[test]
    fn signal_chips_reflect_booleans() {
        let s = SignalSet {
            waiting_for_input: true,
            log_storm: true,
            ..SignalSet::default()
        };
        assert_eq!(signal_chips(&s), vec!["waiting for input", "log storm"]);
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
        assert!(row.contains("context 71%"));
        assert!(row.contains("[Estimate]"));
    }

    #[test]
    fn state_summary_line_uses_full_words_instead_of_chips() {
        let mut rep = base_report();
        rep.signals = SignalSet {
            waiting_for_input: true,
            repeated_output: true,
            ..SignalSet::default()
        };
        let summary = state_summary_line(&rep);
        assert!(summary.contains("high confidence"));
        assert!(!summary.contains("waiting for input"));
        assert!(!summary.contains("repeated output"));
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
        assert!(
            format_profile_lines(&rec).is_empty(),
            "no profile -> no lever lines emitted; caller prints only the rec header"
        );
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
        assert!(
            lines[0].contains("claude-default"),
            "header names the profile: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("2 levers"),
            "header reports lever count: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("[Qmonster]"),
            "header carries ProjectCanonical label so operator sees bundle authority: {}",
            lines[0]
        );

        // Lever line #1: ProviderOfficial badge + key + value + citation.
        assert!(
            lines[1].contains("[Official]"),
            "lever carries ProviderOfficial label: {}",
            lines[1]
        );
        assert!(
            lines[1].contains("BASH_MAX_OUTPUT_LENGTH"),
            "lever key visible: {}",
            lines[1]
        );
        assert!(
            lines[1].contains("30000"),
            "lever value visible: {}",
            lines[1]
        );
        assert!(
            lines[1].contains("bash output cap"),
            "lever citation visible: {}",
            lines[1]
        );

        // Lever line #2.
        assert!(lines[2].contains("[Official]"));
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
        assert_eq!(
            lines.len(),
            5,
            "1 header + 1 lever + 1 side-effects header + 2 entries"
        );

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
        assert!(
            lines[side_effects_header_idx].contains("(2)"),
            "header reports count: {}",
            lines[side_effects_header_idx]
        );

        // Every entry after the header starts with `- ` and carries
        // the operator-visible trade-off text.
        let entries = &lines[side_effects_header_idx + 1..];
        assert_eq!(entries.len(), 2);
        for entry in entries {
            assert!(
                entry.starts_with("- "),
                "each side-effect entry starts with '- ': {entry}"
            );
        }
        assert!(
            entries.iter().any(|e| e.contains("verbose status output")),
            "--bare side effect visible in rendered lines"
        );
        assert!(
            entries.iter().any(|e| e.contains("auto-memory")),
            "DISABLE_AUTO_MEMORY side effect visible in rendered lines"
        );
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
