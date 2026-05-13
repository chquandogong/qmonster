use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{CrossPaneFinding, CrossPaneKind, Recommendation, Severity};
use crate::ui::labels::source_kind_label;
use crate::ui::theme;

/// Codex v1.7.3 (phase3b-strong-rec cleanup): single source of truth for
/// the strong-recommendation render format used by both the TUI alert
/// queue and the `--once` stdout path. Emits `next: …` before `run: …`
/// so the snapshot precondition always precedes the executable command;
/// omits either segment cleanly when its field is `None`.
pub fn format_strong_rec_body(rec: &Recommendation, pane_id: &str) -> String {
    let letter = rec.severity.letter();
    let badge = source_kind_label(rec.source_kind);
    let prefix = format!(
        "[{letter}] [{badge}] >> CHECKPOINT ({pane_id}): {}",
        rec.reason
    );
    append_actionable_tail(prefix, rec)
}

/// Shared renderer for non-strong recommendations so the alert queue
/// and `--once` both surface `next:` / `run:` when present.
pub fn format_recommendation_body(rec: &Recommendation, pane_id: &str) -> String {
    let severity = severity_word(rec.severity);
    let badge = source_kind_label(rec.source_kind);
    let prefix = format!("{severity} [{badge}] {pane_id} — {}", rec.reason);
    append_actionable_tail(prefix, rec)
}

pub fn format_recommendation_tail(rec: &Recommendation) -> Option<String> {
    actionable_tail(rec.next_step.as_deref(), rec.suggested_command.as_deref())
}

fn append_actionable_tail(prefix: String, rec: &Recommendation) -> String {
    match format_recommendation_tail(rec) {
        Some(tail) => format!("{prefix} — {tail}"),
        None => prefix,
    }
}

fn actionable_tail(next_step: Option<&str>, suggested_command: Option<&str>) -> Option<String> {
    let suggested_command = non_empty_command(suggested_command);
    match (
        next_step.map(str::trim).filter(|s| !s.is_empty()),
        suggested_command.as_deref(),
    ) {
        (None, None) => None,
        (None, Some(cmd)) => Some(format!("run: `{cmd}`")),
        (Some(step), None) => Some(format!("next: {step}")),
        (Some(step), Some(cmd)) => Some(format!("next: {step} — run: `{cmd}`")),
    }
}

/// Phase 5 P5-2 (v1.9.2): shared renderer for pending prompt-send
/// proposals, used by both the TUI alert queue and the `--once`
/// stdout path. Emits `[proposal] [Qmonster] <pane_id> — send
/// <slash_command> — <hint>` where the hint narrates the operator
/// keys. Setting `accept_gated = false` (e.g. `ObserveOnly` mode —
/// `EffectRunner::permit(&PromptSendProposed { .. })` returned false)
/// collapses the hint to `[d] dismiss only (send disabled)`; the
/// dismiss key stays available so the operator can still log an
/// explicit rejection to the audit trail from any mode.
pub fn format_prompt_send_proposal(
    target_pane_id: &str,
    slash_command: &str,
    accept_gated: bool,
) -> String {
    let hint = if accept_gated {
        "[p] accept / [d] dismiss"
    } else {
        "[d] dismiss only (send disabled)"
    };
    format!("[proposal] [Qmonster] {target_pane_id} — send `{slash_command}` — {hint}")
}

const DETAIL_LABEL_WIDTH: usize = 8;
pub const ALERT_AUTO_HIDE_DELAY: Duration = Duration::from_secs(20);
const BULK_HIDE_PREFIX: &str = "bulk hide : ";

pub struct AlertView<'a> {
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub now: Instant,
    pub target_label: &'a str,
    pub focused: bool,
    /// v1.59.0: optional case-insensitive substring filter applied
    /// against `title + headline + details + suggested_command` for
    /// each AlertItem. `None` = no filter; `Some("")` = filter input
    /// mode active but buffer empty (renders everything but shows the
    /// `/` input prompt at the top).
    pub filter: Option<&'a str>,
}

pub struct AlertMouseHit {
    pub index: usize,
    pub dismiss: bool,
}

/// Top-of-screen alert queue. Sorts alerts by operational priority so
/// the first row answers "what should I look at first?" instead of
/// replaying discovery order.
pub fn render_alerts(area: Rect, buf: &mut Buffer, state: &mut ListState, view: AlertView<'_>) {
    let mut entries = collect_entries(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    // v1.59.0: apply optional case-insensitive substring filter against
    // title + headline + details + suggested_command. Skip when buffer
    // is empty (filter input just opened, operator hasn't typed yet).
    let needle = view.filter.map(|s| s.trim().to_ascii_lowercase());
    if let Some(n) = needle.as_deref()
        && !n.is_empty()
    {
        entries.retain(|entry| alert_entry_matches_filter(entry, n));
    }
    let new_count = entries.iter().filter(|entry| entry.is_new()).count();
    let pending_hide_count = entries
        .iter()
        .filter(|entry| entry.hide_deadline().is_some())
        .count();
    // v1.59.0: surface the active filter in the panel title so the
    // operator can tell at a glance why the visible count is small.
    let filter_chip = match view.filter {
        Some(buf) if !buf.trim().is_empty() => format!(" · filter:{}", buf.trim()),
        Some(_) => " · filter:(empty)".to_string(),
        None => String::new(),
    };
    let title = format!(
        "Alerts · target {} || visible:{} · new:{} · auto-hide:{}{filter_chip}",
        view.target_label,
        entries.len(),
        new_count,
        pending_hide_count,
    );
    let block = block(&title, view.focused);
    let inner = block.inner(area);
    block.render(area, buf);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    let bulk_area = layout[0];
    let list_area = layout[1];
    Paragraph::new(bulk_hide_line(&entries))
        .style(Style::default().fg(theme::text_dim()))
        .render(bulk_area, buf);
    if entries.is_empty() {
        Paragraph::new("no alerts")
            .style(Style::default().fg(theme::text_dim()))
            .render(list_area, buf);
    } else if list_area.height > 0 {
        sync_list_selection(state, entries.len());
        let item_width = list_width(inner.width);
        let selected = state.selected();
        let alert_items: Vec<ListItem<'static>> = entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                alert_entry_list_item(entry, item_width, view.now, Some(idx) == selected)
            })
            .collect();
        StatefulWidget::render(
            List::new(alert_items)
                .highlight_style(highlight_style(view.focused))
                .highlight_symbol("▶ "),
            list_area,
            buf,
            state,
        );
    }
}

