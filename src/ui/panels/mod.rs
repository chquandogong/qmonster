use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::prelude::*;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::domain::identity::{IdentityConfidence, Provider, Role};
use crate::domain::signal::{IdleCause, MetricValue, RuntimeFact, RuntimeFactKind, SignalSet};
use crate::ui::help_glossary::HelpTopic;
use crate::ui::labels::{ellipsize, format_count_with_suffix, source_kind_label};
use crate::ui::provider_honesty::{self, CacheMetricStatus};
use crate::ui::theme;

pub const STATE_FLASH_DURATION: Duration = Duration::from_secs(3);
const STATE_FLASH_PULSE_MS: u128 = 350;
const BADGE_WRAP_FALLBACK_WIDTH: u16 = 96;

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
            .style(Style::default().fg(theme::text_dim()))
            .block(block)
            .render(area, buf);
        return;
    }

    let selected = state
        .selected()
        .unwrap_or(0)
        .min(reports.len().saturating_sub(1));
    state.select(Some(selected));
    let wrap_width = pane_badge_wrap_width(area);
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
                wrap_width,
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

pub fn pane_index_at_row(
    reports: &[PaneReport],
    state: &ListState,
    row: u16,
    wrap_width: u16,
) -> Option<usize> {
    if reports.is_empty() {
        return None;
    }
    let selected = state
        .selected()
        .unwrap_or(0)
        .min(reports.len().saturating_sub(1));
    let mut remaining = row;
    for (idx, report) in reports.iter().enumerate().skip(state.offset()) {
        let height = pane_list_lines_with_width(
            report,
            idx == selected,
            idx + 1 < reports.len(),
            wrap_width,
        )
        .len() as u16;
        if remaining < height {
            return Some(idx);
        }
        remaining = remaining.saturating_sub(height);
    }
    None
}

pub fn pane_help_topic_at_row(
    reports: &[PaneReport],
    state: &ListState,
    row: u16,
    wrap_width: u16,
) -> Option<crate::ui::help_glossary::HelpTopic> {
    if reports.is_empty() {
        return None;
    }
    let selected = state
        .selected()
        .unwrap_or(0)
        .min(reports.len().saturating_sub(1));
    let mut remaining = row;
    for (idx, report) in reports.iter().enumerate().skip(state.offset()) {
        let topics = pane_list_help_topics_with_width(
            report,
            idx == selected,
            idx + 1 < reports.len(),
            wrap_width,
        );
        let height = topics.len() as u16;
        if remaining < height {
            return topics.get(remaining as usize).and_then(|topic| *topic);
        }
        remaining = remaining.saturating_sub(height);
    }
    None
}

fn pane_list_help_topics_with_width(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
    wrap_width: u16,
) -> Vec<Option<HelpTopic>> {
    let now = Instant::now();
    let mut topics = vec![Some(HelpTopic::PaneHeader)];
    if expanded {
        topics.extend(
            pane_sectioned_rows(
                report,
                now,
                None,
                wrap_width,
                PaneSectionOptions::expanded_list(),
            )
            .into_iter()
            .map(|row| row.topic),
        );
    } else {
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneState),
            render_pane_state_row_with_flash(report, now, None, wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PanePath),
            wrap_aligned_field("path", &path_row_value(report), wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneCommand),
            wrap_aligned_field("cmd", &display_command(&report.current_command), wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneStatus),
            wrap_aligned_field("status", &state_summary_line(report), wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneSignals),
            blocking_signal_lines(&report.signals, wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneSignals),
            signal_badge_lines(
                "signals",
                secondary_signal_chips(&report.signals),
                wrap_width,
            )
            .len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneMetrics),
            metric_badge_lines(
                &report.signals,
                report.identity.identity.provider,
                wrap_width,
                true,
            )
            .len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneRuntime),
            runtime_badge_lines_wrapped(&report.signals, wrap_width).len(),
        );
    }

    if with_separator {
        // uniform-vNext S3b: keep this 1:1 with the rendered separator block,
        // which is now a blank spacer line + the divider rule (two lines).
        topics.push(None); // blank spacer
        topics.push(None); // divider rule
    }
    topics
}

fn push_topic_count<T>(topics: &mut Vec<Option<T>>, topic: Option<T>, count: usize)
where
    T: Copy,
{
    topics.extend(std::iter::repeat_n(topic, count));
}

struct PaneRenderLine {
    line: Line<'static>,
    topic: Option<HelpTopic>,
}

enum PaneTokenRows {
    ExpandedList,
    PanelBody,
}

struct PaneSectionOptions {
    recommendation_limit: usize,
    include_proposal: bool,
    token_rows: PaneTokenRows,
    show_empty_recommendations: bool,
}

impl PaneSectionOptions {
    fn expanded_list() -> Self {
        Self {
            recommendation_limit: 3,
            include_proposal: true,
            token_rows: PaneTokenRows::ExpandedList,
            show_empty_recommendations: true,
        }
    }

    fn panel_body() -> Self {
        Self {
            recommendation_limit: 6,
            include_proposal: false,
            token_rows: PaneTokenRows::PanelBody,
            show_empty_recommendations: true,
        }
    }
}

fn pane_topic_line(line: Line<'static>, topic: HelpTopic) -> PaneRenderLine {
    PaneRenderLine {
        line,
        topic: Some(topic),
    }
}

