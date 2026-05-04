//! Phase v1.39 — Pending Actions overlay (`a` key).
//!
//! Lists every pane with a pending prompt-send proposal AND every
//! alert with a `suggested_command`, in one scrollable modal. Enter
//! jumps to the selected item and opens the Action Explainer modal
//! so operators can dispatch from anywhere without navigating.
//!
//! Pure state + render. The tui_loop key + mouse handlers live in
//! `app::pending_actions_overlay`. Item collection is shared between
//! the overlay and (potentially) future surfaces, so it lives here
//! to avoid a circular dependency with `ui::alerts`.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::ui::dashboard::close_button_rect;
use crate::ui::theme;

/// Open/closed state + selection cursor for the Pending Actions
/// overlay. Pure — mirrors the discipline of
/// `ScrollModalState` / `ActionExplainModal`.
#[derive(Debug, Default, Clone)]
pub struct PendingActionsOverlay {
    open: bool,
    selected: usize,
}

impl PendingActionsOverlay {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.selected = 0;
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    /// Move the cursor down, clamping to the last item. `total = 0`
    /// keeps the cursor at 0 (no items, no selection drift).
    pub fn select_next(&mut self, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected.saturating_add(1);
        self.selected = next.min(total.saturating_sub(1));
    }
    /// Move the cursor up, clamping to 0.
    pub fn select_prev(&mut self, _total: usize) {
        self.selected = self.selected.saturating_sub(1);
    }
}

/// One actionable item the overlay lists. `Proposal` matches a pane
/// carrying a `PromptSendProposed` effect (operator presses `p`/`d`);
/// `Copy` matches an alert with a `suggested_command` (operator
/// presses `y`). Index slots reference the live `reports` /
/// `alert_items` lists at collection time so the caller can drive
/// `pane_state.select(...)` / `alert_state.select(...)`.
#[derive(Debug, Clone)]
pub enum PendingItem {
    Proposal {
        pane_idx: usize,
        pane_label: String,
        slash_command: String,
        severity: Option<Severity>,
        source: SourceKind,
        proposal_id: String,
        target_pane_id: String,
    },
    Copy {
        alert_idx: usize,
        command: String,
        alert_title: String,
        severity: Severity,
        source: SourceKind,
        pane_idx: Option<usize>,
    },
}

impl PendingItem {
    pub fn severity(&self) -> Option<Severity> {
        match self {
            PendingItem::Proposal { severity, .. } => *severity,
            PendingItem::Copy { severity, .. } => Some(*severity),
        }
    }
    pub fn kind_letter(&self) -> &'static str {
        match self {
            PendingItem::Proposal { .. } => "p",
            PendingItem::Copy { .. } => "y",
        }
    }
    pub fn command(&self) -> &str {
        match self {
            PendingItem::Proposal { slash_command, .. } => slash_command.as_str(),
            PendingItem::Copy { command, .. } => command.as_str(),
        }
    }
    pub fn context(&self) -> &str {
        match self {
            PendingItem::Proposal { pane_label, .. } => pane_label.as_str(),
            PendingItem::Copy { alert_title, .. } => alert_title.as_str(),
        }
    }
}

/// Collect every pending action across panes + alerts. Proposals
/// come first (so the operator's eye lands on the more authoritative
/// dispatch surface — `p`/`d` writes to a pane), followed by
/// copyable alerts. Within each group, items keep the source order
/// so the cursor mapping stays predictable across polls.
pub fn collect_pending_items(
    reports: &[PaneReport],
    notices: &[SystemNotice],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<PendingItem> {
    let mut out: Vec<PendingItem> = Vec::new();

    // Proposals — one per pane that carries a PromptSendProposed
    // effect. We use first_prompt_send_proposal_full to match the
    // dispatch path (lex-first proposal_id). This keeps the overlay
    // showing the same proposal that `p` would accept.
    for (pane_idx, report) in reports.iter().enumerate() {
        if let Some((target_pane_id, slash_command, proposal_id)) =
            crate::app::prompt_send_actions::first_prompt_send_proposal_full(report)
        {
            let pane_label = pane_label(report);
            let (severity, source) = pane_proposal_severity_source(report);
            out.push(PendingItem::Proposal {
                pane_idx,
                pane_label,
                slash_command,
                severity,
                source,
                proposal_id,
                target_pane_id,
            });
        }
    }

    // Copyable alerts — every alert item the operator could `y` on.
    let alert_with_meta = crate::ui::alerts::alert_items_with_command(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    );
    for entry in alert_with_meta {
        // Resolve a pane_idx for jump targets so the operator can
        // both jump to the alert AND see the underlying pane card.
        let pane_idx = entry
            .pane_id
            .as_deref()
            .and_then(|pid| reports.iter().position(|r| r.pane_id == pid));
        out.push(PendingItem::Copy {
            alert_idx: entry.alert_idx,
            command: entry.command,
            alert_title: entry.title,
            severity: entry.severity,
            source: entry.source,
            pane_idx,
        });
    }

    out
}

fn pane_proposal_severity_source(report: &PaneReport) -> (Option<Severity>, SourceKind) {
    // Use the highest-severity rec to color the chip in the overlay.
    // Fall back to ProjectCanonical (Qmonster) for the source slot
    // since the proposal is engine-emitted.
    let sev = report.recommendations.iter().map(|r| r.severity).max();
    let source = report
        .recommendations
        .first()
        .map(|r| r.source_kind)
        .unwrap_or(SourceKind::ProjectCanonical);
    (sev, source)
}

fn pane_label(report: &PaneReport) -> String {
    use crate::domain::identity::{Provider, Role};
    let id = &report.identity.identity;
    let provider = match id.provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
        Provider::Gemini => "gemini",
        Provider::Qmonster => "qmonster",
        Provider::Unknown => "?",
    };
    let role = match id.role {
        Role::Main => "main",
        Role::Review => "review",
        Role::Research => "research",
        Role::Monitor => "monitor",
        Role::Unknown => "?",
    };
    format!(
        "{provider}:{}:{role} \u{B7} {}",
        id.instance, report.pane_id
    )
}

