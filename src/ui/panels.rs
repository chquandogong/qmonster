use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::prelude::*;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::domain::identity::{IdentityConfidence, Provider, Role};
use crate::domain::recommendation::Recommendation;
use crate::domain::signal::{IdleCause, RuntimeFact, RuntimeFactKind, SignalSet};
use crate::ui::labels::{ellipsize, format_count_with_suffix, source_kind_label};
use crate::ui::theme;

pub const STATE_FLASH_DURATION: Duration = Duration::from_secs(3);
const STATE_FLASH_PULSE_MS: u128 = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneStateFlash {
    pub state: Option<IdleCause>,
    pub changed_at: Instant,
}

pub struct PaneStateFlashView<'a> {
    pub now: Instant,
    pub state_flashes: &'a HashMap<String, PaneStateFlash>,
}

impl PaneStateFlash {
    pub fn new(state: Option<IdleCause>, changed_at: Instant) -> Self {
        Self { state, changed_at }
    }

    pub fn is_active(self, now: Instant) -> bool {
        now.saturating_duration_since(self.changed_at) < STATE_FLASH_DURATION
    }
}

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
    flash_view: PaneStateFlashView<'_>,
) {
    let title = format!(
        "Panes · target {target_label} · selected pane expands below || counts panes:{}",
        reports.len()
    );
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
        .map(|(idx, report)| {
            pane_list_item(
                report,
                idx == selected,
                idx + 1 < reports.len(),
                flash_view.now,
                flash_view.state_flashes.get(&report.pane_id),
            )
        })
        .collect();

    StatefulWidget::render(
        List::new(items)
            .block(block)
            .highlight_style(highlight_style(focused))
            .highlight_symbol(highlight_symbol())
            .repeat_highlight_symbol(repeat_highlight_symbol()),
        area,
        buf,
        state,
    );
}

pub fn pane_index_at_row(reports: &[PaneReport], state: &ListState, row: u16) -> Option<usize> {
    if reports.is_empty() {
        return None;
    }
    let selected = state
        .selected()
        .unwrap_or(0)
        .min(reports.len().saturating_sub(1));
    let mut remaining = row;
    for (idx, report) in reports.iter().enumerate().skip(state.offset()) {
        let height = pane_list_lines(report, idx == selected, idx + 1 < reports.len()).len() as u16;
        if remaining < height {
            return Some(idx);
        }
        remaining = remaining.saturating_sub(height);
    }
    None
}

fn highlight_style(focused: bool) -> Style {
    let style = Style::default().fg(theme::TEXT_PRIMARY);
    if focused {
        style.bg(theme::BADGE_BG).add_modifier(Modifier::BOLD)
    } else {
        style.add_modifier(Modifier::BOLD)
    }
}

fn highlight_symbol() -> &'static str {
    "▶ "
}

fn repeat_highlight_symbol() -> bool {
    false
}

fn pane_list_item(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
    now: Instant,
    flash: Option<&PaneStateFlash>,
) -> ListItem<'static> {
    ListItem::new(pane_list_lines_with_flash(
        report,
        expanded,
        with_separator,
        now,
        flash,
    ))
}

fn pane_list_lines(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
) -> Vec<Line<'static>> {
    pane_list_lines_with_flash(report, expanded, with_separator, Instant::now(), None)
}

fn pane_list_lines_with_flash(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
    now: Instant,
    flash: Option<&PaneStateFlash>,
) -> Vec<Line<'static>> {
    let flash = matching_state_flash(report, now, flash);
    let mut lines = vec![Line::styled(
        pane_panel_title_with_flash(report, flash),
        pane_header_style(report, now, flash),
    )];
    for row in render_pane_state_row_with_flash(report, now, flash) {
        lines.push(row);
    }
    lines.push(Line::from(aligned_field(
        "path",
        &display_path(&report.current_path),
    )));
    lines.push(Line::from(aligned_field(
        "status",
        &state_summary_line(report),
    )));

    if let Some(line) = blocking_signal_line(&report.signals) {
        lines.push(line);
    }
    if let Some(line) = signal_badge_line("signals", secondary_signal_chips(&report.signals)) {
        lines.push(line);
    }

    for row in metric_badge_line(&report.signals) {
        lines.push(row);
    }
    for row in runtime_badge_lines(&report.signals) {
        lines.push(row);
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

    lines
}