fn pane_section_header(title: &'static str) -> Line<'static> {
    Line::styled(
        title,
        Style::default()
            .fg(theme::text_dim())
            .add_modifier(Modifier::BOLD),
    )
}

const SECTION_INDENT_WIDTH: u16 = 2;

struct PaneSectionRow {
    /// First line + any continuation lines produced by wrapping.
    lines: Vec<Line<'static>>,
    topic: HelpTopic,
    /// Sub-detail rows nested under this row (rendered one level deeper).
    children: Vec<PaneSectionRow>,
}

impl PaneSectionRow {
    fn leaf(lines: Vec<Line<'static>>, topic: HelpTopic) -> Self {
        Self {
            lines,
            topic,
            children: Vec::new(),
        }
    }
}

/// Split a flat `Vec<Line>` into logical row groups. A line whose first
/// span starts with whitespace is treated as a continuation of the
/// preceding row; otherwise it starts a new row. Matches the shape
/// produced by `wrap_aligned_field` (label prefix on the first line,
/// space-padded `cont_indent` on continuations) and by `wrap_badge_line`
/// (label-bearing prefix span on the first line, whitespace-only
/// `continuation_prefix` on continuations).
fn group_into_section_rows(lines: Vec<Line<'static>>, topic: HelpTopic) -> Vec<PaneSectionRow> {
    let mut rows: Vec<PaneSectionRow> = Vec::new();
    for line in lines {
        let is_continuation = line
            .spans
            .first()
            .and_then(|s| s.content.chars().next())
            .is_some_and(|c| c.is_whitespace());
        if is_continuation && let Some(last) = rows.last_mut() {
            last.lines.push(line);
            continue;
        }
        rows.push(PaneSectionRow::leaf(vec![line], topic));
    }
    rows
}

fn push_pane_section_tree(
    out: &mut Vec<PaneRenderLine>,
    title: &'static str,
    topic: HelpTopic,
    rows: Vec<PaneSectionRow>,
) {
    if rows.is_empty() {
        return;
    }
    out.push(pane_topic_line(pane_section_header(title), topic));
    render_section_rows(out, &[], rows);
}

fn render_section_rows(
    out: &mut Vec<PaneRenderLine>,
    ancestor_continues: &[bool],
    rows: Vec<PaneSectionRow>,
) {
    let last_idx = rows.len().saturating_sub(1);
    for (i, row) in rows.into_iter().enumerate() {
        let is_last = i == last_idx;
        let PaneSectionRow {
            lines,
            topic,
            children,
        } = row;
        for (line_idx, mut line) in lines.into_iter().enumerate() {
            let prefix = build_tree_prefix(ancestor_continues, is_last, line_idx == 0);
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(Span::raw(prefix));
            spans.append(&mut line.spans);
            line.spans = spans;
            out.push(PaneRenderLine {
                line,
                topic: Some(topic),
            });
        }
        if !children.is_empty() {
            let mut next_ancestors: Vec<bool> = ancestor_continues.to_vec();
            next_ancestors.push(!is_last);
            render_section_rows(out, &next_ancestors, children);
        }
    }
}

fn build_tree_prefix(ancestor_continues: &[bool], is_last: bool, is_first: bool) -> String {
    let mut s = String::with_capacity((ancestor_continues.len() + 1) * 2);
    for &cont in ancestor_continues {
        s.push_str(if cont { "│ " } else { "  " });
    }
    s.push_str(match (is_first, is_last) {
        (true, false) => "├ ",
        (true, true) => "└ ",
        (false, false) => "│ ",
        (false, true) => "  ",
    });
    s
}

fn pane_sectioned_rows(
    report: &PaneReport,
    now: Instant,
    flash: Option<&PaneStateFlash>,
    wrap_width: u16,
    options: PaneSectionOptions,
) -> Vec<PaneRenderLine> {
    let mut rows = Vec::new();
    let section_wrap = wrap_width.saturating_sub(SECTION_INDENT_WIDTH);
    let detail_wrap = section_wrap.saturating_sub(SECTION_INDENT_WIDTH);

    // IDENTITY — the stable "who + where": path/cmd rows (the
    // provider/role/CLI-version + IDENTITY CONFLICT line live in the card
    // title via `pane_panel_title`). The path row keeps its worktree-role
    // `· wt of <parent>` hint through `path_row_value`.
    let mut identity_rows: Vec<PaneSectionRow> = Vec::new();
    identity_rows.extend(group_into_section_rows(
        wrap_aligned_field("path", &path_row_value(report), section_wrap),
        HelpTopic::PanePath,
    ));
    identity_rows.extend(group_into_section_rows(
        wrap_aligned_field(
            "cmd",
            &display_command(&report.current_command),
            section_wrap,
        ),
        HelpTopic::PaneCommand,
    ));
    push_pane_section_tree(&mut rows, "IDENTITY", HelpTopic::PanePath, identity_rows);

    // NOW — the live state: idle state/reason + signal rows, the identity
    // `status` row (moved here from the old WHERE section), and any pending
    // prompt-send proposal.
    let mut now_rows: Vec<PaneSectionRow> = Vec::new();
    now_rows.extend(group_into_section_rows(
        render_pane_state_row_with_flash(report, now, flash, section_wrap),
        HelpTopic::PaneState,
    ));
    now_rows.extend(group_into_section_rows(
        wrap_aligned_field("status", &state_summary_line(report), section_wrap),
        HelpTopic::PaneStatus,
    ));
    now_rows.extend(group_into_section_rows(
        blocking_signal_lines(&report.signals, section_wrap),
        HelpTopic::PaneSignals,
    ));
    now_rows.extend(group_into_section_rows(
        signal_badge_lines(
            "signals",
            secondary_signal_chips(&report.signals),
            section_wrap,
        ),
        HelpTopic::PaneSignals,
    ));
    if options.include_proposal
        && let Some((_target, slash)) =
            crate::app::prompt_send_actions::first_prompt_send_proposal(report)
    {
        now_rows.extend(group_into_section_rows(
            wrap_aligned_field(
                "proposal",
                &format!("{slash}  \u{2192} press p to accept \u{00b7} d to reject"),
                section_wrap,
            ),
            HelpTopic::PaneRecommendation,
        ));
    }
    push_pane_section_tree(&mut rows, "NOW", HelpTopic::PaneState, now_rows);

    // PRESSURE — all metrics: context/quota/cache/token rows (old PRESSURE)
    // MERGED with the runtime/mem rows (old RUNTIME) into one section.
    let mut pressure_rows: Vec<PaneSectionRow> = group_into_section_rows(
        metric_badge_lines(
            &report.signals,
            report.identity.identity.provider,
            section_wrap,
            true,
        ),
        HelpTopic::PaneMetrics,
    );
    match options.token_rows {
        PaneTokenRows::ExpandedList => {
            if token_rows_supported(report.identity.identity.provider) {
                pressure_rows.push(PaneSectionRow::leaf(
                    vec![token_sparkline_status_line(
                        &report.recent_token_samples,
                        &report.signals,
                    )],
                    HelpTopic::PaneTokens,
                ));
                if let Some(line) = token_io_line(report) {
                    pressure_rows.push(PaneSectionRow::leaf(
                        vec![Line::from(line)],
                        HelpTopic::PaneTokens,
                    ));
                }
                if let Some(line) = cache_token_io_line(report) {
                    pressure_rows.push(PaneSectionRow::leaf(
                        vec![Line::from(line)],
                        HelpTopic::PaneTokens,
                    ));
                }
            }
        }
        PaneTokenRows::PanelBody => {
            if token_rows_supported(report.identity.identity.provider)
                && let Some(line) = token_breakdown_line(report)
            {
                pressure_rows.push(PaneSectionRow::leaf(
                    vec![Line::from(line)],
                    HelpTopic::PaneTokens,
                ));
            }
        }
    }
    // RUNTIME rows (session/permission/sandbox/mem facts) now live inside
    // PRESSURE — kept under HelpTopic::PaneRuntime for hover-help.
    pressure_rows.extend(group_into_section_rows(
        runtime_badge_lines_wrapped(&report.signals, section_wrap),
        HelpTopic::PaneRuntime,
    ));
    push_pane_section_tree(&mut rows, "PRESSURE", HelpTopic::PaneMetrics, pressure_rows);

    // NEXT — the recommendations section (renamed from RECOMMENDATIONS).
    let mut recommendation_rows: Vec<PaneSectionRow> = Vec::new();
    for rec in report
        .recommendations
        .iter()
        .take(options.recommendation_limit)
    {
        let parent_lines =
            wrap_aligned_field(severity_label(rec.severity), &rec.reason, section_wrap);
        let mut children: Vec<PaneSectionRow> = Vec::new();
        for detail in crate::ui::alerts::recommendation_detail_lines(rec) {
            let formatted = expanded_detail_field(&detail);
            children.push(PaneSectionRow::leaf(
                reflow_already_aligned(&formatted, detail_wrap),
                HelpTopic::PaneRecommendation,
            ));
        }
        recommendation_rows.push(PaneSectionRow {
            lines: parent_lines,
            topic: HelpTopic::PaneRecommendation,
            children,
        });
    }
    if recommendation_rows.is_empty() && options.show_empty_recommendations {
        recommendation_rows.push(PaneSectionRow::leaf(
            wrap_aligned_field("status", "no active recommendations", section_wrap),
            HelpTopic::PaneRecommendation,
        ));
    }
    push_pane_section_tree(
        &mut rows,
        "NEXT",
        HelpTopic::PaneRecommendation,
        recommendation_rows,
    );

    rows
}

fn highlight_style(_focused: bool) -> Style {
    Style::default()
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
    wrap_width: u16,
) -> ListItem<'static> {
    ListItem::new(pane_list_lines_with_flash(
        report,
        expanded,
        with_separator,
        now,
        flash,
        wrap_width,
    ))
}

#[cfg(test)]
fn pane_list_lines(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
) -> Vec<Line<'static>> {
    pane_list_lines_with_width(report, expanded, with_separator, BADGE_WRAP_FALLBACK_WIDTH)
}