pub fn bulk_hide_severity_at_column(view: AlertView<'_>, column: u16) -> Option<Severity> {
    let entries = collect_entries(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    entry_severity_chips(&entries)
        .into_iter()
        .find(|chip| column >= chip.start_col && column < chip.end_col)
        .map(|chip| chip.severity)
}

pub fn alert_index_at_row(
    state: &ListState,
    view: AlertView<'_>,
    width: usize,
    row: u16,
) -> Option<usize> {
    alert_hit_at_row(state, view, width, row).map(|hit| hit.index)
}

pub fn alert_hit_at_row(
    state: &ListState,
    view: AlertView<'_>,
    width: usize,
    row: u16,
) -> Option<AlertMouseHit> {
    let entries = collect_entries(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    let mut remaining = row;
    let selected = state.selected();
    for (idx, entry) in entries.iter().enumerate().skip(state.offset()) {
        let dismiss_height = entry_dismiss_line_count(entry, width, view.now) as u16;
        let height = alert_entry_lines(entry, width, view.now, Some(idx) == selected).len() as u16;
        if remaining < height {
            return Some(AlertMouseHit {
                index: idx,
                dismiss: remaining > 0 && remaining < 1 + dismiss_height,
            });
        }
        remaining = remaining.saturating_sub(height);
    }
    None
}

pub fn alert_help_topic_at_row(
    state: &ListState,
    view: AlertView<'_>,
    width: usize,
    row: u16,
) -> Option<crate::ui::help_glossary::HelpTopic> {
    use crate::ui::help_glossary::HelpTopic;

    let entries = collect_entries(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    let mut remaining = row;
    let selected = state.selected();
    for (idx, entry) in entries.iter().enumerate().skip(state.offset()) {
        let is_selected = Some(idx) == selected;
        let dismiss_height = entry_dismiss_line_count(entry, width, view.now) as u16;
        let height = alert_entry_lines(entry, width, view.now, is_selected).len() as u16;
        if remaining < height {
            if remaining == 0 {
                return Some(HelpTopic::AlertHeader);
            }
            if remaining < 1 + dismiss_height {
                return Some(HelpTopic::AlertDismiss);
            }
            if is_alert_summary_row(entry, width, remaining, dismiss_height) {
                return Some(HelpTopic::AlertSummary);
            }
            if is_selected && is_alert_copy_row(entry, width, remaining, height) {
                return Some(HelpTopic::AlertCopy);
            }
            if remaining + 1 >= height {
                return None;
            }
            return Some(HelpTopic::AlertDetail);
        }
        remaining = remaining.saturating_sub(height);
    }
    None
}

fn is_alert_summary_row(
    entry: &AlertEntry,
    width: usize,
    remaining: u16,
    dismiss_height: u16,
) -> bool {
    let summary_height = entry_summary_line_count(entry, width) as u16;
    let start = 1 + dismiss_height;
    remaining >= start && remaining < start.saturating_add(summary_height)
}

fn is_alert_copy_row(entry: &AlertEntry, width: usize, remaining: u16, height: u16) -> bool {
    let copy_height = entry_copy_line_count(entry, width) as u16;
    if copy_height == 0 {
        return false;
    }
    let start = height.saturating_sub(1).saturating_sub(copy_height);
    remaining >= start && remaining < height.saturating_sub(1)
}

pub fn alert_count(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> usize {
    collect_entries(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    )
    .len()
}

pub fn visible_alert_keys(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<String> {
    collect_entries(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    )
    .into_iter()
    .map(|entry| entry.key().to_string())
    .collect()
}

pub fn selected_alert_suggested_command(
    state: &ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Option<String> {
    let idx = state.selected()?;
    collect_entries(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    )
    .get(idx)
    .and_then(|entry| entry.suggested_command().map(str::to_string))
}

/// v1.38 Phase D Task 20: extended companion to
/// `selected_alert_suggested_command` that returns the metadata the
/// Action Explainer modal needs to describe a `y`-copy action — alert
/// title, the suggested command, severity, and source kind. Returns
/// `None` if no alert is selected or the selected alert has no
/// runnable command.
pub fn selected_alert_suggested_command_meta(
    state: &ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Option<(String, String, Severity, SourceKind)> {
    let idx = state.selected()?;
    let entries = collect_entries(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    );
    let entry = entries.get(idx)?;
    let cmd = entry.suggested_command()?.to_string();
    Some((
        entry.title(),
        cmd,
        entry.severity(),
        entry.command_source_kind(),
    ))
}

/// v2.2.0 (P1-3): companion to `selected_alert_suggested_command_meta`
/// that extracts the (pane_id, action) pair from a Recommendation-class
/// alert. Returns `None` for non-Recommendation alerts (system notices,
/// cross-pane findings) — those don't write to the
/// `recommendation_events` ledger today.
///
/// Key format from `recommendation_key`: `rec|{pane_id}|{action}|{sev}|{reason}`
pub fn selected_alert_recommendation_identity(
    state: &ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Option<(String, String)> {
    let idx = state.selected()?;
    let entries = collect_entries(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    );
    entries
        .get(idx)
        .and_then(AlertEntry::recommendation_identity)
}

/// Parses the `rec|{pane_id}|{action}|{sev}|{reason}` key shape into
/// (pane_id, action). Returns None for non-recommendation key prefixes.
fn parse_recommendation_key(key: &str) -> Option<(String, String)> {
    let rest = key.strip_prefix("rec|")?;
    let mut parts = rest.splitn(3, '|');
    let pane_id = parts.next()?.to_string();
    let action = parts.next()?.to_string();
    Some((pane_id, action))
}

pub fn actionable_alert_keys_for_severity(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
    severity: Severity,
) -> Vec<String> {
    collect_entries(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    )
    .into_iter()
    .filter(|entry| entry.is_actionable() && entry.severity() == severity)
    .map(|entry| entry.key().to_string())
    .collect()
}

pub fn pending_auto_hide_count(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> usize {
    collect_entries(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    )
    .into_iter()
    .filter(|entry| entry.hide_deadline().is_some())
    .count()
}

/// v1.39 Pending-action discoverability surface B (footer counter).
/// Returns the number of currently-visible alerts whose executable
/// `suggested_command` survived filtering — i.e., the alerts that the
/// operator can `y`-copy. Reuses `collect_items` so this matches
/// exactly what `render_alerts` would render (same hide / sort
/// pipeline). Caller passes the same `notices`/`reports`/`hidden_until`
/// it would pass to `AlertView`.
///
/// Returns the highest severity among the matching alerts via the
/// second tuple slot so the footer can color the chip when N > 0
/// (None when the count is 0).
pub fn copy_alert_count(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> (usize, Option<Severity>) {
    let mut count = 0usize;
    let mut top: Option<Severity> = None;
    for entry in collect_entries(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    ) {
        if entry.suggested_command().is_some() {
            count += 1;
            top = Some(top.map_or(entry.severity(), |t| t.max(entry.severity())));
        }
    }
    (count, top)
}

/// v1.39 Pending-action discoverability surface C (overlay).
/// Per-alert metadata for `ui::pending_actions::collect_pending_items`.
/// Each entry corresponds to an alert with an executable
/// `suggested_command`, exposing enough state for the overlay to
/// render a row and to drive `alert_state.select(idx)` when the
/// operator presses Enter.
pub struct CommandAlertEntry {
    /// Index into the alert list as `render_alerts` orders it (so
    /// `alert_state.select(idx)` jumps to the matching alert).
    pub alert_idx: usize,
    pub command: String,
    pub title: String,
    pub severity: Severity,
    pub source: SourceKind,
    /// Pane the alert was emitted from, when available. `None` for
    /// `SystemNotice` and findings without an anchor pane (the
    /// overlay falls back to alert-only navigation).
    pub pane_id: Option<String>,
}

/// Walk the same alert pipeline `render_alerts` uses and return one
/// entry per alert with a runnable `suggested_command`. Reused by
/// the Pending Actions overlay (`a` key) so the overlay's row order
/// matches the queue rendering exactly.
pub fn alert_items_with_command(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<CommandAlertEntry> {
    let entries = collect_entries(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    );
    entries
        .into_iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            let command = entry.suggested_command()?.to_string();
            Some(CommandAlertEntry {
                alert_idx: idx,
                command,
                title: entry.title(),
                severity: entry.severity(),
                source: entry.command_source_kind(),
                pane_id: entry.pane_id(),
            })
        })
        .collect()
}

fn block(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            theme::BORDER_ACTIVE
        } else {
            theme::BORDER_IDLE
        }))
        .title(title)
}

pub fn alert_fingerprints(notices: &[SystemNotice], reports: &[PaneReport]) -> HashSet<String> {
    collect_entries(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    )
    .into_iter()
    .map(|entry| entry.key().to_string())
    .collect()
}

#[derive(Debug, Clone)]
struct AlertItem {
    key: String,
    timestamp: String,
    timestamp_sort_key: u32,
    severity: Severity,
    kind: AlertKind,
    title: String,
    headline: String,
    details: Vec<String>,
    suggested_command: Option<String>,
    source_kind: SourceKind,
    color: Color,
    is_new: bool,
    hide_deadline: Option<Instant>,
    related: Option<RelatedAlertContext>,
}

#[derive(Debug, Clone)]
struct RelatedAlertContext {
    group_key: String,
    pane_id: String,
    connector_label: String,
    total: usize,
    steps: Vec<RelatedAlertStep>,
    evidence: Vec<RelatedAlertEvidence>,
}

#[derive(Debug, Clone)]
struct RelatedAlertStep {
    label: String,
    is_current: bool,
}

#[derive(Debug, Clone)]
struct RelatedAlertEvidence {
    action: String,
    source_label: String,
}

#[derive(Debug, Clone)]
struct RelatedConnector {
    rank: u8,
    key: String,
    label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertKind {
    SystemNotice,
    Checkpoint,
    CrossPane,
    Recommendation,
}

#[derive(Debug, Clone)]
enum AlertEntry {
    Item(AlertItem),
    Flow(AlertFlow),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertFlowFamily {
    ContextRecovery,
}

#[derive(Debug, Clone)]
struct AlertFlow {
    key: String,
    family: AlertFlowFamily,
    pane_id: String,
    timestamp: String,
    timestamp_sort_key: u32,
    severity: Severity,
    kind_priority: u8,
    source_kind: SourceKind,
    color: Color,
    is_new: bool,
    hide_deadline: Option<Instant>,
    summary: String,
    steps: Vec<AlertFlowStep>,
    included: Vec<AlertItem>,
    suggested_command: Option<String>,
    command_identity: Option<(String, String)>,
    command_source_kind: Option<SourceKind>,
}

#[derive(Debug, Clone)]
struct AlertFlowStep {
    marker: &'static str,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ContextRecoveryCategory {
    Context,
    Cache,
    CacheAnomaly,
    Snapshot,
}

#[derive(Debug, Clone)]
struct SeverityChip {
    severity: Severity,
    start_col: u16,
    end_col: u16,
    total: usize,
    pending: usize,
}

fn collect_items(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<AlertItem> {
    let mut out = Vec::new();
    for n in notices {
        let badge = source_kind_label(n.source_kind);
        let color = theme::severity_color(n.severity);
        let key = notice_key(n);
        let Some(hide_deadline) = visible_hide_deadline(hidden_until, &key, now) else {
            continue;
        };
        let timestamp = alert_timestamp(&key, alert_times);
        let timestamp_sort_key = sortable_timestamp(&timestamp);
        let is_new = fresh_alerts.contains(&key);
        out.push(AlertItem {
            key,
            timestamp,
            timestamp_sort_key,
            severity: n.severity,
            kind: AlertKind::SystemNotice,
            title: format!("System Notice · {}", n.title),
            headline: format!("[{badge}] {}", n.body),
            details: vec![],
            suggested_command: None,
            source_kind: n.source_kind,
            color,
            is_new,
            hide_deadline,
            related: None,
        });
    }
    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| r.is_strong) {
            let color = theme::severity_color(rec.severity);
            let key = recommendation_key(&rep.pane_id, rec);
            let Some(hide_deadline) = visible_hide_deadline(hidden_until, &key, now) else {
                continue;
            };
            let timestamp = alert_timestamp(&key, alert_times);
            let timestamp_sort_key = sortable_timestamp(&timestamp);
            let is_new = fresh_alerts.contains(&key);
            out.push(AlertItem {
                key,
                timestamp,
                timestamp_sort_key,
                severity: rec.severity,
                kind: AlertKind::Checkpoint,
                title: format!("Checkpoint · {}", rep.pane_id),
                headline: format!("[{}] {}", source_kind_label(rec.source_kind), rec.reason),
                details: recommendation_detail_lines(rec),
                suggested_command: non_empty_command(rec.suggested_command.as_deref()),
                source_kind: rec.source_kind,
                color,
                is_new,
                hide_deadline,
                related: None,
            });
        }
    }
    for rep in reports {
        for f in &rep.cross_pane_findings {
            let color = theme::severity_color(f.severity);
            let key = finding_key(f);
            let Some(hide_deadline) = visible_hide_deadline(hidden_until, &key, now) else {
                continue;
            };
            let timestamp = alert_timestamp(&key, alert_times);
            let timestamp_sort_key = sortable_timestamp(&timestamp);
            let is_new = fresh_alerts.contains(&key);
            let suggested_command = non_empty_command(f.suggested_command.as_deref());
            let mut details = vec![
                aligned_detail("anchor", &f.anchor_pane_id),
                aligned_detail("others", &f.other_pane_ids.join(", ")),
            ];
            if let Some(cmd) = suggested_command.as_deref() {
                details.push(aligned_detail("run", &format!("`{cmd}`")));
            }
            // Phase D D1 (v1.17.0): distinguish cross-window from
            // same-window concurrent-work findings in both the title
            // and the headline so the operator can tell at a glance
            // whether the alert is "two panes in this window" vs
            // "same repo in two different tmux windows".
            let (title_prefix, headline_kind) = match f.kind {
                CrossPaneKind::ConcurrentMutatingWork => ("Cross-Pane", "cross-pane"),
                CrossPaneKind::CrossWindowConcurrentWork => ("Cross-Window", "cross-window"),
                CrossPaneKind::ConcurrentFileEdit => ("File-Conflict", "file-conflict"),
            };
            out.push(AlertItem {
                key,
                timestamp,
                timestamp_sort_key,
                severity: f.severity,
                kind: AlertKind::CrossPane,
                title: format!("{title_prefix} · {}", f.anchor_pane_id),
                headline: format!(
                    "[{}] {headline_kind} — {}",
                    source_kind_label(f.source_kind),
                    f.reason,
                ),
                details,
                suggested_command,
                source_kind: f.source_kind,
                color,
                is_new,
                hide_deadline,
                related: None,
            });
        }
    }
    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| !r.is_strong) {
            let color = theme::severity_color(rec.severity);
            let key = recommendation_key(&rep.pane_id, rec);
            let Some(hide_deadline) = visible_hide_deadline(hidden_until, &key, now) else {
                continue;
            };
            let timestamp = alert_timestamp(&key, alert_times);
            let timestamp_sort_key = sortable_timestamp(&timestamp);
            let is_new = fresh_alerts.contains(&key);
            out.push(AlertItem {
                key,
                timestamp,
                timestamp_sort_key,
                severity: rec.severity,
                kind: AlertKind::Recommendation,
                title: format!("Recommendation · {}", rep.pane_id),
                headline: format!("[{}] {}", source_kind_label(rec.source_kind), rec.reason),
                details: recommendation_detail_lines(rec),
                suggested_command: non_empty_command(rec.suggested_command.as_deref()),
                source_kind: rec.source_kind,
                color,
                is_new,
                hide_deadline,
                related: None,
            });
        }
    }
    out.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| b.is_new.cmp(&a.is_new))
            .then_with(|| b.timestamp_sort_key.cmp(&a.timestamp_sort_key))
            .then_with(|| b.kind.priority().cmp(&a.kind.priority()))
            .then_with(|| a.key.cmp(&b.key))
    });
    out
}

fn collect_entries(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<AlertEntry> {
    let items = collect_items(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    );
    project_alert_entries(items, fresh_alerts, hidden_until, now)
}

fn project_alert_entries(
    items: Vec<AlertItem>,
    fresh_alerts: &HashSet<String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<AlertEntry> {
    let mut consumed = HashSet::new();
    let mut flows = Vec::new();

    let mut by_pane: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        if let Some((pane_id, _)) = parse_recommendation_key(&item.key)
            && context_recovery_category(item).is_some()
        {
            by_pane.entry(pane_id).or_default().push(idx);
        }
    }

    for (pane_id, indexes) in by_pane {
        let categories: HashSet<ContextRecoveryCategory> = indexes
            .iter()
            .filter_map(|idx| context_recovery_category(&items[*idx]))
            .collect();
        if categories.len() < 2 {
            continue;
        }
        let included: Vec<AlertItem> = indexes.iter().map(|idx| items[*idx].clone()).collect();
        let key = context_recovery_flow_key(&pane_id, &included);
        match visible_hide_deadline(hidden_until, &key, now) {
            Some(hide_deadline) => {
                for idx in indexes {
                    consumed.insert(idx);
                }
                flows.push(AlertEntry::Flow(build_context_recovery_flow(
                    key,
                    pane_id,
                    included,
                    hide_deadline,
                    fresh_alerts,
                )));
            }
            None => {
                for idx in indexes {
                    consumed.insert(idx);
                }
            }
        }
    }

    let mut visible_items: Vec<AlertItem> = items
        .into_iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            if consumed.contains(&idx) {
                None
            } else {
                Some(item)
            }
        })
        .collect();
    apply_related_contexts(&mut visible_items);

    let mut entries: Vec<AlertEntry> = visible_items
        .into_iter()
        .map(AlertEntry::Item)
        .chain(flows)
        .collect();
    sort_entries(&mut entries);
    entries
}

fn apply_related_contexts(items: &mut [AlertItem]) {
    let mut groups: HashMap<String, (RelatedConnector, Vec<usize>)> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        let Some(pane_id) = recommendation_pane_id(item) else {
            continue;
        };
        for connector in related_connectors(item) {
            let group_key = format!("related|{pane_id}|{}", connector.key);
            groups
                .entry(group_key)
                .or_insert_with(|| (connector, Vec::new()))
                .1
                .push(idx);
        }
    }

    let mut candidates: Vec<(String, RelatedConnector, Vec<usize>)> = groups
        .into_iter()
        .filter_map(|(group_key, (connector, indexes))| {
            (indexes.len() >= 2).then_some((group_key, connector, indexes))
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.1.rank
            .cmp(&a.1.rank)
            .then_with(|| a.1.label.cmp(&b.1.label))
            .then_with(|| a.0.cmp(&b.0))
    });

    for (group_key, connector, indexes) in candidates {
        for idx in indexes.iter().copied() {
            if items[idx].related.is_none() {
                items[idx].related = Some(build_related_context(
                    &group_key, &connector, items, &indexes, idx,
                ));
            }
        }
    }
}

fn recommendation_pane_id(item: &AlertItem) -> Option<String> {
    parse_recommendation_key(&item.key).map(|(pane_id, _)| pane_id)
}