fn pane_header_color(report: &PaneReport) -> Color {
    report
        .recommendations
        .iter()
        .map(|rec| rec.severity)
        .max()
        .map(theme::severity_color)
        .or_else(|| report.idle_state.map(idle_header_color))
        .unwrap_or_else(|| provider_color(report.identity.identity.provider))
}

fn pane_header_style(report: &PaneReport, now: Instant, flash: Option<&PaneStateFlash>) -> Style {
    let mut style = Style::default()
        .fg(pane_header_color(report))
        .add_modifier(Modifier::BOLD);
    if state_flash_pulse_on(flash, now) {
        style = style
            .fg(Color::Rgb(255, 244, 170))
            .bg(Color::Rgb(70, 60, 28))
            .add_modifier(Modifier::BOLD);
    }
    style
}

fn idle_header_color(cause: IdleCause) -> Color {
    match cause {
        IdleCause::WorkComplete => Color::Rgb(176, 192, 214),
        IdleCause::Stale => Color::Rgb(152, 168, 190),
        IdleCause::InputWait => Color::Rgb(236, 198, 98),
        IdleCause::PermissionWait => Color::Rgb(245, 176, 82),
        IdleCause::LimitHit => theme::severity_color(crate::domain::recommendation::Severity::Risk),
    }
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
    if id.role == Role::Unknown {
        format!(
            "{}:{} · {} · {}",
            report.session_name,
            report.window_index,
            provider_label(id.provider),
            report.pane_id,
        )
    } else {
        format!(
            "{}:{} · {} {} · {}",
            report.session_name,
            report.window_index,
            provider_label(id.provider),
            role_label(id.role),
            report.pane_id,
        )
    }
}

fn pane_panel_title_with_flash(report: &PaneReport, flash: Option<&PaneStateFlash>) -> String {
    let title = pane_panel_title(report);
    if flash.is_some() {
        format!("STATE CHANGED · {title}")
    } else {
        title
    }
}

pub fn panel_body(report: &PaneReport) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    for row in render_pane_state_row(report) {
        items.push(ListItem::new(row));
    }
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

    for row in metric_badge_line(&report.signals) {
        items.push(ListItem::new(row));
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
    if matches!(s.idle_state, Some(IdleCause::InputWait)) {
        chips.push("waiting for input");
    }
    if matches!(s.idle_state, Some(IdleCause::PermissionWait)) {
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
    if let Some(m) = s.quota_pressure.as_ref() {
        parts.push(format!(
            "quota {:.0}% [{}]",
            m.value * 100.0,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.token_count.as_ref() {
        parts.push(format!(
            "tokens {} [{}]",
            format_count_with_suffix(m.value),
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
    if let Some(m) = s.model_name.as_ref() {
        parts.push(format!(
            "model {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.git_branch.as_ref() {
        parts.push(format!(
            "branch {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.worktree_path.as_ref() {
        parts.push(format!(
            "path {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.reasoning_effort.as_ref() {
        parts.push(format!(
            "effort {} [{}]",
            m.value,
            source_kind_label(m.source_kind)
        ));
    }
    parts.join("  ")
}

pub fn runtime_row(s: &SignalSet) -> String {
    runtime_text_groups(s)
        .into_iter()
        .map(|(label, facts)| format!("{label}: {}", facts.join("  ")))
        .collect::<Vec<_>>()
        .join("  ")
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

fn metric_badge_line(signals: &SignalSet) -> Vec<Line<'static>> {
    let mut rows = Vec::with_capacity(2);
    if let Some(line) = primary_metric_row(signals) {
        rows.push(line);
    }
    if let Some(line) = context_metric_row(signals) {
        rows.push(line);
    }
    rows
}

/// DRY helper for the metric/context-row badge placement.
///
/// First call: `has_any` is `false`, no leading separator added.
/// Subsequent calls: pushes a single-space separator before the badge.
/// Always sets `*has_any = true` so the next caller knows a badge is
/// already present.
fn push_badge(spans: &mut Vec<Span<'static>>, has_any: &mut bool, content: String, style: Style) {
    if *has_any {
        spans.push(Span::raw(" "));
    }
    *has_any = true;
    spans.push(Span::styled(content, style));
}

fn primary_metric_row(signals: &SignalSet) -> Option<Line<'static>> {
    let mut spans = vec![Span::raw(format!("{:<8}: ", "metrics"))];
    let mut has_any = false;

    if let Some(metric) = signals.context_pressure.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(" CTX {:.0}% ", metric.value * 100.0),
            theme::severity_badge_style(context_metric_severity(metric.value)),
        );
    }
    if let Some(metric) = signals.quota_pressure.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(" QUOTA {:.0}% ", metric.value * 100.0),
            theme::severity_badge_style(context_metric_severity(metric.value)),
        );
    }
    if let Some(metric) = signals.token_count.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " TOKENS {} [{}] ",
                format_count_with_suffix(metric.value),
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        );
    }
    if let Some(metric) = signals.cost_usd.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " COST ${:.2} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::severity_badge_style(cost_metric_severity(metric.value)),
        );
    }
    if let Some(metric) = signals.model_name.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " MODEL {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        );
    }

    has_any.then(|| Line::from(spans))
}

fn context_metric_row(signals: &SignalSet) -> Option<Line<'static>> {
    let mut spans = vec![Span::raw(format!("{:<8}: ", "context"))];
    let mut has_any = false;

    if let Some(metric) = signals.git_branch.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " BRANCH {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        );
    }
    if let Some(metric) = signals.worktree_path.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " PATH {} [{}] ",
                ellipsize(&metric.value, 40),
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        );
    }
    if let Some(metric) = signals.reasoning_effort.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " EFFORT {} [{}] ",
                metric.value,
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        );
    }

    has_any.then(|| Line::from(spans))
}