fn pane_list_lines_with_width(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
    wrap_width: u16,
) -> Vec<Line<'static>> {
    pane_list_lines_with_flash(
        report,
        expanded,
        with_separator,
        Instant::now(),
        None,
        wrap_width,
    )
}

fn pane_list_lines_with_flash(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
    now: Instant,
    flash: Option<&PaneStateFlash>,
    wrap_width: u16,
) -> Vec<Line<'static>> {
    let flash = matching_state_flash(report, now, flash);
    let mut lines = vec![pane_panel_title_line(report, now, flash)];
    if expanded {
        lines.extend(
            pane_sectioned_rows(
                report,
                now,
                flash,
                wrap_width,
                PaneSectionOptions::expanded_list(),
            )
            .into_iter()
            .map(|row| row.line),
        );
    } else {
        for row in render_pane_state_row_with_flash(report, now, flash, wrap_width) {
            lines.push(row);
        }
        lines.extend(wrap_aligned_field(
            "path",
            &path_row_value(report),
            wrap_width,
        ));
        lines.extend(wrap_aligned_field(
            "cmd",
            &display_command(&report.current_command),
            wrap_width,
        ));
        lines.extend(wrap_aligned_field(
            "status",
            &state_summary_line(report),
            wrap_width,
        ));

        lines.extend(blocking_signal_lines(&report.signals, wrap_width));
        lines.extend(signal_badge_lines(
            "signals",
            secondary_signal_chips(&report.signals),
            wrap_width,
        ));

        for row in metric_badge_lines(
            &report.signals,
            report.identity.identity.provider,
            wrap_width,
            true,
        ) {
            lines.push(row);
        }
        for row in runtime_badge_lines_wrapped(&report.signals, wrap_width) {
            lines.push(row);
        }
    }

    if with_separator {
        // uniform-vNext S3b: a blank line above the divider gives each pane
        // "tile" breathing room (시원) between cards.
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "────────────────────────────────────────",
            Style::default().fg(theme::text_dim()),
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
        Provider::Antigravity => theme::text_primary(),
        Provider::Qmonster => Color::Rgb(175, 160, 210),
        Provider::Unknown => theme::text_primary(),
    }
}

/// Per-pane panel: header line shows identity + confidence, then a
/// signals chip row, then a metrics row with SourceKind badges, then
/// recommendations.
pub fn render_pane_panel(area: Rect, buf: &mut Buffer, report: &PaneReport) {
    let title = pane_panel_title(report);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_idle()))
        .title(title);

    if report.dead {
        Paragraph::new("DEAD — alerts drained")
            .style(Style::default().fg(theme::text_dim()))
            .block(block)
            .render(area, buf);
        return;
    }

    let items = panel_body_with_width(report, pane_badge_wrap_width(area));
    Widget::render(List::new(items).block(block), area, buf);
}

pub fn pane_panel_title(report: &PaneReport) -> String {
    let id = &report.identity.identity;
    let cli_version = pane_title_cli_version(report);
    let conflict = if matches!(report.identity.confidence, IdentityConfidence::Conflict) {
        "IDENTITY CONFLICT · "
    } else {
        ""
    };
    if id.role == Role::Unknown {
        format!(
            "{}:{} · {}{}{} · {}",
            report.session_name,
            report.window_index,
            conflict,
            provider_label(id.provider),
            cli_version,
            report.pane_id,
        )
    } else {
        format!(
            "{}:{} · {}{} {}{} · {}",
            report.session_name,
            report.window_index,
            conflict,
            provider_label(id.provider),
            role_label(id.role),
            cli_version,
            report.pane_id,
        )
    }
}

fn pane_title_cli_version(report: &PaneReport) -> String {
    if report.identity.identity.provider == Provider::Qmonster {
        return String::new();
    }
    report
        .signals
        .runtime_facts
        .iter()
        .find(|fact| fact.kind == RuntimeFactKind::CliVersion)
        .map(|fact| {
            format!(
                " · CLI {} [{}]",
                fact.value,
                source_kind_label(fact.source_kind)
            )
        })
        .unwrap_or_default()
}

fn pane_panel_title_line(
    report: &PaneReport,
    now: Instant,
    flash: Option<&PaneStateFlash>,
) -> Line<'static> {
    let mut spans = Vec::new();
    let pulse_on = state_flash_pulse_on(flash, now);
    if flash.is_some() {
        spans.push(Span::styled(
            " STATE CHANGED ",
            state_changed_badge_style(pulse_on),
        ));
    }
    if let Some((state, style)) = pane_title_state_badge(report, flash) {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!(" {state} "), style));
    }
    if !spans.is_empty() {
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        pane_panel_title(report),
        pane_header_style(report, now, flash),
    ));
    // v1.39 Pending-action discoverability surface A: when this pane
    // carries a pending prompt-send proposal, append a `★p` chip so
    // operators can scan the panes list and spot actionable items
    // without selecting each one. Color uses the highest-severity
    // recommendation on the pane (so a Risk-tier proposal stands out
    // from a Concern-tier one) and falls back to TEXT_PRIMARY when
    // the pane has no recommendation severity.
    if crate::app::prompt_send_actions::first_prompt_send_proposal(report).is_some() {
        let chip_color = report
            .recommendations
            .iter()
            .map(|rec| rec.severity)
            .max()
            .map(theme::severity_color)
            .unwrap_or(theme::text_primary());
        spans.push(Span::styled(
            "  \u{2605}p",
            Style::default().fg(chip_color).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn pane_title_state_badge(
    report: &PaneReport,
    flash: Option<&PaneStateFlash>,
) -> Option<(&'static str, Style)> {
    match report.idle_state {
        Some(cause) => Some((idle_title_state_label(cause), idle_title_state_style(cause))),
        // uniform-vNext S3b: persistent `● ACTIVE` pill so every pane shows its
        // live state at a glance (mockup), not only on the state-change flash —
        // the transient flash is still surfaced by the separate STATE CHANGED
        // badge in pane_panel_title_line.
        None => {
            let _ = flash;
            Some(("● ACTIVE", active_state_badge_style()))
        }
    }
}

fn idle_title_state_label(cause: IdleCause) -> &'static str {
    // uniform-vNext S3b: lead each state with an at-a-glance glyph (mockup
    // status pills). `.contains()`-based tests keep passing (substring intact).
    match cause {
        IdleCause::WorkComplete => "✓ IDLE DONE",
        IdleCause::Stale => "◌ IDLE STALE",
        IdleCause::InputWait => "⌛ WAIT INPUT",
        IdleCause::PermissionWait => "⌛ WAIT APPROVAL",
        IdleCause::LimitHit => "⛔ USAGE LIMIT",
    }
}

fn idle_title_state_style(cause: IdleCause) -> Style {
    match cause {
        IdleCause::WorkComplete => theme::idle_work_complete(),
        IdleCause::Stale => theme::idle_stale(),
        IdleCause::InputWait => theme::idle_input_wait(),
        IdleCause::PermissionWait => theme::idle_permission_wait(),
        IdleCause::LimitHit => theme::idle_limit_hit(),
    }
}

pub fn panel_body(report: &PaneReport) -> Vec<ListItem<'static>> {
    panel_body_with_width(report, BADGE_WRAP_FALLBACK_WIDTH)
}

fn panel_body_with_width(report: &PaneReport, wrap_width: u16) -> Vec<ListItem<'static>> {
    pane_sectioned_rows(
        report,
        Instant::now(),
        None,
        wrap_width,
        PaneSectionOptions::panel_body(),
    )
    .into_iter()
    .map(|row| ListItem::new(row.line))
    .collect()
}

