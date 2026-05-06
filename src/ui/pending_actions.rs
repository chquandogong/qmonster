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

use std::collections::{BTreeSet, HashMap, HashSet};
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
    multi_selected: BTreeSet<String>,
}

impl PendingActionsOverlay {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
        // multi_selected stays empty (default already empty); explicit no-op.
    }
    pub fn close(&mut self) {
        self.open = false;
        self.selected = 0;
        self.multi_selected.clear();
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn set_selected(&mut self, idx: usize) {
        self.selected = idx;
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

    // --- multi-select ----------------------------------------------------

    pub fn multi_len(&self) -> usize {
        self.multi_selected.len()
    }
    pub fn multi_contains(&self, key: &str) -> bool {
        self.multi_selected.contains(key)
    }
    pub fn multi_keys(&self) -> impl Iterator<Item = &String> {
        self.multi_selected.iter()
    }
    pub fn clear_multi(&mut self) {
        self.multi_selected.clear();
    }
    pub fn toggle_multi(&mut self, item: &PendingItem) {
        let key = pending_item_key(item);
        if !self.multi_selected.remove(&key) {
            self.multi_selected.insert(key);
        }
    }

    /// Toggle all proposal items: select-all if not all are selected, otherwise clear-all.
    pub fn toggle_group_proposals(&mut self, items: &[PendingItem]) {
        self.toggle_group_filtered(items, |it| matches!(it, PendingItem::Proposal { .. }));
    }
    /// Toggle all alert (copy) items.
    pub fn toggle_group_alerts(&mut self, items: &[PendingItem]) {
        self.toggle_group_filtered(items, |it| matches!(it, PendingItem::Copy { .. }));
    }
    /// Toggle every item in the list.
    pub fn toggle_group_all(&mut self, items: &[PendingItem]) {
        self.toggle_group_filtered(items, |_| true);
    }

    fn toggle_group_filtered(
        &mut self,
        items: &[PendingItem],
        filter: impl Fn(&PendingItem) -> bool,
    ) {
        let group_keys: Vec<String> = items
            .iter()
            .filter(|it| filter(it))
            .map(pending_item_key)
            .collect();
        if group_keys.is_empty() {
            return;
        }
        let all_selected = group_keys.iter().all(|k| self.multi_selected.contains(k));
        if all_selected {
            for k in &group_keys {
                self.multi_selected.remove(k);
            }
        } else {
            for k in group_keys {
                self.multi_selected.insert(k);
            }
        }
    }

    /// Drop multi-select keys whose item is not in `items`.
    pub fn prune_to(&mut self, items: &[PendingItem]) {
        let live: BTreeSet<String> = items.iter().map(pending_item_key).collect();
        self.multi_selected.retain(|k| live.contains(k));
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

/// Stable string key for a `PendingItem`, used to track multi-select
/// across polling refreshes. `\u{1F}` (unit separator) prevents `:`
/// collisions in alert titles that contain colons.
pub fn pending_item_key(item: &PendingItem) -> String {
    match item {
        PendingItem::Proposal { proposal_id, .. } => format!("p:{proposal_id}"),
        PendingItem::Copy {
            alert_title,
            command,
            ..
        } => format!("y:{alert_title}\u{1F}{command}"),
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

pub(crate) fn pending_actions_modal_area(viewport: Rect) -> Rect {
    let width = (viewport.width * 80 / 100).max(72).min(viewport.width);
    let height = (viewport.height * 65 / 100).max(20).min(viewport.height);
    let x = viewport.x + viewport.width.saturating_sub(width) / 2;
    let y = viewport.y + viewport.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

/// Inner partition of the modal: list on the left (or top), explainer
/// on the right (or bottom), one-row hint at the very bottom inside
/// borders. Wide mode kicks in when the body width (modal width minus
/// 2 for left/right borders) is at least 72 cells.
#[derive(Debug, Clone, Copy)]
pub struct PendingActionsModalRects {
    pub area: Rect,
    pub list: Rect,
    pub explainer: Rect,
    pub hint: Rect,
}

#[allow(dead_code)]
const LIST_WIDTH_WIDE: u16 = 32;
#[allow(dead_code)]
const SPLIT_THRESHOLD_WIDE: u16 = 72;

#[allow(dead_code)]
pub(crate) fn pending_actions_modal_rects(viewport: Rect) -> PendingActionsModalRects {
    let area = pending_actions_modal_area(viewport);
    // Carve a 1-col / 1-row inset for the modal border (Borders::ALL).
    let inner_x = area.x + 1;
    let inner_y = area.y + 1;
    let inner_w = area.width.saturating_sub(2);
    let inner_h = area.height.saturating_sub(2);

    // hint = last row inside the inner area.
    let hint = Rect::new(inner_x, inner_y + inner_h.saturating_sub(1), inner_w, 1);
    let body_h = inner_h.saturating_sub(1);
    let body = Rect::new(inner_x, inner_y, inner_w, body_h);

    let (list, explainer) = if body.width >= SPLIT_THRESHOLD_WIDE {
        // Wide: list 32 cols, 1-col separator, explainer = rest.
        let list = Rect::new(body.x, body.y, LIST_WIDTH_WIDE, body.height);
        let exp_x = body.x + LIST_WIDTH_WIDE + 1;
        let exp_w = body.width.saturating_sub(LIST_WIDTH_WIDE + 1);
        let explainer = Rect::new(exp_x, body.y, exp_w, body.height);
        (list, explainer)
    } else {
        // Narrow: list top half, explainer bottom half.
        let half = body.height / 2;
        let list = Rect::new(body.x, body.y, body.width, half);
        let explainer = Rect::new(body.x, body.y + half, body.width, body.height - half);
        (list, explainer)
    };

    PendingActionsModalRects {
        area,
        list,
        explainer,
        hint,
    }
}

/// Top-border title, e.g. `"Pending Actions · 5 pending · 3 selected · a 다시로 닫기"`.
/// The `· N selected` segment is omitted when multi-selected is empty.
pub fn pending_actions_title(overlay: &PendingActionsOverlay, items: &[PendingItem]) -> String {
    if overlay.multi_len() == 0 {
        format!(
            "Pending Actions \u{B7} {} pending \u{B7} a 다시로 닫기",
            items.len()
        )
    } else {
        format!(
            "Pending Actions \u{B7} {} pending \u{B7} {} selected \u{B7} a 다시로 닫기",
            items.len(),
            overlay.multi_len(),
        )
    }
}

/// Bottom hint line text — multi-aware action counts.
/// Returns the plain string. The renderer wraps it in dim/severity styles
/// for `(0)` segments separately.
pub fn pending_actions_hint_text(overlay: &PendingActionsOverlay, items: &[PendingItem]) -> String {
    let (n_accept, n_clear, n_copy) = pending_actions_counts(overlay, items);
    format!(
        "Space toggle \u{B7} P/Y/A group \u{B7} c clear-sel \u{B7} p accept({n_accept}) \u{B7} d clear({n_clear}) \u{B7} y copy({n_copy}) \u{B7} a/Esc close"
    )
}

fn pending_actions_counts(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> (usize, usize, usize) {
    if overlay.multi_len() == 0 {
        let cursor = items.get(overlay.selected());
        let n_accept = match cursor {
            Some(PendingItem::Proposal { .. }) => 1,
            _ => 0,
        };
        let n_clear = if cursor.is_some() { 1 } else { 0 };
        let n_copy = match cursor {
            Some(PendingItem::Copy { .. }) => 1,
            _ => 0,
        };
        return (n_accept, n_clear, n_copy);
    }
    let mut n_accept = 0;
    let mut n_clear = 0;
    let mut n_copy_present = 0;
    for item in items {
        let key = pending_item_key(item);
        if !overlay.multi_contains(&key) {
            continue;
        }
        match item {
            PendingItem::Proposal { .. } => {
                n_accept += 1;
                n_clear += 1;
            }
            PendingItem::Copy { .. } => {
                n_copy_present += 1;
                n_clear += 1;
            }
        }
    }
    let n_copy = n_copy_present.min(1); // y dispatches the first one only
    (n_accept, n_clear, n_copy)
}

pub struct PendingActionsRenderCtx<'a> {
    pub overlay: &'a PendingActionsOverlay,
    pub items: &'a [PendingItem],
    pub reports: &'a [crate::app::event_loop::PaneReport],
    pub mode: crate::app::config::ActionsMode,
    pub allow_auto_prompt_send: bool,
}

pub fn render_pending_actions_modal(frame: &mut Frame<'_>, ctx: PendingActionsRenderCtx<'_>) {
    let PendingActionsRenderCtx {
        overlay,
        items,
        reports,
        mode,
        allow_auto_prompt_send,
    } = ctx;

    let viewport = frame.area();
    let rects = pending_actions_modal_rects(viewport);
    frame.render_widget(Clear, rects.area);

    let title = pending_actions_title(overlay, items);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE));
    frame.render_widget(block, rects.area);

    // List pane.
    let list_lines = pending_actions_lines(overlay, items);
    frame.render_widget(
        Paragraph::new(list_lines).wrap(Wrap { trim: false }),
        rects.list,
    );

    // Separator + explainer pane.
    let sep_block = if rects.explainer.x > rects.list.x + rects.list.width {
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme::BORDER_ACTIVE))
    } else {
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme::BORDER_ACTIVE))
    };
    let inner_explainer = sep_block.inner(rects.explainer);
    frame.render_widget(sep_block, rects.explainer);
    let explainer_lines = explainer_lines_for_cursor(
        overlay,
        items,
        reports,
        mode,
        allow_auto_prompt_send,
        inner_explainer,
    );
    frame.render_widget(
        Paragraph::new(explainer_lines).wrap(Wrap { trim: false }),
        inner_explainer,
    );

    // Hint row.
    let hint_text = pending_actions_hint_text(overlay, items);
    frame.render_widget(
        Paragraph::new(hint_text).style(Style::default().fg(theme::TEXT_DIM)),
        rects.hint,
    );

    // [x] close button on the top border.
    frame.render_widget(
        Paragraph::new("[x]").style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        close_button_rect(rects.area),
    );
}

