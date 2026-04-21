use std::collections::{HashMap, HashSet};

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::recommendation::{CrossPaneFinding, Recommendation, Severity};
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

const DETAIL_LABEL_WIDTH: usize = 8;

pub struct AlertView<'a> {
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub target_label: &'a str,
    pub focused: bool,
}

/// Top-of-screen alert queue. Renders system notices first, then
/// per-pane recommendations.
pub fn render_alerts(area: Rect, buf: &mut Buffer, state: &mut ListState, view: AlertView<'_>) {
    let items = collect_items(
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
    );
    let new_count = items.iter().filter(|item| item.is_new).count();
    let title = format!(
        "Alerts · target {} · {new_count} new / {} total",
        view.target_label,
        items.len()
    );
    if items.is_empty() {
        Paragraph::new("no alerts")
            .style(Style::default().fg(theme::TEXT_DIM))
            .block(block(&title, view.focused))
            .render(area, buf);
    } else {
        sync_list_selection(state, items.len());
        let wrap_width = area.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });
        let alert_items: Vec<ListItem<'static>> = items
            .iter()
            .map(|item| alert_list_item(item, wrap_width.width.saturating_sub(2) as usize))
            .collect();
        StatefulWidget::render(
            List::new(alert_items)
                .block(block(&title, view.focused))
                .highlight_style(highlight_style(view.focused))
                .highlight_symbol("▶ "),
            area,
            buf,
            state,
        );
    }
}

pub fn alert_count(notices: &[SystemNotice], reports: &[PaneReport]) -> usize {
    notices.len()
        + reports
            .iter()
            .map(|report| report.recommendations.len() + report.cross_pane_findings.len())
            .sum::<usize>()
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
    timestamp: String,
    severity: Severity,
    title: String,
    headline: String,
    details: Vec<String>,
    color: Color,
    is_new: bool,
}

fn collect_items(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
) -> Vec<AlertItem> {
    let mut out = Vec::new();
    // 1. System notices.
    for n in notices {
        let badge = source_kind_label(n.source_kind);
        let color = theme::severity_color(n.severity);
        let key = notice_key(n);
        let timestamp = alert_timestamp(&key, alert_times);
        out.push(AlertItem {
            timestamp,
            severity: n.severity,
            title: format!("System Notice · {}", n.title),
            headline: format!("[{badge}] {}", n.body),
            details: vec![],
            color,
            is_new: fresh_alerts.contains(&key),
        });
    }
    // 2. Strong recommendations (G-7 checkpoint UX).
    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| r.is_strong) {
            let color = theme::severity_color(rec.severity);
            let key = recommendation_key(&rep.pane_id, rec);
            let timestamp = alert_timestamp(&key, alert_times);
            out.push(AlertItem {
                timestamp,
                severity: rec.severity,
                title: format!("Checkpoint · {}", rep.pane_id),
                headline: format!("[{}] {}", source_kind_label(rec.source_kind), rec.reason),
                details: recommendation_detail_lines(rec),
                color,
                is_new: fresh_alerts.contains(&key),
            });
        }
    }
    // 3. Cross-pane findings.
    for rep in reports {
        for f in &rep.cross_pane_findings {
            let color = theme::severity_color(f.severity);
            let key = finding_key(f);
            let timestamp = alert_timestamp(&key, alert_times);
            out.push(AlertItem {
                timestamp,
                severity: f.severity,
                title: format!("Cross-Pane · {}", f.anchor_pane_id),
                headline: format!(
                    "[{}] cross-pane — {}",
                    source_kind_label(f.source_kind),
                    f.reason,
                ),
                details: vec![
                    aligned_detail("anchor", &f.anchor_pane_id),
                    aligned_detail("others", &f.other_pane_ids.join(", ")),
                ],
                color,
                is_new: fresh_alerts.contains(&key),
            });
        }
    }
    // 4. Per-pane non-strong recommendations.
    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| !r.is_strong) {
            let color = theme::severity_color(rec.severity);
            let key = recommendation_key(&rep.pane_id, rec);
            let timestamp = alert_timestamp(&key, alert_times);
            out.push(AlertItem {
                timestamp,
                severity: rec.severity,
                title: format!("Recommendation · {}", rep.pane_id),
                headline: format!("[{}] {}", source_kind_label(rec.source_kind), rec.reason),
                details: recommendation_detail_lines(rec),
                color,
                is_new: fresh_alerts.contains(&key),
            });
        }
    }
    out
}

fn alert_list_item(item: &AlertItem, width: usize) -> ListItem<'static> {
    let prefix = format!("[{}] ", item.timestamp);
    let continuation = " ".repeat(prefix.chars().count());
    let mut lines: Vec<Line<'static>> = vec![title_line(item, &prefix)];
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
    ListItem::new(lines).style(alert_style(item.color, item.is_new))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Recommendation;
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
        let items = collect_items(&[], &[rep], &fresh, &times);
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
            profile: None,
        }]);
        let fresh = HashSet::new();
        let times = HashMap::new();
        let items = collect_items(&[notice], &[rep], &fresh, &times);
        assert_eq!(items.len(), 2);
        assert!(items[0].title.contains("System Notice"));
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
            profile: None,
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
            profile: None,
        };
        let rep = base_report(vec![strong, normal]);
        let fresh = HashSet::new();
        let times = HashMap::new();
        let items = collect_items(&[], &[rep], &fresh, &times);
        // Two items: strong-rec line + normal-rec line. Strong appears first.
        assert_eq!(items.len(), 2);
        assert!(items[0].title.contains("Checkpoint"));
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
        let items = collect_items(&[], &[rep], &fresh, &times);
        assert_eq!(items.len(), 2, "one cross-pane finding + one pane alert");
        assert!(items[0].headline.contains("cross-pane"));
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
        let items = collect_items(&[], &[rep], &fresh, &times);
        assert_eq!(items[0].timestamp, "14:32:10");
        assert!(items[0].is_new);
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
}