fn pane_badge_wrap_width(area: Rect) -> u16 {
    area.width.saturating_sub(4)
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

pub fn metric_row(s: &SignalSet, provider: Provider) -> String {
    let mut parts = Vec::new();
    if let Some(m) = s.context_pressure.as_ref() {
        let mut ctx_text = format!(
            "context {:.0}% [{}]",
            m.value * 100.0,
            source_kind_label(m.source_kind)
        );
        if let Some(window_size) = s.context_window_size.as_ref() {
            ctx_text.push_str(&format!(
                " of {}",
                format_count_with_suffix(window_size.value)
            ));
        }
        parts.push(ctx_text);
    }
    if let Some(m) = s.quota_pressure.as_ref() {
        parts.push(format!(
            "quota {:.0}% [{}]",
            m.value * 100.0,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.quota_5h_pressure.as_ref() {
        parts.push(format!(
            "quota 5h {:.0}% [{}]",
            m.value * 100.0,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.quota_5h_resets_at.as_ref()
        && let Some(eta) = format_resets_eta(m.value)
    {
        parts.push(format!(
            "5h resets in {} [{}]",
            eta,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.quota_weekly_pressure.as_ref() {
        parts.push(format!(
            "quota weekly {:.0}% [{}]",
            m.value * 100.0,
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.quota_weekly_resets_at.as_ref()
        && let Some(eta) = format_resets_eta(m.value)
    {
        parts.push(format!(
            "7d resets in {} [{}]",
            eta,
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
    // v1.58.0 provider-honesty: cost chip surfaces a `cost ? [pricing]`
    // placeholder when the pane has token activity but no cost — that's
    // a missing pricing.toml row the operator can curate, not a silent
    // None.
    if let Some(chip) = cost_chip_text(s, provider) {
        parts.push(chip);
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
    if let Some(m) = s.process_memory_mb.as_ref() {
        parts.push(format!(
            "memory {} [{}]",
            format_memory_mb(m.value),
            source_kind_label(m.source_kind)
        ));
    }
    if let Some(m) = s.agent_memory_bytes.as_ref() {
        parts.push(format!(
            "memory-file {} [{}]",
            format_agent_memory_bytes(m.value),
            source_kind_label(m.source_kind)
        ));
    }
    // v1.56.0 provider-honesty: cache_chip_text never silently omits.
    // Claude/Codex/Gemini: `cache N% [Source]` or `cache ?` while
    // waiting; Gemini switches to `cache —` only when model stats are
    // present but Cache Reads is absent for the current auth surface.
    // Qmonster/Unknown: chip omitted.
    if let Some(chip) = cache_chip_text(s, provider) {
        parts.push(chip);
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

/// Wrap an `aligned_field`-style row across multiple lines so the
/// pane card never silently truncates. Continuation rows are
/// indented under the value column (`label_col + 2` spaces).
///
/// `wrap_width` below `label_col + 4` returns a single uncut line —
/// the renderer can clip the rest. Avoids producing zero-character
/// continuation rows on degenerate viewports.
fn wrap_aligned_field(label: &str, value: &str, wrap_width: u16) -> Vec<Line<'static>> {
    let label_col = label.len().max(8);
    let prefix = format!("{label:<width$}: ", label = label, width = label_col);
    let cont_indent = " ".repeat(label_col + 2);
    let max_width = wrap_width as usize;
    let value_budget = max_width.saturating_sub(prefix.chars().count());

    if max_width < label_col + 4 || value_budget == 0 {
        return vec![Line::from(format!("{prefix}{value}"))];
    }

    let mut lines = Vec::new();
    let mut buf = String::new();
    let mut width_used = 0usize;
    let mut on_first = true;

    for ch in value.chars() {
        if width_used >= value_budget {
            lines.push(Line::from(if on_first {
                format!("{prefix}{buf}")
            } else {
                format!("{cont_indent}{buf}")
            }));
            buf.clear();
            width_used = 0;
            on_first = false;
        }
        buf.push(ch);
        width_used += 1;
    }

    if !buf.is_empty() {
        lines.push(Line::from(if on_first {
            format!("{prefix}{buf}")
        } else {
            format!("{cont_indent}{buf}")
        }));
    }

    if lines.is_empty() {
        lines.push(Line::from(prefix));
    }
    lines
}

/// Re-wrap a string that already has the `label_col + ": " + value`
/// shape produced by `expanded_detail_field`. Splits on the first
/// `: ` and delegates to `wrap_aligned_field`.
fn reflow_already_aligned(formatted: &str, wrap_width: u16) -> Vec<Line<'static>> {
    if let Some(idx) = formatted.find(": ") {
        let (label, rest) = formatted.split_at(idx);
        let value = &rest[2..];
        wrap_aligned_field(label.trim_end(), value, wrap_width)
    } else {
        vec![Line::from(formatted.to_string())]
    }
}

fn expanded_detail_field(raw: &str) -> String {
    if let Some(effect) = raw.strip_prefix("- ") {
        return aligned_field("effect", effect);
    }
    if raw.starts_with('[') {
        return aligned_field("lever", raw);
    }
    if let Some((label, value)) = raw.split_once(':') {
        let label = match label.trim() {
            l if l.starts_with("side_effects") => "effects",
            "" => "detail",
            l => l,
        };
        return aligned_field(label, value.trim_start());
    }
    aligned_field("detail", raw)
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
        ellipsize(path, 80)
    }
}

fn display_command(command: &str) -> String {
    if command.is_empty() {
        "unknown command".into()
    } else {
        // Return full cmdline; both call sites use `wrap_aligned_field`
        // which wraps at the active card width so long argv (e.g.
        // `node /home/u/.nvm/versions/node/v22/bin/codex --foo --bar …`)
        // never truncates on the right edge.
        command.to_string()
    }
}

/// Append a ` · wt of <parent>` suffix to a base `path` cell when the
/// pane's cwd resolves to a linked git worktree. Returns the base
/// value unchanged for Primary and None roles. The parent repo root
/// is run through the same `display_path` helper so its ellipsis
/// behaviour matches the primary cwd's.
fn path_row_value(report: &PaneReport) -> String {
    let base = display_path(&report.current_path);
    match report.worktree_role.as_ref() {
        Some(crate::app::worktree_info::WorktreeRole::Linked { parent_repo_root }) => {
            format!(
                "{base} · wt of {}",
                display_path(&parent_repo_root.display().to_string())
            )
        }
        _ => base,
    }
}

fn confidence_label(c: IdentityConfidence) -> &'static str {
    match c {
        IdentityConfidence::High => "high",
        IdentityConfidence::Medium => "medium",
        IdentityConfidence::Conflict => "conflict",
        IdentityConfidence::Low => "low",
        IdentityConfidence::Unknown => "unknown",
    }
}

fn provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Antigravity => "agy",
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

#[cfg(test)]
fn metric_badge_line(signals: &SignalSet, provider: Provider) -> Vec<Line<'static>> {
    metric_badge_lines(signals, provider, BADGE_WRAP_FALLBACK_WIDTH, false)
}

fn metric_badge_lines(
    signals: &SignalSet,
    provider: Provider,
    wrap_width: u16,
    bounded_as_gauges: bool,
) -> Vec<Line<'static>> {
    let mut rows = Vec::with_capacity(4);
    // uniform-vNext S3: in the expanded card the bounded pressure metrics
    // (CTX / quota / 5h / weekly) render as gauge bars above the badge row;
    // primary_metric_row then skips them as badges to avoid duplication.
    if bounded_as_gauges {
        rows.extend(gauge_bar_rows(signals, wrap_width));
    }
    if let Some(line) = primary_metric_row(signals, provider, bounded_as_gauges) {
        rows.extend(wrap_badge_line(line, wrap_width));
    }
    if let Some(line) = context_metric_row(signals) {
        rows.extend(wrap_badge_line(line, wrap_width));
    }
    rows
}

fn wrap_badge_line(line: Line<'static>, max_width: u16) -> Vec<Line<'static>> {
    let mut spans = line.spans.into_iter();
    let Some(prefix) = spans.next() else {
        return vec![Line::from("")];
    };
    let prefix_width = span_width(&prefix);
    let continuation_prefix = Span::raw(" ".repeat(prefix_width));
    let badges: Vec<Span<'static>> = spans.filter(|span| span.content.as_ref() != " ").collect();
    if badges.is_empty() {
        return vec![Line::from(vec![prefix])];
    }

    let max_width = usize::from(max_width);
    let mut rows = Vec::new();
    let mut current = vec![prefix.clone()];
    let mut used_width = prefix_width;
    let mut has_badge = false;

    for badge in badges {
        let badge_width = span_width(&badge);
        let separator_width = usize::from(has_badge);
        if has_badge
            && max_width > prefix_width
            && used_width + separator_width + badge_width > max_width
        {
            rows.push(Line::from(current));
            current = vec![continuation_prefix.clone()];
            used_width = prefix_width;
            has_badge = false;
        }
        if has_badge {
            current.push(Span::raw(" "));
            used_width += 1;
        }
        used_width += badge_width;
        current.push(badge);
        has_badge = true;
    }

    rows.push(Line::from(current));
    rows
}

fn span_width(span: &Span<'_>) -> usize {
    span.content.chars().count()
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

/// uniform-vNext S3 gauge bars. Total cell width of the bar track.
const GAUGE_WIDTH: usize = 16;
/// Left-eighth block glyphs (1/8 .. 8/8) for sub-cell horizontal fill.
const GAUGE_PARTIALS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// Split a 0..1 ratio into (filled, empty) gauge segments. Uses the
/// left-eighth glyphs so a 58% bar reads precisely, not rounded to whole cells.
fn gauge_filled_empty(value: f32, width: usize) -> (String, String) {
    let v = value.clamp(0.0, 1.0);
    let eighths = (v * (width as f32) * 8.0).round() as usize;
    let full = (eighths / 8).min(width);
    let mut filled = "█".repeat(full);
    let mut used = full;
    let rem = eighths % 8;
    if used < width && rem > 0 {
        filled.push(GAUGE_PARTIALS[rem - 1]);
        used += 1;
    }
    (filled, "░".repeat(width - used))
}

/// One gauge-bar line: `CTX  ████████▌░░░░░  58%  resets 2h13m`. Fill + value
/// are colored by the SAME `context_metric_severity` thresholds (60/75/85) the
/// badges use; the empty track is dim, the reset eta faint. DISPLAY-ONLY — agy
/// bars render here, but the engine choke-point + Now-strip guard keep agy off
/// every alert surface (uniform-vNext S2).
fn gauge_bar_line(
    label: &str,
    value: f32,
    trailing: Option<String>,
    max_width: u16,
) -> Line<'static> {
    let color = theme::severity_color(context_metric_severity(value));
    // F4 (diff-review): clamp the bar so `label + bar + " NN%" [+ "  trailing"]`
    // fits max_width on narrow splits — drop the trailing first, then shrink the
    // bar (min 4 cells). Label gutter = max(4, label) + 1 space; pct = " 100%" = 5.
    let label_w = label.chars().count().max(4) + 1;
    let pct_w = 5usize;
    let max = max_width as usize;
    let trail = trailing.map(|t| format!("  {t}"));
    let trail_w = trail.as_ref().map(|s| s.chars().count()).unwrap_or(0);
    let fits_trail = max >= label_w + pct_w + trail_w + 4;
    let avail = max.saturating_sub(label_w + pct_w + if fits_trail { trail_w } else { 0 });
    let bar_w = avail.clamp(4, GAUGE_WIDTH);
    let (filled, empty) = gauge_filled_empty(value, bar_w);
    let mut spans = vec![
        Span::styled(
            format!("{label:<4}"),
            Style::default().fg(theme::text_primary()),
        ),
        Span::raw(" "),
        Span::styled(filled, Style::default().fg(color)),
        Span::styled(empty, Style::default().fg(theme::text_dim())),
        Span::styled(
            format!(" {:>3.0}%", (value * 100.0).clamp(0.0, 100.0)),
            Style::default().fg(color),
        ),
    ];
    if fits_trail && let Some(t) = trail {
        spans.push(Span::styled(t, Style::default().fg(theme::text_dim())));
    }
    Line::from(spans)
}

/// Bounded-metric gauge bars for the expanded pane card: CTX, then either a
/// single QUOTA (Gemini) or split 5H + 7D (Claude/Codex/agy) with reset etas.
fn gauge_bar_rows(signals: &SignalSet, max_width: u16) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if let Some(m) = signals.context_pressure.as_ref() {
        // preserve the Slice-D window-size readout as the CTX bar's trailing text.
        let of_size = signals
            .context_window_size
            .as_ref()
            .map(|w| format!("of {}", format_count_with_suffix(w.value)));
        rows.push(gauge_bar_line("CTX", m.value, of_size, max_width));
    }
    if let Some(m) = signals.quota_pressure.as_ref() {
        rows.push(gauge_bar_line("QUOTA", m.value, None, max_width));
    }
    push_quota_gauge(
        &mut rows,
        "5H",
        signals.quota_5h_pressure.as_ref(),
        signals.quota_5h_resets_at.as_ref(),
        max_width,
    );
    push_quota_gauge(
        &mut rows,
        "7D",
        signals.quota_weekly_pressure.as_ref(),
        signals.quota_weekly_resets_at.as_ref(),
        max_width,
    );
    rows
}

/// One quota window's row: a gauge bar when pressure is known (reset eta in the
/// trailing), OR — F3 (diff-review) — a reset-only line when the reset is known
/// but pressure is absent (e.g. the Codex status line hides quota % while the
/// rollout still carries resets_at), so the reset ETA is never silently dropped
/// under gauge mode.
fn push_quota_gauge(
    rows: &mut Vec<Line<'static>>,
    label: &str,
    pressure: Option<&MetricValue<f32>>,
    resets_at: Option<&MetricValue<u64>>,
    max_width: u16,
) {
    let eta = resets_at.and_then(|r| format_resets_eta(r.value));
    match pressure {
        Some(m) => rows.push(gauge_bar_line(
            label,
            m.value,
            eta.map(|e| format!("resets {e}")),
            max_width,
        )),
        None => {
            if let Some(e) = eta {
                rows.push(Line::from(vec![
                    Span::styled(
                        format!("{label:<4}"),
                        Style::default().fg(theme::text_primary()),
                    ),
                    Span::styled(
                        format!("  resets {e}"),
                        Style::default().fg(theme::text_dim()),
                    ),
                ]));
            }
        }
    }
}

fn primary_metric_row(
    signals: &SignalSet,
    provider: Provider,
    skip_bounded: bool,
) -> Option<Line<'static>> {
    let mut spans = vec![Span::raw(format!("{:<8}: ", "metrics"))];
    let mut has_any = false;

    // uniform-vNext S3: when the expanded card shows the bounded metrics
    // (CTX / quota / 5h / weekly + reset etas) as gauge BARS above this row,
    // skip them here so they are not duplicated as badges. Collapsed/fallback
    // contexts pass skip_bounded=false and keep the compact badges.
    if !skip_bounded {
        if let Some(metric) = signals.context_pressure.as_ref() {
            let mut ctx_text = format!(" CTX {:.0}% ", metric.value * 100.0);
            if let Some(window_size) = signals.context_window_size.as_ref() {
                ctx_text.push_str(&format!(
                    "of {} ",
                    format_count_with_suffix(window_size.value)
                ));
            }
            push_badge(
                &mut spans,
                &mut has_any,
                ctx_text,
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
        if let Some(metric) = signals.quota_5h_pressure.as_ref() {
            push_badge(
                &mut spans,
                &mut has_any,
                format!(" QUOTA 5H {:.0}% ", metric.value * 100.0),
                theme::severity_badge_style(context_metric_severity(metric.value)),
            );
        }
        if let Some(metric) = signals.quota_5h_resets_at.as_ref()
            && let Some(eta) = format_resets_eta(metric.value)
        {
            push_badge(
                &mut spans,
                &mut has_any,
                format!(
                    " RESET 5H {} [{}] ",
                    eta,
                    source_kind_label(metric.source_kind)
                ),
                Style::default().fg(theme::text_primary()),
            );
        }
        if let Some(metric) = signals.quota_weekly_pressure.as_ref() {
            push_badge(
                &mut spans,
                &mut has_any,
                format!(" QUOTA WEEK {:.0}% ", metric.value * 100.0),
                theme::severity_badge_style(context_metric_severity(metric.value)),
            );
        }
        if let Some(metric) = signals.quota_weekly_resets_at.as_ref()
            && let Some(eta) = format_resets_eta(metric.value)
        {
            push_badge(
                &mut spans,
                &mut has_any,
                format!(
                    " RESET 7D {} [{}] ",
                    eta,
                    source_kind_label(metric.source_kind)
                ),
                Style::default().fg(theme::text_primary()),
            );
        }
    }
    if let Some(fact) = first_model_reset_fact(signals) {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " RESET {} [{}] ",
                ellipsize(&fact.value, 48),
                source_kind_label(fact.source_kind)
            ),
            Style::default().fg(theme::text_primary()),
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
    // v1.58.0 provider-honesty: COST badge mirrors the cache chip's
    // four-state behaviour. Value branch keeps the existing
    // severity-coloured badge; Pending shows ` COST ? [pricing] ` so
    // operators see the missing pricing.toml row instead of nothing.
    match crate::ui::provider_honesty::cost_metric_status(signals, provider) {
        crate::ui::provider_honesty::CostMetricStatus::Value { usd, source_kind } => {
            push_badge(
                &mut spans,
                &mut has_any,
                format!(" COST ${usd:.2} [{}] ", source_kind_label(source_kind)),
                theme::severity_badge_style(cost_metric_severity(usd)),
            );
        }
        crate::ui::provider_honesty::CostMetricStatus::Pending => {
            push_badge(
                &mut spans,
                &mut has_any,
                " COST ? [pricing] ".to_string(),
                theme::label_style(),
            );
        }
        crate::ui::provider_honesty::CostMetricStatus::Hidden => {}
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
    // Phase E E1 (v1.21.0) + Phase F F-1 (v1.22.0): process memory
    // badge. Gemini sources from its status-table column [Official];
    // Claude/Codex source from /proc descendant RSS [Heur].
    // `process_memory_mb` is canonicalized to MiB at parse time;
    // adapt the unit at render time so values >= 1024 MiB render
    // as `GB` for readability.
    if let Some(metric) = signals.process_memory_mb.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " MEM {} [{}] ",
                format_memory_mb(metric.value),
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        );
    }
    // Phase F F-2 (v1.23.0): agent memory file bytes — sums
    // CLAUDE.md / AGENTS.md / GEMINI.md plus per-provider home
    // counterparts. Always Heuristic because file existence is not
    // proof the CLI loaded the bytes — operator decides if the
    // total is too high (advisory threshold: 50_000 bytes (~49 KiB)).
    if let Some(metric) = signals.agent_memory_bytes.as_ref() {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(
                " MEM-FILE {} [{}] ",
                format_agent_memory_bytes(metric.value),
                source_kind_label(metric.source_kind)
            ),
            theme::label_style(),
        );
    }
    // v1.56.0 provider-honesty: emit honest CACHE chip per provider.
    // Value when observed, `?` while waiting for an optional provider
    // surface, and `—` when the current provider/auth surface proves it
    // cannot supply cache reuse.
    if let Some(label) = cache_badge_text(signals, provider) {
        push_badge(&mut spans, &mut has_any, label, theme::label_style());
    }
    if let Some(policy) = policy_mini_strip(signals, provider) {
        push_badge(
            &mut spans,
            &mut has_any,
            format!(" {policy} "),
            theme::label_style(),
        );
    }

    has_any.then(|| Line::from(spans))
}

fn policy_mini_strip(signals: &SignalSet, provider: Provider) -> Option<String> {
    if !matches!(
        provider,
        Provider::Claude | Provider::Codex | Provider::Gemini
    ) {
        return None;
    }
    let approval = signals
        .runtime_facts
        .iter()
        .find(|fact| {
            matches!(
                fact.kind,
                RuntimeFactKind::PermissionMode | RuntimeFactKind::AutoMode
            )
        })
        .and_then(|fact| normalize_approval_mode(&fact.value));
    let sandbox = signals
        .runtime_facts
        .iter()
        .find(|fact| fact.kind == RuntimeFactKind::Sandbox)
        .and_then(|fact| normalize_sandbox_state(&fact.value));
    match (approval, sandbox) {
        (None, None) => Some("POLICY ?".into()),
        (approval, sandbox) => Some(format!(
            "POLICY approval={} sandbox={} [Heur]",
            approval.unwrap_or("?"),
            sandbox.unwrap_or("?")
        )),
    }
}

fn normalize_approval_mode(value: &str) -> Option<&'static str> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("once") {
        Some("once")
    } else if lower.contains("auto") || lower.contains("yolo") || lower.contains("bypass") {
        Some("auto")
    } else if lower.contains("manual") || lower.contains("ask") || lower.contains("approve") {
        Some("manual")
    } else {
        None
    }
}

fn normalize_sandbox_state(value: &str) -> Option<&'static str> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("off")
        || lower.contains("none")
        || lower.contains("disable")
        || lower.contains("danger")
        || lower.contains("full")
    {
        Some("off")
    } else if lower.contains("on")
        || lower.contains("workspace")
        || lower.contains("read")
        || lower.contains("restrict")
        || lower.contains("sandbox")
    {
        Some("on")
    } else {
        None
    }
}

fn first_model_reset_fact(signals: &SignalSet) -> Option<&RuntimeFact> {
    signals
        .runtime_facts
        .iter()
        .find(|fact| fact.kind == RuntimeFactKind::ModelReset)
}

/// Phase F F-5b (v1.31.0): format a Unix timestamp `resets_at`
/// (seconds since epoch) as a relative countdown like `4d 6h`,
/// `2h13m`, `45m`, or `30s`. Returns `None` when the timestamp is
/// in the past or in the impossibly distant future (sentinel-style
/// values that would render absurdly long countdowns). The countdown
/// ticks once per poll cycle since the rendered text is rebuilt
/// from the SignalSet on every frame.
///
/// v1.38 polish: etas >= 24h render as `<d>d <h>h` (e.g. `4d 6h`)
/// rather than `102h00m`, which operators reported as awkward.
pub(crate) fn format_resets_eta(resets_at_unix_seconds: u64) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if resets_at_unix_seconds <= now {
        return None;
    }
    let remaining = resets_at_unix_seconds - now;
    // Cap at 14 days — beyond that, the timestamp is almost certainly
    // a sentinel from a misconfigured surface, not a real ETA.
    if remaining > 14 * 24 * 3600 {
        return None;
    }
    if remaining >= 24 * 3600 {
        let days = remaining / (24 * 3600);
        let remaining_hours = (remaining % (24 * 3600)) / 3600;
        return Some(format!("{days}d {remaining_hours}h"));
    }
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;
    if hours >= 1 {
        Some(format!("{hours}h{minutes:02}m"))
    } else if minutes >= 1 {
        Some(format!("{minutes}m"))
    } else {
        Some(format!("{seconds}s"))
    }
}

/// Format a `process_memory_mb` value (always MiB) for the operator
/// badge. Switches to GiB at 1024 MiB so 1.2 GB renders cleanly
/// instead of 1228.8 MB.
fn format_memory_mb(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{:.1} MB", mb)
    }
}

/// Format an `agent_memory_bytes` value for the operator badge.
/// Uses `KB` (1 KiB = 1024 bytes) below 1 MiB and `MB` (one decimal)
/// at or above. Mirrors `format_memory_mb`'s MiB→GB transition style
/// for visual consistency between the two memory-related badges.
pub(crate) fn format_agent_memory_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if bytes >= MIB {
        format!("{:.1} MB", (bytes as f64) / (MIB as f64))
    } else if bytes < KIB {
        // Sub-1 KiB renders as "<1 KB" rather than "0 KB" so
        // operators can distinguish "tiny non-zero file present" from
        // "no files found" (the latter omits the badge entirely).
        "<1 KB".to_string()
    } else {
        format!("{} KB", bytes / KIB)
    }
}

/// Phase F F-4 (v1.25.0): compute `cache_hit_ratio = cached /
/// (input + cached)` and format as a percent string with one
/// decimal. Returns None when both inputs are None or both zero
/// (avoid divide-by-zero / nonsense ratios). The numerator is
/// strictly cached_input_tokens; the denominator is input_tokens
/// (which is non-cached) PLUS cached_input_tokens — Codex's welcome
/// panel reports them as siblings in `Token usage: total=N
/// input=N (+ N cached) output=N`, so the ratio answers "what
/// fraction of the prompt was reused from cache?".
#[cfg(test)]
fn format_cache_hit_ratio(
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
) -> Option<String> {
    provider_honesty::cache_hit_ratio_from_counts(input_tokens, cached_input_tokens)
        .map(provider_honesty::format_ratio_pct)
}

fn cache_badge_text(s: &SignalSet, provider: Provider) -> Option<String> {
    match provider_honesty::cache_metric_status(s, provider) {
        CacheMetricStatus::Value { ratio, source_kind } => Some(format!(
            " CACHE {} [{}] ",
            provider_honesty::format_ratio_pct(ratio),
            source_kind_label(source_kind)
        )),
        CacheMetricStatus::Pending => Some(" CACHE ? ".to_string()),
        CacheMetricStatus::Unsupported => Some(" CACHE \u{2014} ".to_string()),
        CacheMetricStatus::Hidden => None,
    }
}

/// v1.58.0 provider-honesty: text variant of the COST chip for the
/// `--once` text report and `metric_row`. Mirrors the badge variant in
/// `primary_metric_row`. Returns:
/// - `cost $X.XX [Source]` when known
/// - `cost ? [pricing]` while waiting on a pricing.toml row
/// - `None` for Qmonster / Unknown / pre-token-activity panes
pub(crate) fn cost_chip_text(s: &SignalSet, provider: Provider) -> Option<String> {
    match crate::ui::provider_honesty::cost_metric_status(s, provider) {
        crate::ui::provider_honesty::CostMetricStatus::Value { usd, source_kind } => Some(format!(
            "cost ${usd:.2} [{}]",
            source_kind_label(source_kind)
        )),
        crate::ui::provider_honesty::CostMetricStatus::Pending => {
            Some("cost ? [pricing]".to_string())
        }
        crate::ui::provider_honesty::CostMetricStatus::Hidden => None,
    }
}

/// v1.56.0 provider-honesty: produce the cache chip text for a pane
/// card honestly per-provider. Pre-v1.56.0 the cache chip was silently
/// omitted whenever cache data was absent, making it impossible to tell
/// "waiting for data" from "this surface cannot provide it".
pub(crate) fn cache_chip_text(s: &SignalSet, provider: Provider) -> Option<String> {
    match provider_honesty::cache_metric_status(s, provider) {
        CacheMetricStatus::Value { ratio, source_kind } => Some(format!(
            "cache {} [{}]",
            provider_honesty::format_ratio_pct(ratio),
            source_kind_label(source_kind)
        )),
        CacheMetricStatus::Pending => Some("cache ?".to_string()),
        CacheMetricStatus::Unsupported => Some("cache \u{2014}".to_string()),
        CacheMetricStatus::Hidden => None,
    }
}

/// Phase F F-3 (v1.24.0): render a TOKENS sparkline from recent prompt
/// token samples (`input_tokens + cached_input_tokens`). Returns `None`
/// when there are fewer than 2 samples (no deltas computable). Samples
/// usually arrive newest-first from `SqliteTokenUsageSink::recent_samples`;
/// this function sorts them by timestamp so the sparkline reads left-to-right
/// oldest-to-newest.
///
/// The sparkline shows DELTA between adjacent prompt-token samples
/// (rate of context growth), not just the raw provider total. Idle pane
/// → flat line; active pane → rising blocks. Missing token fields are
/// treated as 0 for delta computation. Negative deltas (provider counter
/// reset) saturate to 0 via `saturating_sub`.
#[cfg(test)]
fn render_token_sparkline(
    samples: &[crate::store::TokenSample],
) -> Option<ratatui::text::Line<'static>> {
    token_sparkline_body(samples).map(|body| ratatui::text::Line::from(format!("TOKENS {body}")))
}

fn token_sparkline_status_line(
    samples: &[crate::store::TokenSample],
    signals: &SignalSet,
) -> Line<'static> {
    let mut spans = vec![Span::raw(format!("{:<8}: ", "tokens"))];
    if let Some(stats) = token_sparkline_stats(samples) {
        spans.push(Span::styled(
            token_sparkline_label(signals),
            Style::default()
                .fg(theme::text_primary())
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(
                "prompt {} · +{}/{}p ",
                format_count_with_suffix(stats.latest_prompt_tokens),
                format_count_with_suffix(stats.delta_prompt_tokens),
                stats.sample_count
            ),
            Style::default().fg(theme::text_primary()),
        ));
        spans.push(Span::styled(
            stats.body,
            Style::default()
                .fg(theme::text_primary())
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        let mut text = format!(
            "{}collecting {}/2 ",
            token_sparkline_label(signals),
            samples.len().min(2)
        );
        if let Some(n) = latest_sampled_prompt_tokens(samples) {
            text.push_str(&format!("· prompt {} ", format_count_with_suffix(n)));
        }
        spans.push(Span::styled(
            text,
            Style::default().fg(theme::text_primary()),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
fn token_sparkline_body(samples: &[crate::store::TokenSample]) -> Option<String> {
    token_sparkline_stats(samples).map(|stats| stats.body)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenSparklineStats {
    body: String,
    latest_prompt_tokens: u64,
    delta_prompt_tokens: u64,
    sample_count: usize,
}

fn token_sparkline_stats(samples: &[crate::store::TokenSample]) -> Option<TokenSparklineStats> {
    if samples.len() < 2 {
        return None;
    }
    let chrono = prompt_token_series_oldest_first(samples);
    // Compute deltas between adjacent samples.
    let deltas: Vec<u64> = chrono
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .collect();
    if deltas.is_empty() {
        return None;
    }
    let max = *deltas.iter().max().unwrap_or(&0);
    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let body: String = if max == 0 {
        // All deltas are 0 — render the lowest block for each.
        deltas.iter().map(|_| blocks[0]).collect()
    } else {
        deltas
            .iter()
            .map(|d| {
                let idx = ((*d as f64 / max as f64) * (blocks.len() - 1) as f64).round() as usize;
                blocks[idx.min(blocks.len() - 1)]
            })
            .collect()
    };
    let first = *chrono.first().unwrap_or(&0);
    let latest = *chrono.last().unwrap_or(&0);
    Some(TokenSparklineStats {
        body,
        latest_prompt_tokens: latest,
        delta_prompt_tokens: latest.saturating_sub(first),
        sample_count: chrono.len(),
    })
}

fn prompt_token_series_oldest_first(samples: &[crate::store::TokenSample]) -> Vec<u64> {
    let mut sorted: Vec<&crate::store::TokenSample> = samples.iter().collect();
    sorted.sort_by_key(|sample| sample.ts_unix_ms);
    sorted.into_iter().map(sample_prompt_tokens).collect()
}

fn latest_sampled_prompt_tokens(samples: &[crate::store::TokenSample]) -> Option<u64> {
    samples
        .iter()
        .max_by_key(|sample| sample.ts_unix_ms)
        .map(sample_prompt_tokens)
}

fn sample_prompt_tokens(sample: &crate::store::TokenSample) -> u64 {
    sample
        .input_tokens
        .unwrap_or(0)
        .saturating_add(sample.cached_input_tokens.unwrap_or(0))
}

fn token_sparkline_label(signals: &SignalSet) -> String {
    if let Some(metric) = signals.token_count.as_ref() {
        format!(" TOKENS {} · ", format_count_with_suffix(metric.value))
    } else {
        " TOKENS ".to_string()
    }
}

#[cfg(not(test))]
fn token_rows_supported(provider: Provider) -> bool {
    matches!(
        provider,
        Provider::Claude | Provider::Codex | Provider::Gemini
    )
}

/// Same logic as the private `fn token_rows_supported`; promoted to
/// `pub(crate)` under `#[cfg(test)]` so that the ObserveOnly six-gate
/// regression in `adapters` can call
/// `crate::ui::panels::token_rows_supported(Provider::Antigravity)` without
/// changing production visibility. The body is identical — this is a
/// guard-rail assertion path only.
#[cfg(test)]
pub(crate) fn token_rows_supported(provider: Provider) -> bool {
    matches!(
        provider,
        Provider::Claude | Provider::Codex | Provider::Gemini
    )
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

fn token_io_line(report: &PaneReport) -> Option<String> {
    token_breakdown_line_with_label(report, "token io")
}

fn cache_token_io_line(report: &PaneReport) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(metric) = report.signals.cached_input_tokens.as_ref() {
        parts.push(format!(
            "read {} [{}]",
            format_count_with_suffix(metric.value),
            source_kind_label(metric.source_kind)
        ));
    }
    if let Some(metric) = report.signals.cache_creation_input_tokens.as_ref() {
        parts.push(format!(
            "create {} [{}]",
            format_count_with_suffix(metric.value),
            source_kind_label(metric.source_kind)
        ));
    }
    (!parts.is_empty()).then(|| aligned_field("cache io", &parts.join(" / ")))
}

fn token_breakdown_line(report: &PaneReport) -> Option<String> {
    token_breakdown_line_with_label(report, "tokens")
}

fn token_breakdown_line_with_label(report: &PaneReport, label: &'static str) -> Option<String> {
    let input = report.signals.input_tokens.as_ref()?;
    let output = report.signals.output_tokens.as_ref()?;
    let source = if input.source_kind == output.source_kind {
        format!("[{}]", source_kind_label(input.source_kind))
    } else {
        format!(
            "[in {} / out {}]",
            source_kind_label(input.source_kind),
            source_kind_label(output.source_kind)
        )
    };
    Some(aligned_field(
        label,
        &format!(
            "{} {} in / {} out {}",
            token_owner_label(report.identity.identity.role),
            format_count_with_suffix(input.value),
            format_count_with_suffix(output.value),
            source
        ),
    ))
}

fn token_owner_label(role: Role) -> &'static str {
    match role {
        Role::Main => "Main",
        Role::Review => "Review",
        Role::Research => "Research",
        Role::Monitor => "Monitor",
        Role::Unknown => "Pane",
    }
}

#[cfg(test)]
fn runtime_badge_lines(signals: &SignalSet) -> Vec<Line<'static>> {
    runtime_badge_lines_wrapped(signals, BADGE_WRAP_FALLBACK_WIDTH)
}

fn runtime_badge_lines_wrapped(signals: &SignalSet, wrap_width: u16) -> Vec<Line<'static>> {
    runtime_text_groups(signals)
        .into_iter()
        .flat_map(|(label, facts)| {
            let mut spans = vec![Span::raw(format!("{label:<8}: "))];
            for (idx, fact) in facts.into_iter().enumerate() {
                if idx > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(format!(" {fact} "), theme::label_style()));
            }
            wrap_badge_line(Line::from(spans), wrap_width)
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
        ("limits", &[RuntimeFactKind::ModelReset]),
        (
            "session",
            &[
                RuntimeFactKind::SessionId,
                RuntimeFactKind::ToolCalls,
                RuntimeFactKind::TranscriptPath,
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
    let value = if matches!(fact.kind, RuntimeFactKind::TranscriptPath) {
        display_path(&fact.value)
    } else {
        fact.value.clone()
    };
    format!(
        "{} {} [{}]",
        runtime_fact_label(fact.kind),
        value,
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
        // Phase F F-5b (v1.31.0): per-pane Claude session identity
        // surfaced from sidefile JSON.
        RuntimeFactKind::SessionId => "SID",
        RuntimeFactKind::ToolCalls => "CALLS",
        RuntimeFactKind::ModelReset => "RESET",
        RuntimeFactKind::TranscriptPath => "XSCRIPT",
        RuntimeFactKind::CliVersion => "CLI",
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
            format!(" {} ", idle_persistent_effect_label(cause)),
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

fn idle_persistent_effect_label(cause: IdleCause) -> &'static str {
    match cause {
        IdleCause::WorkComplete => "COMPLETE",
        IdleCause::Stale => "STILL IDLE",
        IdleCause::InputWait => "INPUT NEEDED",
        IdleCause::PermissionWait => "APPROVAL NEEDED",
        IdleCause::LimitHit => "ACTION REQUIRED",
    }
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
/// Used by the pane card and directly in tests. Delegates to the
/// `_with_flash` variant with `BADGE_WRAP_FALLBACK_WIDTH` so existing
/// no-flash callers don't need to plumb a `wrap_width` parameter.
#[cfg(test)]
fn render_pane_state_row(report: &PaneReport) -> Vec<Line<'static>> {
    render_pane_state_row_with_flash(report, Instant::now(), None, BADGE_WRAP_FALLBACK_WIDTH)
}

fn render_pane_state_row_with_flash(
    report: &PaneReport,
    now: Instant,
    flash: Option<&PaneStateFlash>,
    wrap_width: u16,
) -> Vec<Line<'static>> {
    // State row composes styled badge spans (idle glyph + label badge +
    // persistent marker + elapsed clock), so wrap via `wrap_badge_line`
    // (the same span-aware wrapper used by metric_badge_lines and
    // signal_badge_lines), not `wrap_aligned_field` (plain-text only).
    // Spec §5.1.
    let flash = matching_state_flash(report, now, flash);
    if let (Some(cause), Some(entered_at)) = (report.idle_state, report.idle_state_entered_at) {
        wrap_badge_line(
            format_state_row_with_flash(cause, entered_at, now, flash),
            wrap_width,
        )
    } else if report.idle_state.is_none() {
        flash
            .map(|flash| wrap_badge_line(format_active_state_row(now, flash), wrap_width))
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
        Style::default().fg(theme::text_dim())
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

fn blocking_signal_lines(signals: &SignalSet, wrap_width: u16) -> Vec<Line<'static>> {
    blocking_signal_line(signals)
        .map(|line| wrap_badge_line(line, wrap_width))
        .unwrap_or_default()
}

fn signal_badge_lines(
    label: &str,
    chips: Vec<&'static str>,
    wrap_width: u16,
) -> Vec<Line<'static>> {
    signal_badge_line(label, chips)
        .map(|line| wrap_badge_line(line, wrap_width))
        .unwrap_or_default()
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
mod tests;