/// Centered ~70% × 60% modal area, clamped to a 60×16 minimum so
/// the overlay renders predictably on small terminals. Mirrors the
/// Action Explainer geometry so the two modals feel consistent.
pub(crate) fn pending_actions_modal_area(viewport: Rect) -> Rect {
    let width = (viewport.width * 70 / 100).max(60).min(viewport.width);
    let height = (viewport.height * 60 / 100).max(16).min(viewport.height);
    let x = viewport.x + viewport.width.saturating_sub(width) / 2;
    let y = viewport.y + viewport.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

pub fn render_pending_actions_modal(
    frame: &mut Frame<'_>,
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) {
    let viewport = frame.area();
    let area = pending_actions_modal_area(viewport);
    frame.render_widget(Clear, area);

    let title = format!(
        "Pending Actions \u{B7} {} pending \u{B7} a again to close",
        items.len()
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));

    let lines = pending_actions_lines(overlay, items);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        close_button_rect(area),
    );
}

/// Build the styled body lines for the modal. Each item renders as
/// `<cursor> [p|y] <severity> · <command> · <context>` so the
/// operator sees what kind of action it is, the urgency, the actual
/// command, and where it lives. Cursor is `▶` for the selected row,
/// space otherwise.
pub fn pending_actions_lines(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    if items.is_empty() {
        out.push(Line::from(""));
        out.push(Line::styled(
            "  No pending actions.".to_string(),
            Style::default().fg(theme::TEXT_DIM),
        ));
        out.push(Line::from(""));
        out.push(hint_line());
        return out;
    }
    out.push(Line::from(""));
    let selected = overlay.selected().min(items.len().saturating_sub(1));
    for (idx, item) in items.iter().enumerate() {
        let cursor = if idx == selected { "\u{25B6} " } else { "  " };
        let kind = item.kind_letter();
        let sev_color = item
            .severity()
            .map(theme::severity_color)
            .unwrap_or(theme::TEXT_PRIMARY);
        let sev_label = item.severity().map(severity_label).unwrap_or("\u{2014}");
        let spans: Vec<Span<'static>> = vec![
            Span::raw(cursor.to_string()),
            Span::styled(
                format!("[{kind}]"),
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" {sev_label} "),
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .bg(sev_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" \u{B7} "),
            Span::styled(
                format!("`{}`", item.command()),
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" \u{B7} "),
            Span::styled(
                item.context().to_string(),
                Style::default().fg(theme::TEXT_DIM),
            ),
        ];
        out.push(Line::from(spans));
    }
    out.push(Line::from(""));
    out.push(hint_line());
    out
}

fn hint_line() -> Line<'static> {
    Line::styled(
        "  Enter open explainer \u{B7} \u{2191}/\u{2193} navigate \u{B7} a close \u{B7} Esc close \u{B7} click [x] close".to_string(),
        Style::default().fg(theme::TEXT_DIM),
    )
}