fn explainer_lines_for_cursor(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
    reports: &[crate::app::event_loop::PaneReport],
    mode: crate::app::config::ActionsMode,
    allow_auto_prompt_send: bool,
    body: Rect,
) -> Vec<Line<'static>> {
    use crate::app::pending_actions_overlay::build_explainer_view_for_item;
    use crate::ui::action_explainer::render_explainer_lines;

    let Some(cursor_item) = items.get(overlay.selected()) else {
        return vec![Line::styled(
            "Select an item to see what would happen.".to_string(),
            Style::default().fg(theme::TEXT_DIM),
        )];
    };
    let Some(view) =
        build_explainer_view_for_item(cursor_item, reports, mode, allow_auto_prompt_send)
    else {
        return vec![Line::styled(
            "(no live report available for this item)".to_string(),
            Style::default().fg(theme::TEXT_DIM),
        )];
    };
    render_explainer_lines(&view, body)
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
        return out;
    }
    out.push(Line::from(""));
    let selected = overlay.selected().min(items.len().saturating_sub(1));
    for (idx, item) in items.iter().enumerate() {
        let cursor = if idx == selected { "\u{25B6} " } else { "  " };
        let checked = overlay.multi_contains(&pending_item_key(item));
        let checkbox = if checked { "[x]" } else { "[ ]" };
        let kind = item.kind_letter();
        let sev_color = item
            .severity()
            .map(theme::severity_color)
            .unwrap_or(theme::TEXT_PRIMARY);
        let sev_label = item.severity().map(severity_label).unwrap_or("\u{2014}");

        let checkbox_style = if checked {
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .bg(sev_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_DIM)
        };

        let spans: Vec<Span<'static>> = vec![
            Span::styled(checkbox.to_string(), checkbox_style),
            Span::raw(" "),
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
    out
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
    }

    #[test]
    fn pending_item_key_proposal_uses_proposal_id() {
        use crate::domain::origin::SourceKind;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        assert_eq!(pending_item_key(&item), "p:%1:/compact");
    }

    #[test]
    fn pending_item_key_copy_uses_title_command() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let item = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "context-pressure".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let key = pending_item_key(&item);
        assert!(key.starts_with("y:context-pressure"));
        assert!(key.ends_with("/clear"));
    }

    #[test]
    fn toggle_multi_adds_then_removes() {
        use crate::domain::origin::SourceKind;
        let mut o = PendingActionsOverlay::new();
        o.open();
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let key = pending_item_key(&item);
        o.toggle_multi(&item);
        assert!(o.multi_contains(&key));
        o.toggle_multi(&item);
        assert!(!o.multi_contains(&key));
    }

    #[test]
    fn close_clears_multi_selected() {
        use crate::domain::origin::SourceKind;
        let mut o = PendingActionsOverlay::new();
        o.open();
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        o.toggle_multi(&item);
        o.close();
        assert_eq!(o.multi_len(), 0);
    }

    #[test]
    fn group_toggle_p_selects_then_deselects_all_proposals() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let proposal = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let copy = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "context-pressure".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let items = vec![proposal, copy];

        let mut o = PendingActionsOverlay::new();
        o.open();
        o.toggle_group_proposals(&items);
        assert_eq!(o.multi_len(), 1, "all proposals selected");
        o.toggle_group_proposals(&items);
        assert_eq!(o.multi_len(), 0, "all proposals deselected");
    }

    #[test]
    fn group_toggle_a_selects_then_deselects_all_items() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let proposal = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let copy = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "context-pressure".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let items = vec![proposal, copy];
        let mut o = PendingActionsOverlay::new();
        o.open();
        o.toggle_group_all(&items);
        assert_eq!(o.multi_len(), 2);
        o.toggle_group_all(&items);
        assert_eq!(o.multi_len(), 0);
    }

    #[test]
    fn auto_prune_drops_keys_no_longer_in_items() {
        use crate::domain::origin::SourceKind;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let mut o = PendingActionsOverlay::new();
        o.open();
        o.toggle_multi(&item);
        assert_eq!(o.multi_len(), 1);
        // items now empty (the proposal vanished between polls)
        o.prune_to(&[]);
        assert_eq!(o.multi_len(), 0);
    }

    #[test]
    fn modal_area_uses_80x65_with_min_72_20() {
        let r = pending_actions_modal_area(Rect::new(0, 0, 200, 80));
        assert_eq!(r.width, 200 * 80 / 100);
        assert_eq!(r.height, 80 * 65 / 100);

        // Tiny viewport falls back to the minimum.
        let small = pending_actions_modal_area(Rect::new(0, 0, 60, 18));
        // viewport.width=60 < 72 min: clamps to viewport width
        assert_eq!(small.width, 60);
        // viewport.height=18 < 20 min: clamps to viewport height
        assert_eq!(small.height, 18);
    }

    #[test]
    fn modal_rects_wide_mode_splits_vertically_with_32_col_list() {
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport);
        // Wide mode: list width = 32, separator = 1 col, explainer = rest.
        assert_eq!(rects.list.width, 32);
        let sep_col = rects.list.x + rects.list.width;
        assert_eq!(rects.explainer.x, sep_col + 1);
        assert_eq!(
            rects.explainer.x + rects.explainer.width,
            rects.area.x + rects.area.width - 1, // -1: right border
            "explainer extends to the right border"
        );
        // hint is the bottom row inside borders.
        assert_eq!(rects.hint.height, 1);
        assert_eq!(rects.hint.y, rects.area.y + rects.area.height - 2);
    }

    #[test]
    fn modal_rects_narrow_mode_splits_horizontally() {
        // body.width < 72 → list on top, explainer on bottom.
        let viewport = Rect::new(0, 0, 80, 30);
        let rects = pending_actions_modal_rects(viewport);
        let body_width = rects.area.width - 2; // minus borders
        assert!(body_width < 72, "this test requires narrow body width");
        // list and explainer share the body height (minus hint).
        assert_eq!(rects.list.x, rects.explainer.x);
        assert_eq!(rects.list.width, rects.explainer.width);
        assert!(rects.list.height >= 1);
        assert!(rects.explainer.height >= 1);
        assert_eq!(rects.list.y + rects.list.height, rects.explainer.y);
    }

    #[test]
    fn lines_render_checkbox_unchecked_for_non_multi() {
        use crate::domain::origin::SourceKind;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let overlay = PendingActionsOverlay::new();
        let lines = pending_actions_lines(&overlay, &[item]);
        let dump: String = lines.iter().map(|l| line_text(l) + "\n").collect();
        assert!(
            dump.contains("[ ]"),
            "non-multi row must render `[ ]`: {dump}"
        );
        assert!(
            dump.contains("\u{25B6}"),
            "first row gets the cursor mark: {dump}"
        );
    }

    #[test]
    fn lines_render_checkbox_checked_when_multi_selected() {
        use crate::domain::origin::SourceKind;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let mut overlay = PendingActionsOverlay::new();
        overlay.toggle_multi(&item);
        let lines = pending_actions_lines(&overlay, &[item]);
        let dump: String = lines.iter().map(|l| line_text(l) + "\n").collect();
        assert!(
            dump.contains("[x]"),
            "multi-selected row must render `[x]`: {dump}"
        );
    }

    #[test]
    fn title_includes_selected_count_when_multi_non_empty() {
        use crate::domain::origin::SourceKind;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let mut overlay = PendingActionsOverlay::new();
        overlay.toggle_multi(&item);
        let title = pending_actions_title(&overlay, std::slice::from_ref(&item));
        assert!(title.contains("1 pending"), "{title}");
        assert!(title.contains("1 selected"), "{title}");
    }

    #[test]
    fn title_omits_selected_count_when_empty() {
        use crate::domain::origin::SourceKind;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let overlay = PendingActionsOverlay::new();
        let title = pending_actions_title(&overlay, std::slice::from_ref(&item));
        assert!(title.contains("1 pending"));
        assert!(
            !title.contains("selected"),
            "no selected segment when multi empty"
        );
    }

    #[test]
    fn hint_shows_action_counts_for_cursor_proposal_no_multi() {
        use crate::domain::origin::SourceKind;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let overlay = PendingActionsOverlay::new();
        let hint = pending_actions_hint_text(&overlay, std::slice::from_ref(&item));
        assert!(hint.contains("p accept(1)"), "{hint}");
        assert!(hint.contains("d clear(1)"), "{hint}");
        assert!(
            hint.contains("y copy(0)"),
            "cursor is proposal, not alert: {hint}"
        );
    }

    #[test]
    fn hint_shows_multi_counts() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let p = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let y = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "context-pressure".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let items = vec![p, y];
        let mut overlay = PendingActionsOverlay::new();
        overlay.toggle_group_all(&items);
        let hint = pending_actions_hint_text(&overlay, &items);
        assert!(hint.contains("p accept(1)"), "1 proposal in multi: {hint}");
        assert!(hint.contains("d clear(2)"), "2 items in multi: {hint}");
        assert!(hint.contains("y copy(1)"), "1 alert in multi: {hint}");
    }
}