fn runtime_badge_lines(signals: &SignalSet) -> Vec<Line<'static>> {
    runtime_text_groups(signals)
        .into_iter()
        .map(|(label, facts)| {
            let mut spans = vec![Span::raw(format!("{label:<8}: "))];
            for (idx, fact) in facts.into_iter().enumerate() {
                if idx > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(format!(" {fact} "), theme::label_style()));
            }
            Line::from(spans)
        })
        .collect()
}

fn runtime_text_groups(signals: &SignalSet) -> Vec<(&'static str, Vec<String>)> {
    const GROUPS: &[(&str, &[RuntimeFactKind])] = &[
        (
            "modes",
            &[
                RuntimeFactKind::PermissionMode,
                RuntimeFactKind::AutoMode,
                RuntimeFactKind::Sandbox,
            ],
        ),
        (
            "access",
            &[
                RuntimeFactKind::AllowedDirectory,
                RuntimeFactKind::AgentConfig,
            ],
        ),
        (
            "loaded",
            &[
                RuntimeFactKind::LoadedTool,
                RuntimeFactKind::LoadedSkill,
                RuntimeFactKind::LoadedPlugin,
            ],
        ),
        ("restrict", &[RuntimeFactKind::RestrictedTool]),
    ];

    GROUPS
        .iter()
        .filter_map(|(label, kinds)| {
            let facts: Vec<String> = signals
                .runtime_facts
                .iter()
                .filter(|fact| kinds.contains(&fact.kind))
                .map(format_runtime_fact)
                .collect();
            (!facts.is_empty()).then_some((*label, facts))
        })
        .collect()
}

fn format_runtime_fact(fact: &RuntimeFact) -> String {
    format!(
        "{} {} [{}]",
        runtime_fact_label(fact.kind),
        fact.value,
        source_kind_label(fact.source_kind)
    )
}

fn runtime_fact_label(kind: RuntimeFactKind) -> &'static str {
    match kind {
        RuntimeFactKind::PermissionMode => "PERM",
        RuntimeFactKind::AutoMode => "MODE",
        RuntimeFactKind::Sandbox => "SANDBOX",
        RuntimeFactKind::AllowedDirectory => "DIR",
        RuntimeFactKind::AgentConfig => "AGENTS",
        RuntimeFactKind::LoadedTool => "TOOL",
        RuntimeFactKind::LoadedSkill => "SKILL",
        RuntimeFactKind::LoadedPlugin => "PLUGIN",
        RuntimeFactKind::RestrictedTool => "TOOL",
    }
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