fn recommendation_action(item: &AlertItem) -> Option<String> {
    parse_recommendation_key(&item.key).map(|(_, action)| action)
}

fn related_connectors(item: &AlertItem) -> Vec<RelatedConnector> {
    if item.kind != AlertKind::Recommendation {
        return Vec::new();
    }
    let mut connectors = Vec::new();
    let recovery_category = context_recovery_category(item);
    let has_snapshot_recovery = item
        .details
        .iter()
        .any(|detail| detail.to_ascii_lowercase().contains("snapshot before"));
    if let Some(cmd) = item.suggested_command.as_deref() {
        connectors.push(RelatedConnector {
            rank: 40,
            key: format!("cmd|{cmd}"),
            label: format!("same {cmd} path"),
        });
    }
    if let Some(category) = recovery_category {
        connectors.push(RelatedConnector {
            rank: 30,
            key: format!("context-recovery|{category:?}"),
            label: "same pane recovery".into(),
        });
    }
    if has_snapshot_recovery {
        connectors.push(RelatedConnector {
            rank: 20,
            key: "recovery-path|snapshot-before".into(),
            label: "same pane recovery".into(),
        });
    }
    connectors
}

fn build_related_context(
    group_key: &str,
    connector: &RelatedConnector,
    items: &[AlertItem],
    indexes: &[usize],
    current_idx: usize,
) -> RelatedAlertContext {
    let pane_id = recommendation_pane_id(&items[current_idx]).unwrap_or_default();
    let mut displayed_steps = indexes.iter().copied().take(4).collect::<Vec<_>>();
    if !displayed_steps.contains(&current_idx)
        && let Some(last) = displayed_steps.last_mut()
    {
        *last = current_idx;
    }
    let steps = displayed_steps
        .iter()
        .copied()
        .map(|idx| RelatedAlertStep {
            label: related_step_label(&items[idx]),
            is_current: idx == current_idx,
        })
        .collect();
    let evidence = indexes
        .iter()
        .copied()
        .map(|idx| RelatedAlertEvidence {
            action: recommendation_action(&items[idx])
                .unwrap_or_else(|| items[idx].headline.clone()),
            source_label: source_kind_label(items[idx].source_kind).to_string(),
        })
        .collect();
    RelatedAlertContext {
        group_key: group_key.to_string(),
        pane_id,
        connector_label: connector.label.clone(),
        total: indexes.len(),
        steps,
        evidence,
    }
}

fn related_step_label(item: &AlertItem) -> String {
    match context_recovery_category(item) {
        Some(ContextRecoveryCategory::Context) => "context pressure".into(),
        Some(ContextRecoveryCategory::Cache | ContextRecoveryCategory::CacheAnomaly) => {
            "cache drift".into()
        }
        Some(ContextRecoveryCategory::Snapshot) => "snapshot before reset".into(),
        None => recommendation_action(item)
            .and_then(|action| action.split(':').next().map(str::trim).map(str::to_string))
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| "related alert".into()),
    }
}

fn sort_entries(entries: &mut [AlertEntry]) {
    entries.sort_by(|a, b| {
        b.severity()
            .cmp(&a.severity())
            .then_with(|| b.is_new().cmp(&a.is_new()))
            .then_with(|| b.timestamp_sort_key().cmp(&a.timestamp_sort_key()))
            .then_with(|| b.kind_priority().cmp(&a.kind_priority()))
            .then_with(|| a.key().cmp(b.key()))
    });
}

fn non_empty_command(command: Option<&str>) -> Option<String> {
    let cmd = command.map(str::trim).filter(|cmd| !cmd.is_empty())?;
    if cmd.starts_with('#') || contains_angle_placeholder(cmd) {
        return None;
    }
    Some(cmd.to_string())
}

fn contains_angle_placeholder(command: &str) -> bool {
    let bytes = command.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] != b'<' {
            idx += 1;
            continue;
        }
        if let Some(end_rel) = command[idx + 1..].find('>') {
            let body = &command[idx + 1..idx + 1 + end_rel];
            if !body.is_empty()
                && body
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                return true;
            }
            idx += end_rel + 2;
        } else {
            break;
        }
    }
    false
}

/// v1.59.0: case-insensitive substring match for the alert filter.
/// `needle` must already be lowercased + trimmed by the caller.
/// Matches against title + headline + each details row + the
/// suggested_command if any. Empty needle returns true (caller is
/// expected to short-circuit before calling this).
fn alert_item_matches_filter(item: &AlertItem, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if item.title.to_ascii_lowercase().contains(needle) {
        return true;
    }
    if item.headline.to_ascii_lowercase().contains(needle) {
        return true;
    }
    if item
        .details
        .iter()
        .any(|d| d.to_ascii_lowercase().contains(needle))
    {
        return true;
    }
    if let Some(cmd) = item.suggested_command.as_deref()
        && cmd.to_ascii_lowercase().contains(needle)
    {
        return true;
    }
    if let Some(related) = item.related.as_ref()
        && related_context_matches_filter(related, needle)
    {
        return true;
    }
    false
}

fn related_context_matches_filter(related: &RelatedAlertContext, needle: &str) -> bool {
    related.group_key.to_ascii_lowercase().contains(needle)
        || related.pane_id.to_ascii_lowercase().contains(needle)
        || related
            .connector_label
            .to_ascii_lowercase()
            .contains(needle)
        || related
            .steps
            .iter()
            .any(|step| step.label.to_ascii_lowercase().contains(needle))
        || related.evidence.iter().any(|evidence| {
            evidence.action.to_ascii_lowercase().contains(needle)
                || evidence.source_label.to_ascii_lowercase().contains(needle)
        })
}

fn alert_entry_matches_filter(entry: &AlertEntry, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    match entry {
        AlertEntry::Item(item) => alert_item_matches_filter(item, needle),
        AlertEntry::Flow(flow) => {
            if flow.title().to_ascii_lowercase().contains(needle) {
                return true;
            }
            if flow.summary.to_ascii_lowercase().contains(needle) {
                return true;
            }
            if flow
                .steps
                .iter()
                .any(|step| step.text.to_ascii_lowercase().contains(needle))
            {
                return true;
            }
            if flow.pane_id.to_ascii_lowercase().contains(needle) {
                return true;
            }
            if let Some(cmd) = flow.suggested_command.as_deref()
                && cmd.to_ascii_lowercase().contains(needle)
            {
                return true;
            }
            flow.included
                .iter()
                .any(|item| alert_item_matches_filter(item, needle))
        }
    }
}

fn context_recovery_category(item: &AlertItem) -> Option<ContextRecoveryCategory> {
    let (_, action) = parse_recommendation_key(&item.key)?;
    if action.starts_with("context-pressure:") {
        return Some(ContextRecoveryCategory::Context);
    }
    if action == "anomaly: cache discontinuity detected" {
        return Some(ContextRecoveryCategory::CacheAnomaly);
    }
    if action.starts_with("snapshot before ") {
        return Some(ContextRecoveryCategory::Snapshot);
    }
    if action.starts_with("cache:")
        && action.contains("drift detected")
        && (action.contains("/compact") || item.headline.contains("/compact"))
    {
        return Some(ContextRecoveryCategory::Cache);
    }
    None
}

fn context_recovery_flow_key(pane_id: &str, included: &[AlertItem]) -> String {
    let mut keys: Vec<&str> = included.iter().map(|item| item.key.as_str()).collect();
    keys.sort_unstable();
    format!("flow|context-recovery|{pane_id}|{}", keys.join("\u{1f}"))
}

fn build_context_recovery_flow(
    key: String,
    pane_id: String,
    included: Vec<AlertItem>,
    hide_deadline: Option<Instant>,
    fresh_alerts: &HashSet<String>,
) -> AlertFlow {
    let severity = included
        .iter()
        .map(|item| item.severity)
        .max()
        .unwrap_or(Severity::Concern);
    let source_kind = included
        .iter()
        .map(|item| item.source_kind)
        .max_by_key(|source| source_authority_rank(*source))
        .unwrap_or(SourceKind::Heuristic);
    let kind_priority = included
        .iter()
        .map(|item| item.kind.priority())
        .max()
        .unwrap_or(AlertKind::Recommendation.priority());
    let timestamp = included
        .iter()
        .max_by_key(|item| item.timestamp_sort_key)
        .map(|item| item.timestamp.clone())
        .unwrap_or_else(|| "--:--:--".into());
    let timestamp_sort_key = sortable_timestamp(&timestamp);
    let is_new = fresh_alerts.contains(&key) || included.iter().any(|item| item.is_new);
    let (suggested_command, command_identity, command_source_kind) =
        flow_command_and_identity(&included);
    let steps = context_recovery_steps(&included, suggested_command.as_deref());

    AlertFlow {
        key,
        family: AlertFlowFamily::ContextRecovery,
        pane_id,
        timestamp,
        timestamp_sort_key,
        severity,
        kind_priority,
        source_kind,
        color: theme::severity_color(severity),
        is_new,
        hide_deadline,
        summary: context_recovery_summary(&included, suggested_command.as_deref()),
        steps,
        included,
        suggested_command,
        command_identity,
        command_source_kind,
    }
}

fn source_authority_rank(source: SourceKind) -> u8 {
    match source {
        SourceKind::ProviderOfficial => 4,
        SourceKind::ProjectCanonical => 3,
        SourceKind::Heuristic => 2,
        SourceKind::Estimated => 1,
    }
}

fn flow_command_and_identity(
    included: &[AlertItem],
) -> (Option<String>, Option<(String, String)>, Option<SourceKind>) {
    let preferred = included
        .iter()
        .find(|item| item.suggested_command.as_deref() == Some("/compact"))
        .or_else(|| {
            included
                .iter()
                .find(|item| item.suggested_command.is_some())
        });
    let Some(item) = preferred else {
        return (None, None, None);
    };
    (
        item.suggested_command.clone(),
        parse_recommendation_key(&item.key),
        Some(item.source_kind),
    )
}

fn context_recovery_summary(included: &[AlertItem], command: Option<&str>) -> String {
    let has_context = included
        .iter()
        .any(|item| context_recovery_category(item) == Some(ContextRecoveryCategory::Context));
    let has_cache = included.iter().any(|item| {
        matches!(
            context_recovery_category(item),
            Some(ContextRecoveryCategory::Cache | ContextRecoveryCategory::CacheAnomaly)
        )
    });
    match (has_context, has_cache, command) {
        (true, true, Some("/compact")) => {
            "context pressure led to cache drift; snapshot before compact".into()
        }
        (true, true, _) => "context pressure and cache drift share one recovery path".into(),
        (true, false, _) => "context pressure has a related recovery step".into(),
        (false, true, _) => "cache drift has a related recovery step".into(),
        (false, false, _) => "related recovery alerts share one response path".into(),
    }
}

fn context_recovery_steps(included: &[AlertItem], command: Option<&str>) -> Vec<AlertFlowStep> {
    let mut steps = Vec::new();
    if included
        .iter()
        .any(|item| context_recovery_category(item) == Some(ContextRecoveryCategory::Context))
    {
        steps.push(AlertFlowStep {
            marker: "o",
            text: "context pressure detected".into(),
        });
    }
    if included.iter().any(|item| {
        matches!(
            context_recovery_category(item),
            Some(ContextRecoveryCategory::Cache | ContextRecoveryCategory::CacheAnomaly)
        )
    }) {
        steps.push(AlertFlowStep {
            marker: "|",
            text: "cache drift/discontinuity detected".into(),
        });
    }
    if included
        .iter()
        .any(|item| context_recovery_category(item) == Some(ContextRecoveryCategory::Snapshot))
        || included.iter().any(|item| {
            item.details
                .iter()
                .any(|detail| detail.to_ascii_lowercase().contains("snapshot"))
        })
    {
        steps.push(AlertFlowStep {
            marker: "o",
            text: "snapshot before reset".into(),
        });
    }
    if let Some(cmd) = command {
        steps.push(AlertFlowStep {
            marker: "|",
            text: format!("then run {cmd}"),
        });
    }
    steps
}

fn bulk_hide_line(entries: &[AlertEntry]) -> Line<'static> {
    let chips = entry_severity_chips(entries);
    let mut spans = vec![Span::styled(
        BULK_HIDE_PREFIX.to_string(),
        Style::default()
            .fg(theme::text_dim())
            .add_modifier(Modifier::BOLD),
    )];
    if chips.is_empty() {
        spans.push(Span::styled(
            "actionable alerts only".to_string(),
            Style::default().fg(theme::text_dim()),
        ));
        return Line::from(spans);
    }
    for (idx, chip) in chips.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  ".to_string()));
        }
        spans.push(Span::styled(
            format!("{} ", chip.marker_text()),
            theme::label_style(),
        ));
        spans.push(Span::styled(
            severity_badge_text(chip.severity).to_string(),
            theme::severity_badge_style(chip.severity).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {}", chip.count_label()),
            Style::default().fg(theme::text_primary()),
        ));
    }
    Line::from(spans)
}