fn severity_label(sev: Severity) -> &'static str {
    match sev {
        Severity::Safe => "SAFE",
        Severity::Good => "GOOD",
        Severity::Concern => "CONCERN",
        Severity::Warning => "WARNING",
        Severity::Risk => "RISK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::recommendation::{Recommendation, RequestedEffect};
    use crate::domain::signal::SignalSet;

    fn fixture_report(pane_id: &str, with_proposal: bool, recs: Vec<Recommendation>) -> PaneReport {
        let mut effects = Vec::new();
        if with_proposal {
            effects.push(RequestedEffect::PromptSendProposed {
                target_pane_id: pane_id.to_string(),
                slash_command: "/compact".into(),
                proposal_id: format!("{pane_id}:/compact"),
            });
        }
        PaneReport {
            pane_id: pane_id.to_string(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: pane_id.to_string(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: recs,
            effects,
            dead: false,
            current_path: "/repo".into(),
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(),
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn open_close_round_trip() {
        let mut overlay = PendingActionsOverlay::new();
        assert!(!overlay.is_open());
        overlay.open();
        assert!(overlay.is_open());
        assert_eq!(overlay.selected(), 0);
        overlay.close();
        assert!(!overlay.is_open());
        assert_eq!(overlay.selected(), 0);
    }

    #[test]
    fn select_clamps_to_last_item() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.select_next(3);
        overlay.select_next(3);
        overlay.select_next(3);
        // Already at the last index; further next must NOT push past.
        overlay.select_next(3);
        assert_eq!(overlay.selected(), 2, "must clamp at total - 1");
        overlay.select_prev(3);
        overlay.select_prev(3);
        overlay.select_prev(3);
        // Prev at 0 must stay at 0.
        overlay.select_prev(3);
        assert_eq!(overlay.selected(), 0, "must clamp at 0");
    }

    #[test]
    fn select_total_zero_keeps_cursor_at_zero() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.select_next(0);
        assert_eq!(overlay.selected(), 0);
        overlay.select_prev(0);
        assert_eq!(overlay.selected(), 0);
    }

    #[test]
    fn collect_pending_items_groups_proposals_then_copies() {
        // Pane 0: pending proposal. Pane 1: warning rec with run command.
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let rec_with_cmd = Recommendation {
            action: "context-pressure",
            reason: "near limit".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/clear".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep0 = fixture_report("%1", true, vec![]);
        let rep1 = fixture_report("%2", false, vec![rec_with_cmd]);
        let reports = vec![rep0, rep1];
        let items = collect_pending_items(
            &reports,
            &[],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );
        assert_eq!(items.len(), 2, "one proposal + one copyable: {items:?}");
        assert!(matches!(items[0], PendingItem::Proposal { .. }));
        assert!(matches!(items[1], PendingItem::Copy { .. }));
    }

    #[test]
    fn collect_pending_items_skips_alerts_without_run_command() {
        // A recommendation with no suggested_command must not appear
        // in the overlay. Operator can't `y`-copy it anyway.
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let rec_no_cmd = Recommendation {
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
        let rep = fixture_report("%1", false, vec![rec_no_cmd]);
        let items = collect_pending_items(
            &[rep],
            &[],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );
        assert!(items.is_empty(), "no copyable alerts; got {items:?}");
    }

    #[test]
    fn pending_actions_overlay_renders_proposal_and_copy_rows() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let rec_with_cmd = Recommendation {
            action: "context-pressure",
            reason: "near limit".into(),
            severity: Severity::Risk,
            source_kind: SourceKind::Estimated,
            suggested_command: Some("/clear".into()),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        let rep0 = fixture_report("%1", true, vec![]);
        let rep1 = fixture_report("%2", false, vec![rec_with_cmd]);
        let reports = vec![rep0, rep1];
        let items = collect_pending_items(
            &reports,
            &[],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            Instant::now(),
        );
        let overlay = PendingActionsOverlay::new();
        let lines = pending_actions_lines(&overlay, &items);
        let dump: String = lines.iter().map(|l| line_text(l) + "\n").collect();
        assert!(
            dump.contains("[p]"),
            "modal must label proposal rows: {dump}"
        );
        assert!(dump.contains("[y]"), "modal must label copy rows: {dump}");
        assert!(
            dump.contains("/compact"),
            "modal must show proposal command: {dump}"
        );
        assert!(
            dump.contains("/clear"),
            "modal must show copy command: {dump}"
        );
        assert!(
            dump.contains("Enter open explainer"),
            "modal must show hint row: {dump}"
        );
    }

    #[test]
    fn empty_modal_renders_no_pending_actions_message() {
        let overlay = PendingActionsOverlay::new();
        let lines = pending_actions_lines(&overlay, &[]);
        let dump: String = lines.iter().map(|l| line_text(l) + "\n").collect();
        assert!(
            dump.contains("No pending actions"),
            "empty state must render explanatory message: {dump}"
        );
        assert!(
            dump.contains("Enter open explainer"),
            "hint row stays even when empty: {dump}"
        );
    }
}