/// v1.15.15: cost-side severity coloring on the COST badge mirrors the
/// v1.15.14 cost_pressure_* advisory thresholds:
///   - >= $20.00 USD → Risk    (matches cost_pressure_critical)
///   - >= $5.00  USD → Warning (matches cost_pressure_warning)
///   - >= $2.00  USD → Concern (early warning before the advisory fires)
///   - else            Good   (no spend pressure)
///
/// The numeric thresholds are kept in sync with the advisory rule
/// constants in `policy/rules/advisories.rs`.
fn cost_metric_severity(value: f64) -> crate::domain::recommendation::Severity {
    use crate::domain::recommendation::Severity;
    if value >= 20.0 {
        Severity::Risk
    } else if value >= 5.0 {
        Severity::Warning
    } else if value >= 2.0 {
        Severity::Concern
    } else {
        Severity::Good
    }
}

/// Produces a single styled line carrying glyph + label + elapsed time for
/// a pane that is currently in an idle cause. Callers are responsible for
/// only calling this when `idle_state.is_some()`.
#[cfg(test)]
fn format_state_row(cause: IdleCause, entered_at: Instant) -> Line<'static> {
    format_state_row_with_flash(cause, entered_at, Instant::now(), None)
}

fn format_state_row_with_flash(
    cause: IdleCause,
    entered_at: Instant,
    now: Instant,
    flash: Option<&PaneStateFlash>,
) -> Line<'static> {
    let (glyph, label, badge_style) = match cause {
        IdleCause::WorkComplete => ("⏹", "IDLE (done)", theme::idle_work_complete()),
        IdleCause::Stale => ("⏸", "IDLE (?)", theme::idle_stale()),
        IdleCause::InputWait => ("⏸", "WAIT (input)", theme::idle_input_wait()),
        IdleCause::PermissionWait => ("⚠", "WAIT (approval)", theme::idle_permission_wait()),
        IdleCause::LimitHit => ("⛔", "USAGE LIMIT", theme::idle_limit_hit()),
    };
    let pulse_on = state_flash_pulse_on(flash, now);
    let changed = flash.is_some_and(|flash| flash.is_active(now));
    let elapsed_str = format_elapsed(now.saturating_duration_since(entered_at));
    let mut spans = vec![
        Span::styled(format!("{:<8}: ", "state"), state_label_style(pulse_on)),
        Span::styled(
            format!(" {glyph} {label} "),
            flashable_badge_style(badge_style, pulse_on),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" ⏱ {elapsed_str} "),
            flashable_badge_style(theme::idle_elapsed_badge(), pulse_on),
        ),
    ];
    if changed {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            " CHANGED ",
            state_changed_badge_style(pulse_on),
        ));
    }
    Line::from(spans)
}

fn format_active_state_row(now: Instant, flash: &PaneStateFlash) -> Line<'static> {
    let pulse_on = state_flash_pulse_on(Some(flash), now);
    Line::from(vec![
        Span::styled(format!("{:<8}: ", "state"), state_label_style(pulse_on)),
        Span::styled(
            " ▶ ACTIVE ",
            flashable_badge_style(active_state_badge_style(), pulse_on),
        ),
        Span::raw(" "),
        Span::styled(" CHANGED ", state_changed_badge_style(pulse_on)),
    ])
}

fn format_elapsed(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if secs < 3600 {
        format!("{minutes:02}:{seconds:02}")
    } else {
        format!("{}:{minutes:02}:{seconds:02}", secs / 3600)
    }
}

/// Returns a one-element vec with the state row when the report carries an
/// active idle cause, or an empty vec when the pane is running normally.
/// Used by the pane card and directly in tests.
fn render_pane_state_row(report: &PaneReport) -> Vec<Line<'static>> {
    render_pane_state_row_with_flash(report, Instant::now(), None)
}

fn render_pane_state_row_with_flash(
    report: &PaneReport,
    now: Instant,
    flash: Option<&PaneStateFlash>,
) -> Vec<Line<'static>> {
    let flash = matching_state_flash(report, now, flash);
    if let (Some(cause), Some(entered_at)) = (report.idle_state, report.idle_state_entered_at) {
        vec![format_state_row_with_flash(cause, entered_at, now, flash)]
    } else if report.idle_state.is_none() {
        flash
            .map(|flash| vec![format_active_state_row(now, flash)])
            .unwrap_or_default()
    } else {
        vec![]
    }
}

fn matching_state_flash<'a>(
    report: &PaneReport,
    now: Instant,
    flash: Option<&'a PaneStateFlash>,
) -> Option<&'a PaneStateFlash> {
    flash.filter(|flash| flash.state == report.idle_state && flash.is_active(now))
}