fn entry_severity_chips(entries: &[AlertEntry]) -> Vec<SeverityChip> {
    let mut chips = Vec::new();
    let mut start_col = BULK_HIDE_PREFIX.chars().count() as u16;
    for severity in [
        Severity::Risk,
        Severity::Warning,
        Severity::Concern,
        Severity::Good,
        Severity::Safe,
    ] {
        let matching: Vec<&AlertEntry> = entries
            .iter()
            .filter(|entry| entry.is_actionable() && entry.severity() == severity)
            .collect();
        if matching.is_empty() {
            continue;
        }
        let total = matching.len();
        let pending = matching
            .iter()
            .filter(|entry| entry.hide_deadline().is_some())
            .count();
        let width = SeverityChip::display_text(severity, total, pending)
            .chars()
            .count() as u16;
        chips.push(SeverityChip {
            severity,
            start_col,
            end_col: start_col.saturating_add(width),
            total,
            pending,
        });
        start_col = start_col.saturating_add(width + 2);
    }
    chips
}

#[cfg(test)]
fn severity_chips(items: &[AlertItem]) -> Vec<SeverityChip> {
    let entries: Vec<AlertEntry> = items.iter().cloned().map(AlertEntry::Item).collect();
    entry_severity_chips(&entries)
}

fn list_width(inner_width: u16) -> usize {
    inner_width.saturating_sub(3) as usize
}

fn alert_entry_list_item(
    entry: &AlertEntry,
    width: usize,
    now: Instant,
    is_selected: bool,
) -> ListItem<'static> {
    ListItem::new(alert_entry_lines(entry, width, now, is_selected))
        .style(alert_style(entry.color(), entry.is_new()))
}

fn alert_entry_lines(
    entry: &AlertEntry,
    width: usize,
    now: Instant,
    is_selected: bool,
) -> Vec<Line<'static>> {
    match entry {
        AlertEntry::Item(item) => alert_item_lines(item, width, now, is_selected),
        AlertEntry::Flow(flow) => alert_flow_lines(flow, width, now, is_selected),
    }
}

fn related_summary_line(related: &RelatedAlertContext) -> String {
    aligned_detail(
        "related",
        &format!(
            "{} alerts on {} · {}",
            related.total, related.pane_id, related.connector_label
        ),
    )
}

fn related_rail_line(related: &RelatedAlertContext) -> String {
    let body = related
        .steps
        .iter()
        .map(|step| {
            let marker = if step.is_current { "[o]" } else { "[ ]" };
            format!("{marker} {}", step.label)
        })
        .collect::<Vec<_>>()
        .join(" ");
    aligned_detail("rail", &body)
}

fn related_evidence_line(evidence: &RelatedAlertEvidence) -> String {
    aligned_detail(
        "included",
        &format!("{} [{}]", evidence.action, evidence.source_label),
    )
}

fn alert_item_lines(
    item: &AlertItem,
    width: usize,
    now: Instant,
    is_selected: bool,
) -> Vec<Line<'static>> {
    let prefix = timestamp_prefix(item);
    let continuation = continuation_prefix(&prefix);
    let mut lines: Vec<Line<'static>> = vec![title_line(item, &prefix)];
    lines.extend(
        wrap_with_prefix(
            &dismiss_line_text(item, now),
            width,
            &continuation,
            &continuation,
        )
        .into_iter()
        .map(Line::from),
    );
    lines.extend(
        wrap_with_prefix(
            &aligned_detail("summary", &item.headline),
            width,
            &continuation,
            &continuation,
        )
        .into_iter()
        .map(Line::from),
    );
    for detail in &item.details {
        lines.extend(
            wrap_with_prefix(detail, width, &continuation, &continuation)
                .into_iter()
                .map(Line::from),
        );
    }
    if let Some(related) = item.related.as_ref() {
        for detail in [related_summary_line(related), related_rail_line(related)] {
            lines.extend(
                wrap_with_prefix(&detail, width, &continuation, &continuation)
                    .into_iter()
                    .map(|line| Line::styled(line, Style::default().fg(theme::text_dim()))),
            );
        }
        if is_selected {
            for evidence in &related.evidence {
                lines.extend(
                    wrap_with_prefix(
                        &related_evidence_line(evidence),
                        width,
                        &continuation,
                        &continuation,
                    )
                    .into_iter()
                    .map(|line| Line::styled(line, Style::default().fg(theme::text_dim()))),
                );
            }
        }
    }
    // v1.38 §6.5: on the currently selected alert only, when its
    // `suggested_command` is `Some(cmd)`, append a dim
    // `copy    : `<cmd>`  → press y to copy` line below the existing
    // detail rows so the operator can see the runnable command and the
    // keypress hint without reaching for the metrics overlay. Uses
    // `aligned_detail` and backticks to match the `run` / `summary` /
    // `dismiss` sibling rows.
    if is_selected && let Some(cmd) = item.suggested_command.as_deref() {
        for wrapped in wrap_copy_detail(width, &continuation, cmd) {
            lines.push(Line::styled(
                wrapped,
                Style::default().fg(theme::text_dim()),
            ));
        }
    }
    lines.push(Line::styled(
        format!(
            "{}{}",
            continuation,
            "─".repeat(width.saturating_sub(continuation.chars().count()).max(8))
        ),
        Style::default().fg(theme::text_dim()),
    ));
    lines
}

fn alert_flow_lines(
    flow: &AlertFlow,
    width: usize,
    now: Instant,
    is_selected: bool,
) -> Vec<Line<'static>> {
    let prefix = format!("[{}] ", flow.timestamp);
    let continuation = continuation_prefix(&prefix);
    let mut lines = vec![flow_title_line(flow, &prefix)];
    lines.extend(
        wrap_with_prefix(
            &flow_dismiss_line_text(flow, now),
            width,
            &continuation,
            &continuation,
        )
        .into_iter()
        .map(Line::from),
    );
    lines.extend(
        wrap_with_prefix(
            &aligned_detail("summary", &flow.summary),
            width,
            &continuation,
            &continuation,
        )
        .into_iter()
        .map(Line::from),
    );
    for step in &flow.steps {
        lines.extend(
            wrap_with_prefix(
                &format!("{} {}", step.marker, step.text),
                width,
                &continuation,
                &continuation,
            )
            .into_iter()
            .map(Line::from),
        );
    }
    if is_selected {
        for item in &flow.included {
            let evidence = parse_recommendation_key(&item.key)
                .map(|(_, action)| action)
                .unwrap_or_else(|| item.headline.clone());
            let evidence = format!("{} [{}]", evidence, source_kind_label(item.source_kind));
            lines.extend(
                wrap_with_prefix(
                    &aligned_detail("included", &evidence),
                    width,
                    &continuation,
                    &continuation,
                )
                .into_iter()
                .map(|line| Line::styled(line, Style::default().fg(theme::text_dim()))),
            );
        }
        if let Some(cmd) = flow.suggested_command.as_deref() {
            for wrapped in wrap_copy_detail(width, &continuation, cmd) {
                lines.push(Line::styled(
                    wrapped,
                    Style::default().fg(theme::text_dim()),
                ));
            }
        }
    }
    lines.push(Line::styled(
        format!(
            "{}{}",
            continuation,
            "─".repeat(width.saturating_sub(continuation.chars().count()).max(8))
        ),
        Style::default().fg(theme::text_dim()),
    ));
    lines
}

fn dismiss_line_text(item: &AlertItem, now: Instant) -> String {
    match item.hide_deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(now);
            let secs = remaining.as_secs().max(1);
            aligned_detail(
                "dismiss",
                &format!("[x] auto-hide in {secs}s · click undo · Enter/Space undo"),
            )
        }
        None => aligned_detail("dismiss", "[ ] click hide · Enter/Space hide"),
    }
}

fn flow_dismiss_line_text(flow: &AlertFlow, now: Instant) -> String {
    match flow.hide_deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(now);
            let secs = remaining.as_secs().max(1);
            aligned_detail(
                "dismiss",
                &format!("[x] auto-hide in {secs}s · click undo · Enter/Space undo"),
            )
        }
        None => aligned_detail("dismiss", "[ ] click hide · Enter/Space hide"),
    }
}

fn dismiss_line_count(item: &AlertItem, width: usize, now: Instant) -> usize {
    let prefix = timestamp_prefix(item);
    let continuation = continuation_prefix(&prefix);
    wrap_with_prefix(
        &dismiss_line_text(item, now),
        width,
        &continuation,
        &continuation,
    )
    .len()
}

fn entry_dismiss_line_count(entry: &AlertEntry, width: usize, now: Instant) -> usize {
    match entry {
        AlertEntry::Item(item) => dismiss_line_count(item, width, now),
        AlertEntry::Flow(flow) => {
            let prefix = format!("[{}] ", flow.timestamp);
            let continuation = continuation_prefix(&prefix);
            wrap_with_prefix(
                &flow_dismiss_line_text(flow, now),
                width,
                &continuation,
                &continuation,
            )
            .len()
        }
    }
}

