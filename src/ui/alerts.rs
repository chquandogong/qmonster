use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
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
    match (
        next_step.filter(|s| !s.is_empty()),
        suggested_command.filter(|s| !s.is_empty()),
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
}

pub struct AlertMouseHit {
    pub index: usize,
    pub dismiss: bool,
}

/// Top-of-screen alert queue. Sorts alerts by operational priority so
/// the first row answers "what should I look at first?" instead of
/// replaying discovery order.
pub fn render_alerts(area: Rect, buf: &mut Buffer, state: &mut ListState, view: AlertView<'_>) {
    let items = collect_items(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    let new_count = items.iter().filter(|item| item.is_new).count();
    let pending_hide_count = items
        .iter()
        .filter(|item| item.hide_deadline.is_some())
        .count();
    let title = format!(
        "Alerts · target {} || visible:{} · new:{} · auto-hide:{}",
        view.target_label,
        items.len(),
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
    Paragraph::new(bulk_hide_line(&items))
        .style(Style::default().fg(theme::TEXT_DIM))
        .render(bulk_area, buf);
    if items.is_empty() {
        Paragraph::new("no alerts")
            .style(Style::default().fg(theme::TEXT_DIM))
            .render(list_area, buf);
    } else if list_area.height > 0 {
        sync_list_selection(state, items.len());
        let item_width = list_width(inner.width);
        let alert_items: Vec<ListItem<'static>> = items
            .iter()
            .map(|item| alert_list_item(item, item_width, view.now))
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
    let items = collect_items(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    severity_chips(&items)
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
    let items = collect_items(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    let mut remaining = row;
    for (idx, item) in items.iter().enumerate().skip(state.offset()) {
        let dismiss_height = dismiss_line_count(item, width, view.now) as u16;
        let height = alert_item_lines(item, width, view.now).len() as u16;
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

pub fn alert_count(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> usize {
    collect_items(
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
    collect_items(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    )
    .into_iter()
    .map(|item| item.key)
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
    collect_items(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    )
    .get(idx)
    .and_then(|item| item.suggested_command.clone())
}

pub fn actionable_alert_keys_for_severity(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
    severity: Severity,
) -> Vec<String> {
    collect_items(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    )
    .into_iter()
    .filter(|item| item.kind.is_actionable() && item.severity == severity)
    .map(|item| item.key)
    .collect()
}

pub fn pending_auto_hide_count(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> usize {
    collect_items(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        hidden_until,
        now,
    )
    .into_iter()
    .filter(|item| item.hide_deadline.is_some())
    .count()
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
    let mut out = HashSet::new();
    for notice in notices {
        out.insert(notice_key(notice));
    }
    for report in reports {
        for rec in &report.recommendations {
            out.insert(recommendation_key(&report.pane_id, rec));
        }
        for finding in &report.cross_pane_findings {
            out.insert(finding_key(finding));
        }
    }
    out
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
    color: Color,
    is_new: bool,
    hide_deadline: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertKind {
    SystemNotice,
    Checkpoint,
    CrossPane,
    Recommendation,
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
            color,
            is_new,
            hide_deadline,
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
                color,
                is_new,
                hide_deadline,
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
                color,
                is_new,
                hide_deadline,
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
                color,
                is_new,
                hide_deadline,
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

fn non_empty_command(command: Option<&str>) -> Option<String> {
    command
        .map(str::trim)
        .filter(|cmd| !cmd.is_empty())
        .map(str::to_string)
}

fn bulk_hide_line(items: &[AlertItem]) -> Line<'static> {
    let chips = severity_chips(items);
    let mut spans = vec![Span::styled(
        BULK_HIDE_PREFIX.to_string(),
        Style::default()
            .fg(theme::TEXT_DIM)
            .add_modifier(Modifier::BOLD),
    )];
    if chips.is_empty() {
        spans.push(Span::styled(
            "actionable alerts only".to_string(),
            Style::default().fg(theme::TEXT_DIM),
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
            Style::default().fg(theme::TEXT_PRIMARY),
        ));
    }
    Line::from(spans)
}

fn severity_chips(items: &[AlertItem]) -> Vec<SeverityChip> {
    let mut chips = Vec::new();
    let mut start_col = BULK_HIDE_PREFIX.chars().count() as u16;
    for severity in [
        Severity::Risk,
        Severity::Warning,
        Severity::Concern,
        Severity::Good,
        Severity::Safe,
    ] {
        let matching: Vec<&AlertItem> = items
            .iter()
            .filter(|item| item.kind.is_actionable() && item.severity == severity)
            .collect();
        if matching.is_empty() {
            continue;
        }
        let total = matching.len();
        let pending = matching
            .iter()
            .filter(|item| item.hide_deadline.is_some())
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

fn list_width(inner_width: u16) -> usize {
    inner_width.saturating_sub(3) as usize
}

fn alert_list_item(item: &AlertItem, width: usize, now: Instant) -> ListItem<'static> {
    ListItem::new(alert_item_lines(item, width, now)).style(alert_style(item.color, item.is_new))
}

fn alert_item_lines(item: &AlertItem, width: usize, now: Instant) -> Vec<Line<'static>> {
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
    lines.push(Line::styled(
        format!(
            "{}{}",
            continuation,
            "─".repeat(width.saturating_sub(continuation.chars().count()).max(8))
        ),
        Style::default().fg(theme::TEXT_DIM),
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

fn title_line(item: &AlertItem, prefix: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_string())];
    if item.is_new {
        spans.push(Span::styled(
            "NEW ",
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .bg(theme::BADGE_BG)
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
    Line::from(spans)
}

fn alert_style(color: Color, is_new: bool) -> Style {
    let style = Style::default().fg(color);
    if is_new {
        style.bg(theme::BADGE_BG).add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn highlight_style(focused: bool) -> Style {
    let style = Style::default().fg(theme::TEXT_PRIMARY);
    if focused {
        style.bg(theme::BADGE_BG).add_modifier(Modifier::BOLD)
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
    format!("{label:<DETAIL_LABEL_WIDTH$}: {value}")
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
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
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
            suggested_command: Some("# config-edit ...".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: Some("record in CURRENT_STATE first".into()),
            profile: None,
        };
        let body = format_recommendation_body(&rec, "%2");
        let next_idx = body
            .find("next: record in CURRENT_STATE first")
            .expect("next segment");
        let run_idx = body.find("run: `# config-edit ...`").expect("run segment");
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
            suggested_command: Some("# coordinate via research pane".into()),
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
            Some("# coordinate via research pane")
        );
        assert!(
            items[0]
                .details
                .iter()
                .any(|line| line.contains("run") && line.contains("coordinate via research pane")),
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
            color: Color::Yellow,
            is_new: false,
            hide_deadline: Some(Instant::now() + Duration::from_secs(9)),
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
            color: Color::Yellow,
            is_new: false,
            hide_deadline: None,
        };
        let line = dismiss_line_text(&item, Instant::now());
        assert!(line.contains("[ ] click hide"));
        assert!(line.contains("Enter/Space hide"));
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
        // P5-2 render contract (observe_only path — Gemini UX TODO):
        // when the `permit` gate returns false for the proposal, the
        // accept key must disappear so the operator cannot initiate a
        // send that the runner would have blocked anyway. Dismiss
        // stays available so the operator can still log an explicit
        // rejection from any mode.
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
}