fn state_flash_pulse_on(flash: Option<&PaneStateFlash>, now: Instant) -> bool {
    flash.is_some_and(|flash| {
        flash.is_active(now)
            && (now.saturating_duration_since(flash.changed_at).as_millis() / STATE_FLASH_PULSE_MS)
                .is_multiple_of(2)
    })
}

fn state_label_style(pulse_on: bool) -> Style {
    if pulse_on {
        Style::default()
            .fg(Color::Rgb(28, 24, 12))
            .bg(Color::Rgb(245, 226, 120))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    }
}

fn flashable_badge_style(style: Style, pulse_on: bool) -> Style {
    if pulse_on {
        Style::default()
            .fg(Color::Rgb(28, 24, 12))
            .bg(Color::Rgb(245, 226, 120))
            .add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn state_changed_badge_style(pulse_on: bool) -> Style {
    if pulse_on {
        Style::default()
            .fg(Color::Rgb(28, 24, 12))
            .bg(Color::Rgb(255, 240, 150))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Rgb(246, 232, 150))
            .bg(Color::Rgb(76, 66, 34))
            .add_modifier(Modifier::BOLD)
    }
}

fn active_state_badge_style() -> Style {
    Style::default()
        .fg(Color::Rgb(18, 34, 28))
        .bg(Color::Rgb(112, 192, 156))
        .add_modifier(Modifier::BOLD)
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
    use crate::domain::signal::{MetricValue, RuntimeFact, RuntimeFactKind, SignalSet};

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
            idle_state: None,
            idle_state_entered_at: None,
        }
    }

    #[test]
    fn panel_title_includes_identity_and_confidence() {
        let rep = base_report();
        assert_eq!(pane_panel_title(&rep), "qwork:1 · Claude main · %1");
    }

    #[test]
    fn panel_title_omits_unknown_role_word() {
        let mut rep = base_report();
        rep.identity.identity.role = Role::Unknown;
        assert_eq!(pane_panel_title(&rep), "qwork:1 · Claude · %1");
    }

    #[test]
    fn signal_chips_reflect_booleans() {
        let s = SignalSet {
            idle_state: Some(IdleCause::InputWait),
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
    fn metric_row_renders_quota_pressure_when_populated() {
        // S3-3: Gemini quota_pressure surfaces in the --once metric
        // row alongside context_pressure. Same percent + source-kind
        // shape as context.
        let s = SignalSet {
            quota_pressure: Some(
                MetricValue::new(0.47, SourceKind::ProviderOfficial).with_confidence(0.95),
            ),
            ..SignalSet::default()
        };
        let row = metric_row(&s);
        assert!(row.contains("quota 47%"));
        assert!(row.contains("[Official]"));
    }

    #[test]
    fn cost_metric_severity_thresholds_match_advisory_rule_pair() {
        // v1.15.15: cost severity thresholds must stay in sync with
        // the v1.15.14 cost_pressure_warning ($5.00) and
        // cost_pressure_critical ($20.00) advisory thresholds, plus
        // an early Concern band at $2.00 so the COST badge starts
        // tinting before the advisory fires.
        use crate::domain::recommendation::Severity;
        assert_eq!(cost_metric_severity(0.0), Severity::Good);
        assert_eq!(cost_metric_severity(1.99), Severity::Good);
        assert_eq!(cost_metric_severity(2.0), Severity::Concern);
        assert_eq!(cost_metric_severity(4.99), Severity::Concern);
        assert_eq!(cost_metric_severity(5.0), Severity::Warning);
        assert_eq!(cost_metric_severity(19.99), Severity::Warning);
        assert_eq!(cost_metric_severity(20.0), Severity::Risk);
        assert_eq!(cost_metric_severity(100.0), Severity::Risk);
    }

    #[test]
    fn state_summary_line_uses_full_words_instead_of_chips() {
        let mut rep = base_report();
        rep.signals = SignalSet {
            idle_state: Some(IdleCause::InputWait),
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

    #[test]
    fn metric_row_renders_model_name_line_when_populated() {
        let s = crate::domain::signal::SignalSet {
            model_name: Some(crate::domain::signal::MetricValue::new(
                "gpt-5.4".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let row = metric_row(&s);
        assert!(row.contains("model gpt-5.4"), "row: {row}");
        assert!(row.contains("Official"), "row: {row}");
    }

    #[test]
    fn metric_row_uses_count_suffix_for_tokens() {
        let s = crate::domain::signal::SignalSet {
            token_count: Some(crate::domain::signal::MetricValue::new(
                1_530_000,
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let row = metric_row(&s);
        assert!(row.contains("tokens 1.53M"), "got: {row}");
    }

    #[test]
    fn metric_row_renders_git_branch_and_worktree_and_effort() {
        let s = crate::domain::signal::SignalSet {
            git_branch: Some(crate::domain::signal::MetricValue::new(
                "main".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            worktree_path: Some(crate::domain::signal::MetricValue::new(
                "~/Qmonster".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            reasoning_effort: Some(crate::domain::signal::MetricValue::new(
                "xhigh".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let row = metric_row(&s);
        assert!(row.contains("branch main"), "row: {row}");
        assert!(row.contains("path ~/Qmonster"), "row: {row}");
        assert!(row.contains("effort xhigh"), "row: {row}");
    }

    #[test]
    fn runtime_row_groups_modes_access_loaded_and_restricted_facts() {
        let s = crate::domain::signal::SignalSet {
            runtime_facts: vec![
                RuntimeFact::new(
                    RuntimeFactKind::PermissionMode,
                    "Full Access",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::Sandbox,
                    "danger-full-access",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::AllowedDirectory,
                    "~/Qmonster",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::LoadedSkill,
                    "superpowers:executing-plans",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::RestrictedTool,
                    "Bash(rm *)",
                    SourceKind::ProviderOfficial,
                ),
            ],
            ..crate::domain::signal::SignalSet::default()
        };
        let row = runtime_row(&s);
        assert!(row.contains("modes:"), "row: {row}");
        assert!(row.contains("PERM Full Access [Official]"), "row: {row}");
        assert!(
            row.contains("SANDBOX danger-full-access [Official]"),
            "row: {row}"
        );
        assert!(row.contains("access:"), "row: {row}");
        assert!(row.contains("DIR ~/Qmonster [Official]"), "row: {row}");
        assert!(row.contains("loaded:"), "row: {row}");
        assert!(
            row.contains("SKILL superpowers:executing-plans [Official]"),
            "row: {row}"
        );
        assert!(row.contains("restrict:"), "row: {row}");
        assert!(row.contains("TOOL Bash(rm *) [Official]"), "row: {row}");
    }

    #[test]
    fn runtime_badge_lines_returns_one_row_per_populated_group() {
        let s = crate::domain::signal::SignalSet {
            runtime_facts: vec![
                RuntimeFact::new(
                    RuntimeFactKind::AutoMode,
                    "YOLO mode",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::LoadedTool,
                    "Bash",
                    SourceKind::ProviderOfficial,
                ),
            ],
            ..crate::domain::signal::SignalSet::default()
        };
        let rows = runtime_badge_lines(&s);
        assert_eq!(rows.len(), 2);
        let text: Vec<String> = rows
            .into_iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(text.iter().any(|line| line.starts_with("modes")));
        assert!(text.iter().any(|line| line.starts_with("loaded")));
    }

    #[test]
    fn metric_badge_line_returns_two_rows_when_context_fields_present() {
        let s = crate::domain::signal::SignalSet {
            token_count: Some(crate::domain::signal::MetricValue::new(
                1_530_000_u64,
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            git_branch: Some(crate::domain::signal::MetricValue::new(
                "main".to_string(),
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let rows = metric_badge_line(&s);
        assert_eq!(
            rows.len(),
            2,
            "TOKENS on row 1 + BRANCH on row 2 → exactly two rows"
        );
    }

    #[test]
    fn metric_badge_line_returns_single_row_when_only_primary_fields_present() {
        let s = crate::domain::signal::SignalSet {
            token_count: Some(crate::domain::signal::MetricValue::new(
                1_530_000_u64,
                crate::domain::origin::SourceKind::ProviderOfficial,
            )),
            ..crate::domain::signal::SignalSet::default()
        };
        let rows = metric_badge_line(&s);
        assert_eq!(rows.len(), 1, "primary fields only → one row");
    }

    #[test]
    fn metric_badge_line_returns_empty_vec_when_no_fields() {
        let rows = metric_badge_line(&crate::domain::signal::SignalSet::default());
        assert!(
            rows.is_empty(),
            "no fields populated → empty Vec (not a single empty Line)"
        );
    }

    #[test]
    fn state_row_renders_input_wait_with_yellow_glyph_and_label() {
        let entered_at = std::time::Instant::now();
        let line = format_state_row(IdleCause::InputWait, entered_at);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.starts_with("state   : "),
            "field alignment missing: {text}"
        );
        assert!(text.contains("⏸"), "glyph missing: {text}");
        assert!(text.contains("WAIT (input)"), "label missing: {text}");
        assert!(text.contains("⏱"), "elapsed badge missing: {text}");
    }

    #[test]
    fn state_row_renders_limit_hit_as_usage_limit() {
        let entered_at = std::time::Instant::now();
        let line = format_state_row(IdleCause::LimitHit, entered_at);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("⛔"), "glyph missing: {text}");
        assert!(
            text.contains("USAGE LIMIT"),
            "limit state must be distinct from normal idle: {text}"
        );
    }

    #[test]
    fn state_row_marks_recent_transition_as_changed() {
        let now = std::time::Instant::now();
        let flash = PaneStateFlash::new(Some(IdleCause::InputWait), now);
        let line = format_state_row_with_flash(IdleCause::InputWait, now, now, Some(&flash));
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("WAIT (input)"), "label missing: {text}");
        assert!(text.contains("CHANGED"), "changed badge missing: {text}");
    }

    #[test]
    fn selection_highlight_stays_stable_during_state_flash() {
        let style = highlight_style(true);
        assert_eq!(style.fg, Some(theme::TEXT_PRIMARY));
        assert_eq!(style.bg, Some(theme::BADGE_BG));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn selection_marker_does_not_encode_state_flash() {
        assert_eq!(highlight_symbol(), "▶ ");
        assert!(!repeat_highlight_symbol());
    }

    #[test]
    fn state_flash_header_names_state_changed_for_all_selection_states() {
        let rep = base_report();
        let now = std::time::Instant::now();
        let flash = PaneStateFlash::new(None, now);
        let expanded_lines = pane_list_lines_with_flash(&rep, true, false, now, Some(&flash));
        let collapsed_lines = pane_list_lines_with_flash(&rep, false, false, now, Some(&flash));
        let expanded_text: String = expanded_lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        let collapsed_text: String = collapsed_lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();

        assert!(
            expanded_text.starts_with("STATE CHANGED · qwork:1"),
            "expanded card flash header missing: {expanded_text}"
        );
        assert!(
            collapsed_text.starts_with("STATE CHANGED · qwork:1"),
            "collapsed card flash header missing: {collapsed_text}"
        );
    }

    #[test]
    fn active_transition_renders_temporary_state_row() {
        let rep = base_report();
        let now = std::time::Instant::now();
        let flash = PaneStateFlash::new(None, now);
        let lines = render_pane_state_row_with_flash(&rep, now, Some(&flash));
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("ACTIVE"), "active badge missing: {text}");
        assert!(text.contains("CHANGED"), "changed badge missing: {text}");
    }

    #[test]
    fn expired_active_transition_flash_removes_state_row() {
        let rep = base_report();
        let now = std::time::Instant::now();
        let flash =
            PaneStateFlash::new(None, now - STATE_FLASH_DURATION - Duration::from_millis(1));
        let lines = render_pane_state_row_with_flash(&rep, now, Some(&flash));
        assert!(
            lines.is_empty(),
            "expired flash should not render state row"
        );
    }

    #[test]
    fn format_elapsed_uses_clock_style_for_fast_scanning() {
        assert_eq!(format_elapsed(std::time::Duration::from_secs(5)), "00:05");
        assert_eq!(format_elapsed(std::time::Duration::from_secs(65)), "01:05");
        assert_eq!(
            format_elapsed(std::time::Duration::from_secs(3_665)),
            "1:01:05"
        );
    }

    #[test]
    fn idle_state_tints_header_when_no_recommendations_are_active() {
        let mut rep = base_report();
        rep.idle_state = Some(IdleCause::PermissionWait);
        assert_eq!(
            pane_header_color(&rep),
            idle_header_color(IdleCause::PermissionWait)
        );
    }

    #[test]
    fn state_row_omitted_for_idle_state_none() {
        let rep = base_report();
        let lines = render_pane_state_row(&rep);
        assert!(lines.is_empty(), "no state row when idle_state is None");
    }
}