fn title_line(item: &AlertItem, prefix: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_string())];
    if item.is_new {
        spans.push(Span::styled(
            "NEW ",
            Style::default()
                .fg(theme::text_primary())
                .bg(theme::badge_bg())
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!(" {} ", severity_badge_text(item.severity)),
        theme::severity_badge_style(item.severity).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        item.title.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    // v1.39 Pending-action discoverability surface A: when this alert
    // has a `suggested_command`, append a `★y` chip colored to match
    // its severity so the operator can spot copyable items in the
    // alert queue without selecting each one. Mirrors the `★p` chip
    // on pane cards (panels.rs).
    if let Some(label) = item_action_label(item.suggested_command.is_some()) {
        spans.push(Span::styled(
            format!("  {label}"),
            Style::default()
                .fg(theme::severity_color(item.severity))
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn flow_title_line(flow: &AlertFlow, prefix: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_string())];
    if flow.is_new {
        spans.push(Span::styled(
            "NEW ",
            Style::default()
                .fg(theme::text_primary())
                .bg(theme::badge_bg())
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!(" {} ", severity_badge_text(flow.severity)),
        theme::severity_badge_style(flow.severity).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        flow.title(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    if let Some(label) = item_action_label(flow.suggested_command.is_some()) {
        spans.push(Span::styled(
            format!("  {label}"),
            Style::default()
                .fg(theme::severity_color(flow.severity))
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn item_action_label(has_command: bool) -> Option<&'static str> {
    has_command.then_some("\u{2605}y")
}

fn entry_summary_line_count(entry: &AlertEntry, width: usize) -> usize {
    match entry {
        AlertEntry::Item(item) => {
            let prefix = timestamp_prefix(item);
            let continuation = continuation_prefix(&prefix);
            wrap_with_prefix(
                &aligned_detail("summary", &item.headline),
                width,
                &continuation,
                &continuation,
            )
            .len()
        }
        AlertEntry::Flow(flow) => {
            let prefix = format!("[{}] ", flow.timestamp);
            let continuation = continuation_prefix(&prefix);
            wrap_with_prefix(
                &aligned_detail("summary", &flow.summary),
                width,
                &continuation,
                &continuation,
            )
            .len()
        }
    }
}

fn entry_copy_line_count(entry: &AlertEntry, width: usize) -> usize {
    let Some(cmd) = entry.suggested_command() else {
        return 0;
    };
    let prefix = match entry {
        AlertEntry::Item(item) => timestamp_prefix(item),
        AlertEntry::Flow(flow) => format!("[{}] ", flow.timestamp),
    };
    let continuation = continuation_prefix(&prefix);
    wrap_copy_detail(width, &continuation, cmd).len()
}

fn wrap_copy_detail(width: usize, continuation: &str, cmd: &str) -> Vec<String> {
    let detail = aligned_detail("copy", &format!("`{cmd}`  → press y to copy"));
    if continuation.chars().count() + detail.chars().count() <= width {
        vec![format!("{continuation}{detail}")]
    } else {
        wrap_with_prefix(&detail, width, continuation, continuation)
    }
}

fn alert_style(color: Color, is_new: bool) -> Style {
    let style = Style::default().fg(color);
    if is_new {
        style.bg(theme::badge_bg()).add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn highlight_style(focused: bool) -> Style {
    let style = Style::default().fg(theme::text_primary());
    if focused {
        style.bg(theme::badge_bg()).add_modifier(Modifier::BOLD)
    } else {
        style.add_modifier(Modifier::BOLD)
    }
}

fn alert_timestamp(key: &str, alert_times: &HashMap<String, String>) -> String {
    alert_times
        .get(key)
        .cloned()
        .unwrap_or_else(|| "--:--:--".into())
}

fn sortable_timestamp(timestamp: &str) -> u32 {
    let mut parts = timestamp.split(':');
    let Some(hours) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return 0;
    };
    let Some(minutes) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return 0;
    };
    let Some(seconds) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return 0;
    };
    if parts.next().is_some() {
        return 0;
    }
    (hours * 60 * 60) + (minutes * 60) + seconds
}

fn timestamp_prefix(item: &AlertItem) -> String {
    format!("[{}] ", item.timestamp)
}

fn continuation_prefix(prefix: &str) -> String {
    " ".repeat(prefix.chars().count())
}

fn visible_hide_deadline(
    hidden_until: &HashMap<String, Instant>,
    key: &str,
    now: Instant,
) -> Option<Option<Instant>> {
    match hidden_until.get(key).copied() {
        Some(deadline) if deadline <= now => None,
        Some(deadline) => Some(Some(deadline)),
        None => Some(None),
    }
}

pub fn recommendation_detail_lines(rec: &Recommendation) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(next) = rec.next_step.as_deref().filter(|s| !s.is_empty()) {
        out.push(aligned_detail("next", next));
    }
    if let Some(cmd) = rec.suggested_command.as_deref().filter(|s| !s.is_empty()) {
        out.push(aligned_detail("run", &format!("`{cmd}`")));
    }
    out
}

fn aligned_detail(label: &str, value: &str) -> String {
    format!("{label:<width$}: {value}", width = DETAIL_LABEL_WIDTH)
}

fn severity_badge_text(sev: Severity) -> &'static str {
    match sev {
        Severity::Safe => "SAFE",
        Severity::Good => "GOOD",
        Severity::Concern => "CONCERN",
        Severity::Warning => "WARNING",
        Severity::Risk => "RISK",
    }
}

fn wrap_with_prefix(
    text: &str,
    total_width: usize,
    prefix: &str,
    continuation_prefix: &str,
) -> Vec<String> {
    let reserved = prefix
        .chars()
        .count()
        .max(continuation_prefix.chars().count());
    let content_width = total_width.saturating_sub(reserved).max(12);
    let wrapped = wrap_text(text, content_width);
    let mut out = Vec::with_capacity(wrapped.len());
    for (idx, line) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            out.push(format!("{prefix}{line}"));
        } else {
            out.push(format!("{continuation_prefix}{line}"));
        }
    }
    out
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if word.chars().count() > width {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            out.extend(split_long_word(word, width));
            continue;
        }

        if current.is_empty() {
            current.push_str(word);
            continue;
        }

        let next_len = current.chars().count() + 1 + word.chars().count();
        if next_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn split_long_word(word: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut chunk = String::new();
    for ch in word.chars() {
        if chunk.chars().count() == width {
            out.push(std::mem::take(&mut chunk));
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        out.push(chunk);
    }
    out
}

fn sync_list_selection(state: &mut ListState, item_count: usize) {
    match item_count {
        0 => state.select(None),
        count => {
            let selected = state.selected().unwrap_or(0).min(count.saturating_sub(1));
            state.select(Some(selected));
        }
    }
}

fn notice_key(notice: &SystemNotice) -> String {
    format!(
        "notice|{}|{}|{}|{}",
        notice.title,
        notice.body,
        notice.severity.letter(),
        notice.source_kind.badge(),
    )
}

fn recommendation_key(pane_id: &str, rec: &Recommendation) -> String {
    format!(
        "rec|{pane_id}|{}|{}|{}",
        rec.action,
        rec.severity.letter(),
        rec.reason,
    )
}

fn finding_key(finding: &CrossPaneFinding) -> String {
    format!(
        "finding|{}|{}|{}",
        finding.anchor_pane_id,
        finding.severity.letter(),
        finding.reason,
    )
}

fn severity_word(sev: Severity) -> &'static str {
    match sev {
        Severity::Safe => "safe",
        Severity::Good => "good",
        Severity::Concern => "concern",
        Severity::Warning => "warning",
        Severity::Risk => "risk",
    }
}

pub fn severity_letter(sev: Severity) -> &'static str {
    sev.letter()
}

impl AlertKind {
    fn priority(self) -> u8 {
        match self {
            AlertKind::SystemNotice => 0,
            AlertKind::Recommendation => 1,
            AlertKind::CrossPane => 2,
            AlertKind::Checkpoint => 3,
        }
    }

    fn is_actionable(self) -> bool {
        !matches!(self, AlertKind::SystemNotice)
    }
}

impl AlertEntry {
    fn key(&self) -> &str {
        match self {
            AlertEntry::Item(item) => &item.key,
            AlertEntry::Flow(flow) => &flow.key,
        }
    }

    fn severity(&self) -> Severity {
        match self {
            AlertEntry::Item(item) => item.severity,
            AlertEntry::Flow(flow) => flow.severity,
        }
    }

    fn timestamp_sort_key(&self) -> u32 {
        match self {
            AlertEntry::Item(item) => item.timestamp_sort_key,
            AlertEntry::Flow(flow) => flow.timestamp_sort_key,
        }
    }

    fn kind_priority(&self) -> u8 {
        match self {
            AlertEntry::Item(item) => item.kind.priority(),
            AlertEntry::Flow(flow) => flow.kind_priority,
        }
    }

    fn is_new(&self) -> bool {
        match self {
            AlertEntry::Item(item) => item.is_new,
            AlertEntry::Flow(flow) => flow.is_new,
        }
    }

    fn hide_deadline(&self) -> Option<Instant> {
        match self {
            AlertEntry::Item(item) => item.hide_deadline,
            AlertEntry::Flow(flow) => flow.hide_deadline,
        }
    }

    fn suggested_command(&self) -> Option<&str> {
        match self {
            AlertEntry::Item(item) => item.suggested_command.as_deref(),
            AlertEntry::Flow(flow) => flow.suggested_command.as_deref(),
        }
    }

    fn command_source_kind(&self) -> SourceKind {
        match self {
            AlertEntry::Item(item) => item.source_kind,
            AlertEntry::Flow(flow) => flow.command_source_kind.unwrap_or(flow.source_kind),
        }
    }

    fn title(&self) -> String {
        match self {
            AlertEntry::Item(item) => item.title.clone(),
            AlertEntry::Flow(flow) => flow.title(),
        }
    }

    fn color(&self) -> Color {
        match self {
            AlertEntry::Item(item) => item.color,
            AlertEntry::Flow(flow) => flow.color,
        }
    }

    fn pane_id(&self) -> Option<String> {
        match self {
            AlertEntry::Item(item) => item
                .key
                .strip_prefix("rec|")
                .and_then(|tail| tail.split('|').next())
                .map(|s| s.to_string())
                .or_else(|| {
                    item.key
                        .strip_prefix("finding|")
                        .and_then(|tail| tail.split('|').next())
                        .map(|s| s.to_string())
                }),
            AlertEntry::Flow(flow) => Some(flow.pane_id.clone()),
        }
    }

    fn recommendation_identity(&self) -> Option<(String, String)> {
        match self {
            AlertEntry::Item(item) => parse_recommendation_key(&item.key),
            AlertEntry::Flow(flow) => flow.command_identity.clone(),
        }
    }

    fn is_actionable(&self) -> bool {
        match self {
            AlertEntry::Item(item) => item.kind.is_actionable(),
            AlertEntry::Flow(_) => true,
        }
    }

    #[cfg(test)]
    fn as_flow(&self) -> Option<&AlertFlow> {
        match self {
            AlertEntry::Flow(flow) => Some(flow),
            AlertEntry::Item(_) => None,
        }
    }
}

impl AlertFlow {
    fn title(&self) -> String {
        let family = match self.family {
            AlertFlowFamily::ContextRecovery => "Context recovery",
        };
        format!("FLOW {family} · {} alerts · active", self.included.len())
    }
}

impl SeverityChip {
    fn count_label(&self) -> String {
        if self.pending > 0 && self.pending < self.total {
            format!("{}/{}", self.pending, self.total)
        } else {
            self.total.to_string()
        }
    }

    fn marker_text(&self) -> &'static str {
        match (self.pending, self.total) {
            (0, _) => "[ ]",
            (pending, total) if pending >= total => "[x]",
            _ => "[-]",
        }
    }

    fn display_text(severity: Severity, total: usize, pending: usize) -> String {
        let count = if pending > 0 && pending < total {
            format!("{pending}/{total}")
        } else {
            total.to_string()
        };
        let marker = match (pending, total) {
            (0, _) => "[ ]",
            (armed, all) if armed >= all => "[x]",
            _ => "[-]",
        };
        format!("{marker} {} {count}", severity_badge_text(severity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::{CrossPaneFinding, CrossPaneKind, Recommendation};
    use crate::domain::signal::SignalSet;

    fn base_report(recs: Vec<Recommendation>) -> PaneReport {
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
            effects: vec![],
            dead: false,
            recommendations: recs,
            current_path: "/repo".into(),
            worktree_role: None,
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(), // F-3: test fixture; production fetches via event_loop
            anomalies: vec![],
        }
    }

    fn rec(
        action: &'static str,
        reason: &str,
        severity: Severity,
        source_kind: SourceKind,
        suggested_command: Option<&str>,
        next_step: Option<&str>,
    ) -> Recommendation {
        Recommendation {
            action,
            reason: reason.into(),
            severity,
            source_kind,
            suggested_command: suggested_command.map(str::to_string),
            side_effects: vec![],
            is_strong: false,
            next_step: next_step.map(str::to_string),
            profile: None,
        }
    }

    fn report_with_pane(pane_id: &str, recs: Vec<Recommendation>) -> PaneReport {
        let mut report = base_report(recs);
        report.pane_id = pane_id.into();
        report.identity.identity.pane_id = pane_id.into();
        report
    }

    fn item_related(entry: &AlertEntry) -> Option<&RelatedAlertContext> {
        match entry {
            AlertEntry::Item(item) => item.related.as_ref(),
            AlertEntry::Flow(_) => None,
        }
    }

    fn entry_title(entry: &AlertEntry) -> String {
        entry.title()
    }

    #[test]
    fn related_context_includes_current_step_for_truncated_groups() {
        let recs = vec![
            rec(
                "action:zero",
                "zero should stay in top view",
                Severity::Warning,
                SourceKind::Estimated,
                Some("/compact"),
                None,
            ),
            rec(
                "action:one",
                "one should stay in top view",
                Severity::Warning,
                SourceKind::Estimated,
                Some("/compact"),
                None,
            ),
            rec(
                "action:two",
                "two should stay in top view",
                Severity::Warning,
                SourceKind::Estimated,
                Some("/compact"),
                None,
            ),
            rec(
                "action:three",
                "three should stay in top view",
                Severity::Warning,
                SourceKind::Estimated,
                Some("/compact"),
                None,
            ),
            rec(
                "action:four",
                "four should be truncated unless current-step guaranteed",
                Severity::Warning,
                SourceKind::Estimated,
                Some("/compact"),
                None,
            ),
        ];
        let times = HashMap::from([
            (recommendation_key("%1", &recs[0]), "14:33:05".into()),
            (recommendation_key("%1", &recs[1]), "14:33:04".into()),
            (recommendation_key("%1", &recs[2]), "14:33:03".into()),
            (recommendation_key("%1", &recs[3]), "14:33:02".into()),
            (recommendation_key("%1", &recs[4]), "14:33:01".into()),
        ]);
        let report = base_report(recs.clone());

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &times,
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 5);
        let target = entries.iter().find(|entry| match entry {
            AlertEntry::Item(item) => item.headline.contains("four should"),
            AlertEntry::Flow(_) => false,
        });
        let target = match target.expect("target entry should remain visible") {
            AlertEntry::Item(item) => item,
            AlertEntry::Flow(_) => panic!("target entry is an item"),
        };
        let related = target
            .related
            .as_ref()
            .expect("target entry should include related context");
        assert_eq!(related.steps.len(), 4);
        let current_step = related
            .steps
            .iter()
            .find(|step| step.is_current)
            .expect("current item should keep a step in a truncated group");
        assert!(current_step.label.contains("action"));
    }

    #[test]
    fn context_recovery_flow_groups_context_and_cache_on_same_pane() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context.clone(), cache.clone()]);
        let times = HashMap::from([
            (recommendation_key("%1", &context), "14:23:08".into()),
            (recommendation_key("%1", &cache), "14:23:12".into()),
        ]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &times,
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(
            entries.len(),
            1,
            "context/cache should collapse into one flow"
        );
        let flow = entries[0].as_flow().expect("entry must be a flow");
        assert_eq!(flow.pane_id, "%1");
        assert_eq!(flow.severity, Severity::Warning);
        assert_eq!(flow.suggested_command.as_deref(), Some("/compact"));
        assert_eq!(
            flow.command_identity
                .as_ref()
                .map(|(_, action)| action.as_str()),
            Some("context-pressure: checkpoint")
        );
        assert_eq!(flow.included.len(), 2);
        assert!(flow.title().contains("Context recovery"));
        assert!(flow.summary.contains("context pressure"));
    }

    #[test]
    fn context_recovery_flow_requires_two_categories() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let report = base_report(vec![context]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], AlertEntry::Item(_)));
    }

    #[test]
    fn related_context_links_same_pane_same_command_without_collapsing() {
        let compact_quota = rec(
            "quota-pressure: compact soon",
            "quota pressure can recover after compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let compact_profile = rec(
            "profile-switch: compact profile",
            "profile switch should happen after compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let report = base_report(vec![compact_quota.clone(), compact_profile.clone()]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(
            entries.len(),
            2,
            "weak related context must not collapse into FLOW"
        );
        assert!(
            entries
                .iter()
                .all(|entry| matches!(entry, AlertEntry::Item(_)))
        );
        assert_eq!(entry_title(&entries[0]), "Recommendation · %1");

        let related: Vec<&RelatedAlertContext> = entries.iter().filter_map(item_related).collect();
        assert_eq!(
            related.len(),
            2,
            "both visible siblings should carry related context"
        );
        assert!(related.iter().all(|ctx| ctx.total == 2));
        assert!(related.iter().all(|ctx| ctx.pane_id == "%1"));
        assert!(
            related
                .iter()
                .all(|ctx| ctx.connector_label == "same /compact path")
        );
    }

    #[test]
    fn related_context_does_not_link_different_panes() {
        let first = rec(
            "quota-pressure: compact soon",
            "first pane compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let second = rec(
            "profile-switch: compact profile",
            "second pane compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let reports = vec![
            report_with_pane("%1", vec![first]),
            report_with_pane("%2", vec![second]),
        ];

        let entries = collect_entries(
            &[],
            &reports,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| item_related(entry).is_none()));
    }

    #[test]
    fn related_context_does_not_decorate_items_consumed_by_flow() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cache]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 1);
        assert!(entries[0].as_flow().is_some());
        assert!(item_related(&entries[0]).is_none());
    }

    #[test]
    fn related_context_same_action_family_requires_shared_command_or_recovery_connector() {
        let first = rec(
            "profile-switch: compact profile",
            "first profile switch recommendation",
            Severity::Warning,
            SourceKind::Estimated,
            None,
            None,
        );
        let second = rec(
            "profile-switch: open profile docs",
            "second profile switch recommendation",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            None,
            None,
        );
        let report = base_report(vec![first, second]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| item_related(entry).is_none()));
    }

    #[test]
    fn related_context_does_not_link_same_family_with_different_commands() {
        let compact = rec(
            "maintenance: compact first",
            "compact first",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let snapshot = rec(
            "maintenance: snapshot first",
            "snapshot first",
            Severity::Warning,
            SourceKind::ProjectCanonical,
            Some("qmonster snapshot"),
            None,
        );
        let report = base_report(vec![compact, snapshot]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| item_related(entry).is_none()));
    }

    #[test]
    fn context_recovery_flow_does_not_merge_different_panes() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let reports = vec![
            report_with_pane("%1", vec![context]),
            report_with_pane("%2", vec![cache]),
        ];

        let entries = collect_entries(
            &[],
            &reports,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| matches!(entry, AlertEntry::Item(_)))
        );
    }

    #[test]
    fn context_recovery_flow_key_uses_stable_projected_key() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let raw_context_key = recommendation_key("%1", &context);
        let raw_cache_key = recommendation_key("%1", &cache);
        let report = base_report(vec![context.clone(), cache.clone()]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        let entry_keys: Vec<&str> = entries.iter().map(|entry| entry.key()).collect();
        assert_eq!(entry_keys.len(), 1);
        assert!(entry_keys[0].starts_with("flow|context-recovery|%1|"));
        assert!(!entry_keys.contains(&raw_context_key.as_str()));
        assert!(!entry_keys.contains(&raw_cache_key.as_str()));
    }

    #[test]
    fn context_recovery_flow_does_not_group_hot_cache_advice() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let hot_cache = rec(
            "cache: avoid /compact while cache is hot",
            "cache is still hot after a recent reset",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, hot_cache]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| matches!(entry, AlertEntry::Item(_)))
        );
    }

    #[test]
    fn context_recovery_flow_does_not_group_cold_cache_advice() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cold_cache = rec(
            "cache: /compact is safe — cache is cold",
            "cache has cooled and compact is safe",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cold_cache]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| matches!(entry, AlertEntry::Item(_)))
        );
    }

    #[test]
    fn context_recovery_flow_preserves_highest_included_kind_priority() {
        let mut strong_context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        strong_context.is_strong = true;
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Warning,
            SourceKind::Heuristic,
            None,
            None,
        );
        let plain = rec(
            "notify-input-wait",
            "waiting for user input",
            Severity::Warning,
            SourceKind::ProjectCanonical,
            None,
            None,
        );
        let report = base_report(vec![strong_context.clone(), cache.clone(), plain.clone()]);
        let times = HashMap::from([
            (recommendation_key("%1", &strong_context), "14:23:12".into()),
            (recommendation_key("%1", &cache), "14:23:12".into()),
            (recommendation_key("%1", &plain), "14:23:12".into()),
        ]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &times,
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(
            entries[0].as_flow().is_some(),
            "flow should sort before plain recommendation"
        );
        assert!(matches!(entries[1], AlertEntry::Item(_)));
    }

    #[test]
    fn selected_flow_copies_compact_and_returns_source_recommendation_identity() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cache]);
        let mut state = ListState::default();
        state.select(Some(0));

        let cmd = selected_alert_suggested_command(
            &state,
            &[],
            std::slice::from_ref(&report),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );
        let identity = selected_alert_recommendation_identity(
            &state,
            &[],
            std::slice::from_ref(&report),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(cmd.as_deref(), Some("/compact"));
        assert_eq!(
            identity,
            Some(("%1".into(), "context-pressure: checkpoint".into()))
        );
    }

    #[test]
    fn selected_flow_copy_meta_and_pending_action_use_command_source_kind() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cache]);
        let mut state = ListState::default();
        state.select(Some(0));

        let meta = selected_alert_suggested_command_meta(
            &state,
            &[],
            std::slice::from_ref(&report),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        )
        .expect("flow should expose copy metadata");
        let entries = alert_items_with_command(
            &[],
            std::slice::from_ref(&report),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(meta.1, "/compact");
        assert_eq!(meta.3, SourceKind::Estimated);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "/compact");
        assert_eq!(entries[0].source, SourceKind::Estimated);
    }

    #[test]
    fn flow_visible_keys_fingerprints_and_bulk_hide_count_once() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cache]);
        let now = Instant::now();

        let keys = visible_alert_keys(&[], std::slice::from_ref(&report), &HashMap::new(), now);
        let warning = actionable_alert_keys_for_severity(
            &[],
            std::slice::from_ref(&report),
            &HashMap::new(),
            now,
            Severity::Warning,
        );
        let fingerprints = alert_fingerprints(&[], std::slice::from_ref(&report));

        assert_eq!(keys.len(), 1);
        assert!(keys[0].starts_with("flow|context-recovery|%1|"));
        assert_eq!(warning, keys);
        assert_eq!(fingerprints, keys.into_iter().collect());
    }

    #[test]
    fn related_context_does_not_change_sort_order_or_bulk_count() {
        let warning = rec(
            "quota-pressure: compact soon",
            "warning compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let concern = rec(
            "profile-switch: compact profile",
            "concern compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let reports = vec![base_report(vec![concern.clone(), warning.clone()])];

        let entries = collect_entries(
            &[],
            &reports,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );
        let warning_keys = actionable_alert_keys_for_severity(
            &[],
            &reports,
            &HashMap::new(),
            Instant::now(),
            Severity::Warning,
        );
        let concern_keys = actionable_alert_keys_for_severity(
            &[],
            &reports,
            &HashMap::new(),
            Instant::now(),
            Severity::Concern,
        );

        assert_eq!(entries.len(), 2);
        assert!(entry_title(&entries[0]).contains("Recommendation"));
        assert_eq!(entries[0].severity(), Severity::Warning);
        assert_eq!(entries[1].severity(), Severity::Concern);
        assert_eq!(warning_keys.len(), 1);
        assert_eq!(concern_keys.len(), 1);
    }

    #[test]
    fn hiding_one_related_item_does_not_hide_sibling() {
        let warning = rec(
            "quota-pressure: compact soon",
            "warning compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let concern = rec(
            "profile-switch: compact profile",
            "concern compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let hidden_key = recommendation_key("%1", &warning);
        let hidden = HashMap::from([(hidden_key, Instant::now() - Duration::from_secs(1))]);
        let report = base_report(vec![warning, concern]);

        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &hidden,
            Instant::now(),
        );

        assert_eq!(entries.len(), 1);
        assert!(entry_title(&entries[0]).contains("Recommendation"));
        assert!(
            item_related(&entries[0]).is_none(),
            "single remaining sibling should not claim a group"
        );
    }

    #[test]
    fn related_copy_stays_item_local() {
        let compact = rec(
            "maintenance: compact first",
            "compact first",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let snapshot = rec(
            "maintenance: snapshot first",
            "snapshot first",
            Severity::Warning,
            SourceKind::ProjectCanonical,
            Some("qmonster snapshot"),
            None,
        );
        let report = base_report(vec![compact.clone(), snapshot.clone()]);
        let mut state = ListState::default();
        state.select(Some(0));

        let entries = collect_entries(
            &[],
            std::slice::from_ref(&report),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );
        let cmd = selected_alert_suggested_command(
            &state,
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert!(entries.iter().all(|entry| item_related(entry).is_none()));
        assert_eq!(cmd.as_deref(), Some("/compact"));
    }

    #[test]
    fn alert_filter_matches_included_alert_and_keeps_flow_visible() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cache]);
        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 1);
        assert!(alert_entry_matches_filter(&entries[0], "0.72"));
    }

    #[test]
    fn alert_filter_matches_related_evidence_and_keeps_item_visible() {
        let compact_quota = rec(
            "quota-pressure: compact soon",
            "quota pressure can recover after compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let compact_profile = rec(
            "profile-switch: compact profile",
            "profile switch should happen after compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let report = base_report(vec![compact_quota, compact_profile]);
        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|entry| alert_entry_matches_filter(entry, "profile-switch"))
        );
        assert!(
            entries
                .iter()
                .any(|entry| alert_entry_matches_filter(entry, "same /compact path"))
        );
    }

    #[test]
    fn alert_items_with_command_reports_flow_command_and_pane() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cache]);

        let entries = alert_items_with_command(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "/compact");
        assert_eq!(entries[0].pane_id.as_deref(), Some("%1"));
        assert!(entries[0].title.contains("FLOW Context recovery"));
    }

    #[test]
    fn renders_context_recovery_flow_with_timeline_rail() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let report = base_report(vec![context, cache]);
        let mut state = ListState::default();
        state.select(Some(0));
        let area = Rect::new(0, 0, 96, 18);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[],
            reports: &[report],
            fresh_alerts: &HashSet::new(),
            alert_times: &HashMap::new(),
            hidden_until: &HashMap::new(),
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };

        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(dump.contains("FLOW Context recovery"), "{dump}");
        assert!(
            dump.contains("summary : context pressure led to cache drift"),
            "{dump}"
        );
        assert!(dump.contains("o context pressure detected"), "{dump}");
        assert!(
            dump.contains("| cache drift/discontinuity detected"),
            "{dump}"
        );
        assert!(dump.contains("| then run /compact"), "{dump}");
        assert!(
            dump.contains("included: context-pressure: checkpoint [Estimate]"),
            "{dump}"
        );
        assert!(
            dump.contains(
                "included: cache: drift detected — /compact will let cache rebuild [Heur]"
            ),
            "{dump}"
        );
        assert!(dump.contains("copy    : `/compact`"), "{dump}");
    }

    #[test]
    fn renders_related_context_rail_without_collapsing_alerts() {
        let compact_quota = rec(
            "quota-pressure: compact soon",
            "quota pressure can recover after compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let compact_profile = rec(
            "profile-switch: compact profile",
            "profile switch should happen after compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let report = base_report(vec![compact_quota, compact_profile]);
        let mut state = ListState::default();
        state.select(Some(0));
        let area = Rect::new(0, 0, 100, 18);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[],
            reports: &[report],
            fresh_alerts: &HashSet::new(),
            alert_times: &HashMap::new(),
            hidden_until: &HashMap::new(),
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };

        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(!dump.contains("FLOW Context recovery"), "{dump}");
        assert!(
            dump.contains("related : 2 alerts on %1 · same /compact path"),
            "{dump}"
        );
        assert!(
            dump.contains("rail : [o] quota-pressure [ ] profile-switch"),
            "{dump}"
        );
        assert!(
            dump.contains("included: quota-pressure: compact soon [Estimate]"),
            "{dump}"
        );
        assert!(
            dump.contains("included: profile-switch: compact profile [Qmonster]"),
            "{dump}"
        );
    }

    #[test]
    fn unselected_related_context_omits_evidence_but_keeps_rail() {
        let compact_quota = rec(
            "quota-pressure: compact soon",
            "quota pressure can recover after compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let compact_profile = rec(
            "profile-switch: compact profile",
            "profile switch should happen after compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let notice = SystemNotice {
            title: "startup".into(),
            body: "ready".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
        };
        let report = base_report(vec![compact_quota, compact_profile]);
        let mut state = ListState::default();
        state.select(Some(2));
        let area = Rect::new(0, 0, 100, 18);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[notice],
            reports: &[report],
            fresh_alerts: &HashSet::new(),
            alert_times: &HashMap::new(),
            hidden_until: &HashMap::new(),
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };

        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("related : 2 alerts on %1 · same /compact path"),
            "{dump}"
        );
        assert!(
            dump.contains("rail : [ ] quota-pressure [o] profile-switch"),
            "{dump}"
        );
        assert!(!dump.contains("included: quota-pressure"), "{dump}");
    }

    #[test]
    fn unselected_flow_omits_included_and_copy_detail() {
        let context = rec(
            "context-pressure: checkpoint",
            "context near warning threshold",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            Some("press 's' to snapshot + archive first"),
        );
        let cache = rec(
            "cache: drift detected — /compact will let cache rebuild",
            "cache hit ratio dropped from 0.72 to 0.38",
            Severity::Concern,
            SourceKind::Heuristic,
            None,
            None,
        );
        let notice = SystemNotice {
            title: "startup".into(),
            body: "ready".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
        };
        let report = base_report(vec![context, cache]);
        let mut state = ListState::default();
        state.select(Some(1));
        let area = Rect::new(0, 0, 96, 18);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[notice],
            reports: &[report],
            fresh_alerts: &HashSet::new(),
            alert_times: &HashMap::new(),
            hidden_until: &HashMap::new(),
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };

        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(dump.contains("FLOW Context recovery"), "{dump}");
        assert!(!dump.contains("included: context-pressure"), "{dump}");
        assert!(!dump.contains("press y to copy"), "{dump}");
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
                profile: None,
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
                profile: None,
            },
        ]);
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let items = collect_items(&[], &[rep], &fresh, &times, &hidden, Instant::now());
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn higher_severity_alerts_sort_before_lower_severity_alerts() {
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
            profile: None,
        }]);
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let items = collect_items(&[notice], &[rep], &fresh, &times, &hidden, Instant::now());
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].severity, Severity::Risk);
        assert!(items[0].title.contains("Recommendation"));
    }

    #[test]
    fn fresh_alerts_sort_before_older_alerts_within_same_severity() {
        let strong = Recommendation {
            action: "context-pressure",
            reason: "fresh warning".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let normal = Recommendation {
            action: "notify-input-wait",
            reason: "older warning".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![strong.clone(), normal.clone()]);
        let fresh = HashSet::from([recommendation_key("%1", &strong)]);
        let times = HashMap::from([
            (recommendation_key("%1", &normal), "14:32:10".into()),
            (recommendation_key("%1", &strong), "14:32:05".into()),
        ]);
        let hidden = HashMap::new();
        let items = collect_items(&[], &[rep], &fresh, &times, &hidden, Instant::now());
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].headline, "[Heur] fresh warning");
        assert!(items[0].is_new);
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
            profile: None,
        };
        let body = format_strong_rec_body(&strong, "%1");

        let next_idx = body
            .find("next: press 's' to snapshot + archive now")
            .expect("body must contain literal `next: …snapshot…` segment");
        let run_idx = body
            .find("run: `/compact`")
            .expect("body must contain literal `run: `/compact`` segment");
        assert!(
            next_idx < run_idx,
            "ordering contract: `next:` MUST precede `run:`. body: {body}"
        );
        assert!(
            body.contains(">> CHECKPOINT (%1)"),
            "body must carry the CHECKPOINT slot prefix. got: {body}"
        );
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
            profile: None,
        };
        let body = format_strong_rec_body(&strong, "%1");
        assert!(
            !body.contains("next:"),
            "no next_step → no `next:` segment. got: {body}"
        );
        assert!(
            body.contains("run: `/compact`"),
            "cmd still rendered. got: {body}"
        );
    }

    #[test]
    fn format_recommendation_body_renders_run_for_non_strong_recommendation() {
        let rec = Recommendation {
            action: "archive-preview-suggested",
            reason: "log storm pattern".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: Some(
                "tmux capture-pane -pS -2000 > ~/.qmonster/archive/x.log".into(),
            ),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let body = format_recommendation_body(&rec, "%7");
        assert!(body.contains("%7"));
        assert!(body.contains("run: `tmux capture-pane -pS -2000 > ~/.qmonster/archive/x.log`"));
    }

    #[test]
    fn format_recommendation_body_keeps_next_before_run_for_non_strong_recommendation() {
        let rec = Recommendation {
            action: "auto-memory: route to MDR / CURRENT_STATE",
            reason: "state-critical task detected".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: Some("qmonster --once".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: Some("record in CURRENT_STATE first".into()),
            profile: None,
        };
        let body = format_recommendation_body(&rec, "%2");
        let next_idx = body
            .find("next: record in CURRENT_STATE first")
            .expect("next segment");
        let run_idx = body.find("run: `qmonster --once`").expect("run segment");
        assert!(next_idx < run_idx, "next must precede run. body: {body}");
    }

    #[test]
    fn selected_alert_suggested_command_returns_selected_run_command() {
        let rep = base_report(vec![Recommendation {
            action: "context-pressure",
            reason: "context near threshold".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some(" /compact ".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        }]);
        let mut state = ListState::default();
        state.select(Some(0));

        let cmd = selected_alert_suggested_command(
            &state,
            &[],
            &[rep],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(cmd.as_deref(), Some("/compact"));
    }

    #[test]
    fn selected_alert_suggested_command_returns_none_without_run_command() {
        let rep = base_report(vec![Recommendation {
            action: "notify-input-wait",
            reason: "waiting for input".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        }]);
        let mut state = ListState::default();
        state.select(Some(0));

        let cmd = selected_alert_suggested_command(
            &state,
            &[],
            &[rep],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(cmd, None);
    }

    #[test]
    fn selected_alert_suggested_command_rejects_comment_and_placeholder_commands() {
        let rep = base_report(vec![
            Recommendation {
                action: "comment command",
                reason: "not executable".into(),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
                suggested_command: Some("# edit config/qmonster.toml".into()),
                side_effects: vec![],
                is_strong: false,
                next_step: None,
                profile: None,
            },
            Recommendation {
                action: "placeholder command",
                reason: "not concrete".into(),
                severity: Severity::Concern,
                source_kind: SourceKind::ProjectCanonical,
                suggested_command: Some("git worktree add -b <branch> <path>".into()),
                side_effects: vec![],
                is_strong: false,
                next_step: None,
                profile: None,
            },
        ]);
        let mut state = ListState::default();
        let reports = vec![rep];

        state.select(Some(0));
        assert!(
            selected_alert_suggested_command(
                &state,
                &[],
                &reports,
                &HashSet::new(),
                &HashMap::new(),
                &HashMap::new(),
                Instant::now(),
            )
            .is_none(),
            "comment commands must not be copyable"
        );

        state.select(Some(1));
        assert!(
            selected_alert_suggested_command(
                &state,
                &[],
                &reports,
                &HashSet::new(),
                &HashMap::new(),
                &HashMap::new(),
                Instant::now(),
            )
            .is_none(),
            "placeholder commands must not be copyable"
        );
    }

    #[test]
    fn selected_alert_suggested_command_uses_render_sort_inputs() {
        let old = Recommendation {
            action: "a-old",
            reason: "older warning".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: Some("/old".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let new = Recommendation {
            action: "z-new",
            reason: "newer warning".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: Some("/new".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![old.clone(), new.clone()]);
        let times = HashMap::from([
            (recommendation_key("%1", &old), "14:32:10".into()),
            (recommendation_key("%1", &new), "14:33:45".into()),
        ]);
        let mut state = ListState::default();
        state.select(Some(0));

        let cmd = selected_alert_suggested_command(
            &state,
            &[],
            &[rep],
            &HashSet::new(),
            &times,
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(cmd.as_deref(), Some("/new"));
    }

    #[test]
    fn cross_pane_finding_details_surface_run_command() {
        let mut rep = base_report(vec![]);
        rep.cross_pane_findings.push(CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentMutatingWork,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec!["%2".into()],
            reason: "coordinate edits".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some(
                "git -C /repo worktree add -b main-split /repo-main-split HEAD".into(),
            ),
            paths: Vec::new(),
        });
        let items = collect_items(
            &[],
            &[rep],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(
            items[0].suggested_command.as_deref(),
            Some("git -C /repo worktree add -b main-split /repo-main-split HEAD")
        );
        assert!(
            items[0]
                .details
                .iter()
                .any(|line| line.contains("run") && line.contains("git -C /repo")),
            "cross-pane alert details must render copyable run command: {:?}",
            items[0].details
        );
    }

    #[test]
    fn latest_timestamp_breaks_same_severity_ties_after_newness() {
        let older = Recommendation {
            action: "notify-input-wait",
            reason: "older warning".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let newer = Recommendation {
            action: "log-storm",
            reason: "newer warning".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![older.clone(), newer.clone()]);
        let fresh = HashSet::new();
        let times = HashMap::from([
            (recommendation_key("%1", &older), "14:32:10".into()),
            (recommendation_key("%1", &newer), "14:33:45".into()),
        ]);
        let hidden = HashMap::new();
        let items = collect_items(&[], &[rep], &fresh, &times, &hidden, Instant::now());
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].headline, "[Heur] newer warning");
        assert_eq!(items[0].timestamp, "14:33:45");
    }

    #[test]
    fn cross_pane_findings_beat_plain_recommendations_on_same_priority_tie() {
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
            profile: None,
        }]);
        rep.cross_pane_findings.push(CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentMutatingWork,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec!["%2".into()],
            reason: "concurrent work on /repo".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: None,
            paths: Vec::new(),
        });
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let items = collect_items(&[], &[rep], &fresh, &times, &hidden, Instant::now());
        assert_eq!(items.len(), 2, "one cross-pane finding + one pane alert");
        assert!(items[0].headline.contains("cross-pane"));
    }

    #[test]
    fn actionable_items_beat_system_notices_on_same_priority_tie() {
        let notice = SystemNotice {
            title: "version drift".into(),
            body: "tmux: 3.4 -> 3.5".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProviderOfficial,
        };
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec.clone()]);
        let fresh = HashSet::new();
        let times = HashMap::from([
            (notice_key(&notice), "14:32:10".into()),
            (recommendation_key("%1", &rec), "14:32:10".into()),
        ]);
        let hidden = HashMap::new();
        let items = collect_items(&[notice], &[rep], &fresh, &times, &hidden, Instant::now());
        assert_eq!(items.len(), 2);
        assert!(items[0].title.contains("Recommendation"));
        assert!(items[1].title.contains("System Notice"));
    }

    #[test]
    fn fresh_alert_is_prefixed_with_new_and_timestamp() {
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "pane appears to be waiting for user input".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec.clone()]);
        let key = recommendation_key("%1", &rec);
        let fresh = HashSet::from([key.clone()]);
        let times = HashMap::from([(key, "14:32:10".into())]);
        let hidden = HashMap::new();
        let items = collect_items(&[], &[rep], &fresh, &times, &hidden, Instant::now());
        assert_eq!(items[0].timestamp, "14:32:10");
        assert!(items[0].is_new);
    }

    #[test]
    fn hidden_alert_is_filtered_after_deadline() {
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec.clone()]);
        let key = recommendation_key("%1", &rec);
        let now = Instant::now();
        let hidden = HashMap::from([(key, now - Duration::from_secs(1))]);
        let keys = visible_alert_keys(&[], &[rep], &hidden, now);
        assert!(keys.is_empty());
    }

    #[test]
    fn severity_bulk_chips_ignore_system_notices() {
        let notice = SystemNotice {
            title: "polling recovered".into(),
            body: "tmux ok".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
        };
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec]);
        let items = collect_items(
            &[notice],
            &[rep],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );
        let chips = severity_chips(&items);
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].severity, Severity::Warning);
    }

    #[test]
    fn severity_bulk_chip_uses_partial_marker_when_only_some_are_pending_hide() {
        let rec_a = Recommendation {
            action: "notify-input-wait",
            reason: "waiting a".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rec_b = Recommendation {
            action: "log-storm",
            reason: "waiting b".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec_a.clone(), rec_b.clone()]);
        let pending_key = recommendation_key("%1", &rec_a);
        let hidden = HashMap::from([(pending_key, Instant::now() + Duration::from_secs(9))]);
        let items = collect_items(
            &[],
            &[rep],
            &HashSet::new(),
            &HashMap::new(),
            &hidden,
            Instant::now(),
        );
        let chips = severity_chips(&items);
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].marker_text(), "[-]");
        assert_eq!(chips[0].count_label(), "1/2");
    }

    #[test]
    fn pending_auto_hide_keeps_alert_visible_with_dismiss_text() {
        let item = AlertItem {
            key: "rec|%1|x|W|y".into(),
            timestamp: "14:32:10".into(),
            timestamp_sort_key: sortable_timestamp("14:32:10"),
            severity: Severity::Warning,
            kind: AlertKind::Recommendation,
            title: "Recommendation · %1".into(),
            headline: "[Heur] waiting".into(),
            details: vec![],
            suggested_command: None,
            source_kind: SourceKind::Heuristic,
            color: Color::Yellow,
            is_new: false,
            hide_deadline: Some(Instant::now() + Duration::from_secs(9)),
            related: None,
        };
        let line = dismiss_line_text(&item, Instant::now());
        assert!(line.contains("[x] auto-hide"));
        assert!(line.contains("click undo"));
        assert!(line.contains("Enter/Space undo"));
    }

    #[test]
    fn idle_alert_shows_unchecked_dismiss_marker() {
        let item = AlertItem {
            key: "rec|%1|x|W|y".into(),
            timestamp: "14:32:10".into(),
            timestamp_sort_key: sortable_timestamp("14:32:10"),
            severity: Severity::Warning,
            kind: AlertKind::Recommendation,
            title: "Recommendation · %1".into(),
            headline: "[Heur] waiting".into(),
            details: vec![],
            suggested_command: None,
            source_kind: SourceKind::Heuristic,
            color: Color::Yellow,
            is_new: false,
            hide_deadline: None,
            related: None,
        };
        let line = dismiss_line_text(&item, Instant::now());
        assert!(line.contains("[ ] click hide"));
        assert!(line.contains("Enter/Space hide"));
    }

    #[test]
    fn alert_filter_matches_title_headline_details_and_command() {
        let mut item = AlertItem {
            key: "rec|%1|x|W|y".into(),
            timestamp: "14:32:10".into(),
            timestamp_sort_key: sortable_timestamp("14:32:10"),
            severity: Severity::Warning,
            kind: AlertKind::Recommendation,
            title: "Recommendation · %1".into(),
            headline: "[Heur] api throttled".into(),
            details: vec!["upstream rate limit hit".into()],
            suggested_command: Some("qmonster diag api".into()),
            source_kind: SourceKind::Heuristic,
            color: Color::Yellow,
            is_new: false,
            hide_deadline: None,
            related: None,
        };

        // Empty needle short-circuits to true.
        assert!(alert_item_matches_filter(&item, ""));

        // Title-token match. Caller is expected to pre-lowercase the
        // needle (callers do via `.to_ascii_lowercase()`); the helper
        // matches against lowercased haystacks.
        assert!(alert_item_matches_filter(&item, "recommendation"));

        // Headline-token match.
        assert!(alert_item_matches_filter(&item, "throttled"));

        // Details-token match.
        assert!(alert_item_matches_filter(&item, "rate limit"));

        // suggested_command match.
        assert!(alert_item_matches_filter(&item, "diag"));

        // No match.
        assert!(!alert_item_matches_filter(&item, "snapshot"));

        // Without a command, the rest still matches.
        item.suggested_command = None;
        assert!(alert_item_matches_filter(&item, "throttled"));
        assert!(!alert_item_matches_filter(&item, "diag"));
    }

    #[test]
    fn alert_filter_matches_related_context_fields() {
        let quota = rec(
            "quota-pressure: compact soon",
            "quota pressure can recover after compact",
            Severity::Warning,
            SourceKind::Estimated,
            Some("/compact"),
            None,
        );
        let profile = rec(
            "profile-switch: compact profile",
            "profile switch should happen after compact",
            Severity::Concern,
            SourceKind::ProjectCanonical,
            Some("/compact"),
            None,
        );
        let report = base_report(vec![quota.clone(), profile.clone()]);
        let entries = collect_entries(
            &[],
            &[report],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );

        assert_eq!(entries.len(), 2);
        let first = match &entries[0] {
            AlertEntry::Item(item) => item,
            AlertEntry::Flow(_) => panic!("expected plain alert"),
        };
        let related = first
            .related
            .as_ref()
            .expect("related context should be attached");
        assert!(alert_item_matches_filter(first, "same /compact path"));
        assert!(alert_item_matches_filter(first, "quota-pressure"));
        assert!(alert_item_matches_filter(first, "estimate"));
        assert_eq!(related.total, 2);
        assert_eq!(related.pane_id, "%1");
        assert!(related.connector_label.contains("same /compact path"));
        assert!(
            related
                .evidence
                .iter()
                .any(|e| e.source_label == "Estimate")
        );
        assert!(
            related
                .steps
                .iter()
                .any(|step| step.label.contains("quota-pressure"))
        );
    }

    #[test]
    fn wrap_text_splits_long_alerts_into_multiple_lines() {
        let lines = wrap_text(
            "warning [Official] %1 — very long alert body that should wrap cleanly",
            24,
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.chars().count() <= 24));
    }

    #[test]
    fn wrap_with_prefix_hangs_under_timestamp_column() {
        let lines = wrap_with_prefix(
            "warning [Heur] %1 — long message that wraps",
            28,
            "[14:32:10] ",
            "           ",
        );
        assert!(lines.len() > 1);
        assert!(lines[1].starts_with("           "));
    }

    #[test]
    fn format_prompt_send_proposal_shows_both_keys_when_accept_is_gated_on() {
        // P5-2 render contract (recommend_only / safe_auto path): the
        // operator sees both the accept and dismiss keys so they can
        // either confirm the pending send or explicitly dismiss it.
        // Both actions get audit-logged downstream.
        let line = format_prompt_send_proposal("%3", "/compact", true);
        assert!(line.contains("[proposal]"), "proposal marker: {line}");
        assert!(line.contains("[Qmonster]"), "authority label: {line}");
        assert!(line.contains("%3"), "target pane id visible: {line}");
        assert!(
            line.contains("`/compact`"),
            "slash command in backticks: {line}"
        );
        assert!(line.contains("[p] accept"), "accept key shown: {line}");
        assert!(line.contains("[d] dismiss"), "dismiss key shown: {line}");
    }

    #[test]
    fn format_prompt_send_proposal_hides_accept_key_when_gate_denies() {
        // P5-2 render contract (observe_only path): when the `permit`
        // gate returns false for the proposal, the accept key must
        // disappear so the operator cannot initiate a send that the
        // runner would have blocked anyway. Dismiss stays available so
        // the operator can still log an explicit rejection from any mode.
        let line = format_prompt_send_proposal("%3", "/compact", false);
        assert!(!line.contains("[p] accept"), "accept key hidden: {line}");
        assert!(
            line.contains("send disabled"),
            "operator sees why accept is missing: {line}"
        );
        assert!(
            line.contains("[d] dismiss"),
            "dismiss still available: {line}"
        );
    }

    fn buffer_to_string(buf: &Buffer) -> String {
        let area = buf.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(area.x + x, area.y + y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn selected_alert_shows_inline_copy_line_when_command_present() {
        // Spec §6.5: on the currently selected alert only, when its
        // `suggested_command` is `Some(cmd)`, append `copy: <cmd> →
        // press y to copy` as a dim line below existing detail rows.
        let rec = Recommendation {
            action: "context-pressure",
            reason: "context near threshold".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/clear".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec.clone()]);
        let key = recommendation_key("%1", &rec);
        let times = HashMap::from([(key, "14:32:10".into())]);
        let fresh = HashSet::new();
        let hidden = HashMap::new();

        let mut state = ListState::default();
        state.select(Some(0));
        let area = Rect::new(0, 0, 80, 16);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[],
            reports: &[rep],
            fresh_alerts: &fresh,
            alert_times: &times,
            hidden_until: &hidden,
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };
        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("press y to copy"),
            "selected alert with suggested_command must render copy line: {dump}"
        );
        assert!(
            dump.contains("/clear"),
            "selected alert must render the actual command: {dump}"
        );
    }

    #[test]
    fn alert_help_topic_at_row_maps_header_dismiss_summary_detail_and_copy() {
        use crate::ui::help_glossary::HelpTopic;

        let rec = Recommendation {
            action: "context-pressure",
            reason: "context near threshold".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/clear".into()),
            side_effects: vec!["will drop visible transcript".into()],
            is_strong: false,
            next_step: Some("compact context".into()),
            profile: None,
        };
        let reports = vec![base_report(vec![rec])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let now = Instant::now();
        let mut state = ListState::default();
        state.select(Some(0));
        let view = || AlertView {
            notices: &[],
            reports: &reports,
            fresh_alerts: &fresh,
            alert_times: &times,
            hidden_until: &hidden,
            now,
            target_label: "all",
            focused: true,
            filter: None,
        };

        assert_eq!(
            alert_help_topic_at_row(&state, view(), 100, 0),
            Some(HelpTopic::AlertHeader)
        );
        assert_eq!(
            alert_help_topic_at_row(&state, view(), 100, 1),
            Some(HelpTopic::AlertDismiss)
        );
        assert_eq!(
            alert_help_topic_at_row(&state, view(), 100, 2),
            Some(HelpTopic::AlertSummary)
        );
        assert_eq!(
            alert_help_topic_at_row(&state, view(), 100, 3),
            Some(HelpTopic::AlertDetail)
        );

        let item = collect_items(&[], &reports, &fresh, &times, &hidden, now)
            .into_iter()
            .next()
            .expect("item");
        let copy_row = alert_item_lines(&item, 100, now, true)
            .iter()
            .position(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("press y to copy"))
            })
            .expect("copy row") as u16;
        assert_eq!(
            alert_help_topic_at_row(&state, view(), 100, copy_row),
            Some(HelpTopic::AlertCopy)
        );
    }

    #[test]
    fn alert_title_carries_copy_chip_when_command_present() {
        // v1.39 surface A: alert title carries a `★y` chip when its
        // `suggested_command` is `Some(_)` so operators can spot
        // copyable items in the alert queue without selecting each
        // one. Mirrors the `★p` chip on pane cards.
        let rec = Recommendation {
            action: "context-pressure",
            reason: "context near threshold".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/clear".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec.clone()]);
        let key = recommendation_key("%1", &rec);
        let times = HashMap::from([(key, "14:32:10".into())]);
        let fresh = HashSet::new();
        let hidden = HashMap::new();

        let mut state = ListState::default();
        // Don't select; the chip is on the title independent of selection.
        let area = Rect::new(0, 0, 80, 16);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[],
            reports: &[rep],
            fresh_alerts: &fresh,
            alert_times: &times,
            hidden_until: &hidden,
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };
        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("\u{2605}y"),
            "alert title should carry ★y chip when suggested_command is Some; got: {dump}"
        );
    }

    #[test]
    fn alert_title_omits_chip_when_no_command() {
        // v1.39 surface A: no suggested_command => no chip, so the
        // chip never lights up for an alert the operator can't copy
        // a runnable command from.
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting for input".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec.clone()]);
        let key = recommendation_key("%1", &rec);
        let times = HashMap::from([(key, "14:32:10".into())]);
        let fresh = HashSet::new();
        let hidden = HashMap::new();

        let mut state = ListState::default();
        let area = Rect::new(0, 0, 80, 16);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[],
            reports: &[rep],
            fresh_alerts: &fresh,
            alert_times: &times,
            hidden_until: &hidden,
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };
        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(
            !dump.contains("\u{2605}y"),
            "alert title MUST NOT carry ★y chip when suggested_command is None; got: {dump}"
        );
    }

    #[test]
    fn selected_alert_no_copy_line_when_no_command() {
        // Spec §6.5: when the selected alert has no `suggested_command`,
        // no inline `copy:` line is rendered (matches the "nothing to
        // copy" SystemNotice path in copy_selected_alert_command_to_clipboard).
        let rec = Recommendation {
            action: "notify-input-wait",
            reason: "waiting for input".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep = base_report(vec![rec.clone()]);
        let key = recommendation_key("%1", &rec);
        let times = HashMap::from([(key, "14:32:10".into())]);
        let fresh = HashSet::new();
        let hidden = HashMap::new();

        let mut state = ListState::default();
        state.select(Some(0));
        let area = Rect::new(0, 0, 80, 16);
        let mut buf = Buffer::empty(area);
        let view = AlertView {
            notices: &[],
            reports: &[rep],
            fresh_alerts: &fresh,
            alert_times: &times,
            hidden_until: &hidden,
            now: Instant::now(),
            target_label: "all",
            focused: true,
            filter: None,
        };
        render_alerts(area, &mut buf, &mut state, view);

        let dump = buffer_to_string(&buf);
        assert!(
            !dump.contains("press y to copy"),
            "selected alert without suggested_command must NOT render copy line: {dump}"
        );
    }
}
