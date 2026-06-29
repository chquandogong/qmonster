//! Phase v1.39 surface C / v1.40 redesign — Pending Actions overlay (`a` key).
//!
//! Lists every pane with a pending prompt-send proposal AND every
//! alert with a `suggested_command` in a split modal: list pane on
//! the left, live Action Explainer panel on the right (or top/bottom
//! when the body is narrow). Multi-select via Space / P / Y / A / c
//! with stable-key tracking; p / d / y dispatch in-place through the
//! existing `confirm_pending_action(...)` path; Enter is silently
//! swallowed (the live explainer panel is the confirmation).
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
use crate::ui::scroll_hint;
use crate::ui::theme;

// Modal size + position adjustability (parity with the other large
// overlays). `[`/`]` shrink/grow by SIZE_STEP percent (clamped to
// [SIZE_MIN, SIZE_MAX]). `=` resets to defaults AND zeros offsets.
// Title-row drag updates offsets. Mirrors the anomaly overlay's
// `ModalGeometry` controls so the operator's mental model of "modal
// geometry controls" is the same across overlays.
pub const SIZE_STEP: u16 = 5;
pub const SIZE_MIN: u16 = 50;
pub const SIZE_MAX: u16 = 99;
pub const DEFAULT_WIDTH_PCT: u16 = 80;
pub const DEFAULT_HEIGHT_PCT: u16 = 65;

/// Open/closed state + selection cursor for the Pending Actions
/// overlay. Pure — mirrors the discipline of
/// `ScrollModalState` / `ActionExplainModal`.
#[derive(Debug, Clone)]
pub struct PendingActionsOverlay {
    open: bool,
    selected: usize,
    multi_selected: BTreeSet<String>,
    width_pct: u16,
    height_pct: u16,
    offset_x: i16,
    offset_y: i16,
    move_drag_anchor: Option<MoveDragAnchor>,
    // Operator-set list-pane width override. None = use auto (60% of
    // body, clamped to [LIST_WIDTH_WIDE_MIN, LIST_WIDTH_WIDE_MAX]).
    list_width_override: Option<u16>,
    resize_drag_anchor: Option<ResizeDragAnchor>,
    /// v1.41: tracks whether the operator has opened this overlay at
    /// least once in the current session. Used by `tui_loop` to fire
    /// a one-time SystemNotice about the confirm_actions bypass.
    /// Resets to false on Qmonster restart (not persisted).
    seen_first_open: bool,
}

/// Anchor captured at title-row Down(Left). The drag handler computes
/// new offset = `start_offset_*` + (current_event - start_*). Named
/// `MoveDragAnchor` (not `DragAnchor`) so a future TX-B can introduce
/// a `ResizeDragAnchor` without renaming.
#[derive(Debug, Clone, Copy)]
pub struct MoveDragAnchor {
    pub start_col: u16,
    pub start_row: u16,
    pub start_offset_x: i16,
    pub start_offset_y: i16,
}

/// Anchor captured at separator-zone Down(Left). The drag handler
/// computes new list_width = (event.column - body.x). Stored as the
/// column delta from `body.x` so it survives a modal-resize during
/// drag (rare, but defensive).
#[derive(Debug, Clone, Copy)]
pub struct ResizeDragAnchor {
    pub start_col: u16,
    pub start_list_width: u16,
}

impl Default for PendingActionsOverlay {
    fn default() -> Self {
        Self {
            open: false,
            selected: 0,
            multi_selected: BTreeSet::new(),
            width_pct: DEFAULT_WIDTH_PCT,
            height_pct: DEFAULT_HEIGHT_PCT,
            offset_x: 0,
            offset_y: 0,
            move_drag_anchor: None,
            list_width_override: None,
            resize_drag_anchor: None,
            seen_first_open: false,
        }
    }
}

impl PendingActionsOverlay {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
        self.move_drag_anchor = None;
        self.resize_drag_anchor = None;
        // multi_selected stays empty (default already empty); explicit no-op.
        // width_pct, height_pct, offset_x, offset_y, list_width_override
        // intentionally preserved (mirrors m overlay size persistence —
        // operator's chosen geometry survives close/open within a session).
    }
    pub fn close(&mut self) {
        self.open = false;
        self.selected = 0;
        self.multi_selected.clear();
        self.move_drag_anchor = None;
        self.resize_drag_anchor = None;
        // size + offset + list_width_override preserved (same as open).
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    /// v1.41: returns whether this overlay has been opened at least
    /// once in the current session. `tui_loop` reads this on the `a`
    /// key arm to decide whether to fire the one-time
    /// confirm_actions-bypass SystemNotice. Resets to false on
    /// Qmonster restart (not persisted).
    pub fn seen_first_open(&self) -> bool {
        self.seen_first_open
    }
    /// v1.41: marks the overlay as opened at least once in this
    /// session. Call once, alongside the SystemNotice, on the FIRST
    /// `a`-key open (not on subsequent opens, not on close).
    pub fn mark_first_open_seen(&mut self) {
        self.seen_first_open = true;
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

    /// Drop multi-select keys whose item is not in `items`, and clamp
    /// `selected` to a valid index for the new `items.len()`.
    ///
    /// v1.40 post-release fix: the renderer was already clamping the
    /// visual cursor for display, but `dispatch_accept` / `dispatch_clear`
    /// / `dispatch_copy` (and the live explainer panel) read the raw
    /// `selected()` and would silently produce no-op outcomes when a
    /// polling tick shrunk `items.len()` past the previously-selected
    /// index. Clamping here propagates everywhere because `tui_loop`
    /// invokes `prune_to` after every `collect_pending_items(...)` call.
    pub fn prune_to(&mut self, items: &[PendingItem]) {
        let live: BTreeSet<String> = items.iter().map(pending_item_key).collect();
        self.multi_selected.retain(|k| live.contains(k));
        if items.is_empty() {
            self.selected = 0;
        } else if self.selected >= items.len() {
            self.selected = items.len() - 1;
        }
    }

    /// Retain only multi-select keys for which `pred` returns true.
    /// Used by the bulk dispatchers to drop keys that were just
    /// dispatched, while keeping the rest of the multi-selection
    /// intact (spec §5.10: "only dispatched keys are removed").
    pub fn retain_multi(&mut self, mut pred: impl FnMut(&String) -> bool) {
        self.multi_selected.retain(|k| pred(k));
    }

    // --- size + position controls ---------------------------------------

    pub fn width_pct(&self) -> u16 {
        self.width_pct
    }
    pub fn height_pct(&self) -> u16 {
        self.height_pct
    }
    pub fn offset_x(&self) -> i16 {
        self.offset_x
    }
    pub fn offset_y(&self) -> i16 {
        self.offset_y
    }
    pub fn move_drag_anchor(&self) -> Option<MoveDragAnchor> {
        self.move_drag_anchor
    }

    pub fn set_offset(&mut self, x: i16, y: i16) {
        self.offset_x = x;
        self.offset_y = y;
    }

    pub fn begin_move_drag(&mut self, anchor: MoveDragAnchor) {
        self.move_drag_anchor = Some(anchor);
    }

    pub fn end_drag(&mut self) {
        self.move_drag_anchor = None;
        self.resize_drag_anchor = None;
    }

    pub fn grow(&mut self) {
        self.width_pct = (self.width_pct + SIZE_STEP).min(SIZE_MAX);
        self.height_pct = (self.height_pct + SIZE_STEP).min(SIZE_MAX);
    }

    pub fn shrink(&mut self) {
        self.width_pct = self.width_pct.saturating_sub(SIZE_STEP).max(SIZE_MIN);
        self.height_pct = self.height_pct.saturating_sub(SIZE_STEP).max(SIZE_MIN);
    }

    pub fn reset_size(&mut self) {
        self.width_pct = DEFAULT_WIDTH_PCT;
        self.height_pct = DEFAULT_HEIGHT_PCT;
        self.offset_x = 0;
        self.offset_y = 0;
        self.list_width_override = None;
    }

    // --- list/explainer ratio controls ----------------------------------

    pub fn list_width_override(&self) -> Option<u16> {
        self.list_width_override
    }

    pub fn resize_drag_anchor(&self) -> Option<ResizeDragAnchor> {
        self.resize_drag_anchor
    }

    pub fn set_list_width(&mut self, w: u16) {
        self.list_width_override = Some(w.clamp(LIST_WIDTH_WIDE_MIN, LIST_WIDTH_WIDE_MAX));
    }

    /// Step the list pane wider by `LIST_WIDTH_WIDE_STEP`, starting
    /// from `current` (the effective list width seen on screen). The
    /// caller is expected to compute `current` via
    /// `pending_actions_modal_rects(viewport, &overlay).list.width` so
    /// that the first press relative to the auto-formula's effective
    /// width steps in the right direction.
    ///
    /// v1.40 post-release fix: the previous implementation used
    /// `LIST_WIDTH_WIDE_MIN` as the no-override fallback, which made
    /// the first press of `.` jump to MIN+2 (a shrink) on a typical
    /// 120-col terminal where auto width sits mid-range.
    pub fn widen_list(&mut self, current: u16) {
        let next = (current + LIST_WIDTH_WIDE_STEP).min(LIST_WIDTH_WIDE_MAX);
        self.list_width_override = Some(next.max(LIST_WIDTH_WIDE_MIN));
    }

    /// Step the list pane narrower by `LIST_WIDTH_WIDE_STEP`. See
    /// [`widen_list`] for the rationale on threading `current`.
    pub fn narrow_list(&mut self, current: u16) {
        let next = current
            .saturating_sub(LIST_WIDTH_WIDE_STEP)
            .max(LIST_WIDTH_WIDE_MIN);
        self.list_width_override = Some(next.min(LIST_WIDTH_WIDE_MAX));
    }

    pub fn begin_resize_drag(&mut self, anchor: ResizeDragAnchor) {
        self.resize_drag_anchor = Some(anchor);
    }
}

/// One actionable item the overlay lists. `Proposal` matches a pane
/// carrying a `PromptSendProposed` effect (operator presses `p`/`d`);
/// `Copy` matches an alert with an executable `suggested_command`
/// (operator presses `y`). Index slots reference the live `reports` /
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
        Provider::Antigravity => "agy",
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

pub(crate) fn pending_actions_modal_area(
    viewport: Rect,
    width_pct: u16,
    height_pct: u16,
    offset_x: i16,
    offset_y: i16,
) -> Rect {
    let width = (viewport.width * width_pct / 100)
        .max(72)
        .min(viewport.width);
    let height = (viewport.height * height_pct / 100)
        .max(20)
        .min(viewport.height);
    let base_x = viewport.x + viewport.width.saturating_sub(width) / 2;
    let base_y = viewport.y + viewport.height.saturating_sub(height) / 2;
    let base = Rect::new(base_x, base_y, width, height);
    apply_clamped_offset(base, viewport, offset_x, offset_y)
}

/// Apply offset_x/offset_y to base, keeping >= 4 cells horizontally
/// and >= 1 row vertically inside viewport. Asymmetric: left/top are
/// hard bounds (modal can't extend past viewport.x or viewport.y),
/// right/bottom are soft bounds (can extend past with a sliver visible).
/// Mirrors `apply_clamped_offset` on the m overlay (src/ui/metrics.rs).
fn apply_clamped_offset(base: Rect, viewport: Rect, offset_x: i16, offset_y: i16) -> Rect {
    let min_x = viewport.x as i32;
    let max_x = (viewport.x as i32) + (viewport.width as i32) - 4;
    let min_y = viewport.y as i32;
    let max_y = (viewport.y as i32) + (viewport.height as i32) - 1;
    let x = ((base.x as i32) + (offset_x as i32)).clamp(min_x, max_x.max(min_x));
    let y = ((base.y as i32) + (offset_y as i32)).clamp(min_y, max_y.max(min_y));
    Rect::new(
        x.max(0) as u16,
        y.max(0) as u16,
        base.width.min(
            viewport
                .width
                .saturating_sub((x - viewport.x as i32).max(0) as u16)
                .max(1),
        ),
        base.height.min(
            viewport
                .height
                .saturating_sub((y - viewport.y as i32).max(0) as u16)
                .max(1),
        ),
    )
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

// Wide-mode list pane occupies 60% of body width, clamped to [44, 64].
// Rationale (operator feedback 2026-05-06): a fixed 32-cell list wrapped
// every typical row (`[x] ▶ [p] CONCERN · /compact · pane:label · %ID`
// runs ~50–55 chars). 60% gives the list comfortable room without
// starving the explainer, the [44, 64] clamp keeps the explainer usable
// on smaller terminals (~30 cells minimum) and prevents the list from
// growing past the row content length on huge terminals.
const LIST_WIDTH_WIDE_RATIO: u32 = 60;
pub(crate) const LIST_WIDTH_WIDE_MIN: u16 = 44;
pub(crate) const LIST_WIDTH_WIDE_MAX: u16 = 64;
pub const LIST_WIDTH_WIDE_STEP: u16 = 2;
const SPLIT_THRESHOLD_WIDE: u16 = 72;

pub(crate) fn wide_mode_list_width(body_width: u16, override_width: Option<u16>) -> u16 {
    if let Some(w) = override_width {
        // Operator override clamped to the same range as the auto-formula
        // ([LIST_WIDTH_WIDE_MIN, LIST_WIDTH_WIDE_MAX]). Body width is not
        // a factor here: the modal's SPLIT_THRESHOLD_WIDE (72) ensures
        // body ≥ 72 in wide mode, so body - LIST_WIDTH_WIDE_MAX ≥ 8 cells
        // remain for the explainer even at the operator's maximum
        // override.
        return w.clamp(LIST_WIDTH_WIDE_MIN, LIST_WIDTH_WIDE_MAX);
    }
    let scaled = (body_width as u32 * LIST_WIDTH_WIDE_RATIO / 100) as u16;
    scaled.clamp(LIST_WIDTH_WIDE_MIN, LIST_WIDTH_WIDE_MAX)
}

pub(crate) fn pending_actions_modal_rects(
    viewport: Rect,
    overlay: &PendingActionsOverlay,
) -> PendingActionsModalRects {
    let area = pending_actions_modal_area(
        viewport,
        overlay.width_pct(),
        overlay.height_pct(),
        overlay.offset_x(),
        overlay.offset_y(),
    );
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
        // Wide: list at 60% of body (clamped), 1-col separator,
        // explainer = rest. Separator is the explainer's LEFT border.
        let list_w = wide_mode_list_width(body.width, overlay.list_width_override());
        let list = Rect::new(body.x, body.y, list_w, body.height);
        let exp_x = body.x + list_w;
        let exp_w = body.width.saturating_sub(list_w);
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
    let max_scroll = items.len().saturating_sub(1).min(u16::MAX as usize) as u16;
    let scroll = overlay.selected().min(max_scroll as usize) as u16;
    format!(
        "Space toggle \u{B7} P/Y/A group \u{B7} c clear-sel \u{B7} p accept({n_accept}) \u{B7} d clear({n_clear}) \u{B7} y copy({n_copy}) \u{B7} [/]/, /. /= geom \u{B7} (confirm_actions bypass) \u{B7} a/Esc close \u{B7} {}",
        scroll_hint::scroll_status_label(scroll, max_scroll)
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
    let rects = pending_actions_modal_rects(viewport, overlay);
    frame.render_widget(Clear, rects.area);

    let title = pending_actions_title(overlay, items);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_active()));
    frame.render_widget(block, rects.area);

    // List pane.
    let list_lines = pending_actions_lines(overlay, items);
    frame.render_widget(
        Paragraph::new(list_lines).wrap(Wrap { trim: false }),
        rects.list,
    );

    // Separator + explainer pane.
    // Wide mode: explainer is shifted right of the list — use a vertical separator.
    // Narrow mode: explainer sits below the list (same x) — use a horizontal separator.
    let wide_mode = rects.explainer.x > rects.list.x;
    let sep_block = if wide_mode {
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme::border_active()))
    } else {
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme::border_active()))
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
        Paragraph::new(hint_text).style(Style::default().fg(theme::text_dim())),
        rects.hint,
    );

    // [x] close button on the top border.
    frame.render_widget(
        Paragraph::new("[x]").style(theme::modal_close_style()),
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
            Style::default().fg(theme::text_dim()),
        )];
    };
    let Some(view) =
        build_explainer_view_for_item(cursor_item, reports, mode, allow_auto_prompt_send)
    else {
        return vec![Line::styled(
            "(no live report available for this item)".to_string(),
            Style::default().fg(theme::text_dim()),
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
            Style::default().fg(theme::text_dim()),
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
            .unwrap_or(theme::text_primary());
        let sev_label = item.severity().map(severity_label).unwrap_or("\u{2014}");

        let checkbox_style = if checked {
            Style::default()
                .fg(theme::text_primary())
                .bg(sev_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::text_dim())
        };

        let spans: Vec<Span<'static>> = vec![
            Span::styled(checkbox.to_string(), checkbox_style),
            Span::raw(" "),
            Span::raw(cursor.to_string()),
            Span::styled(
                format!("[{kind}]"),
                Style::default()
                    .fg(theme::text_primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" {sev_label} "),
                Style::default()
                    .fg(theme::text_primary())
                    .bg(sev_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" \u{B7} "),
            Span::styled(
                format!("`{}`", item.command()),
                Style::default()
                    .fg(theme::text_primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" \u{B7} "),
            Span::styled(
                item.context().to_string(),
                Style::default().fg(theme::text_dim()),
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
            worktree_role: None,
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(),
            anomalies: vec![],
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
    fn list_lines_render_proposal_and_copy_rows() {
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
    fn empty_list_renders_no_pending_actions_message() {
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
    fn prune_to_clamps_selected_when_items_shrink() {
        use crate::domain::origin::SourceKind;
        let p1 = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let p2 = PendingItem::Proposal {
            pane_idx: 1,
            pane_label: "claude:1:review · %2".into(),
            slash_command: "/clear".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%2:/clear".into(),
            target_pane_id: "%2".into(),
        };
        let _ = &p1; // p1 is here for clarity; the shrink uses just p2
        let mut o = PendingActionsOverlay::new();
        o.set_selected(1); // pointing at the second item
        o.prune_to(std::slice::from_ref(&p2)); // items shrink to one
        assert_eq!(
            o.selected(),
            0,
            "selected must clamp to items.len() - 1 = 0"
        );

        let mut o2 = PendingActionsOverlay::new();
        o2.set_selected(5);
        o2.prune_to(&[]); // items shrink to empty
        assert_eq!(o2.selected(), 0, "empty items resets selected to 0");
    }

    #[test]
    fn modal_area_uses_80x65_with_min_72_20() {
        let r = pending_actions_modal_area(
            Rect::new(0, 0, 200, 80),
            DEFAULT_WIDTH_PCT,
            DEFAULT_HEIGHT_PCT,
            0,
            0,
        );
        assert_eq!(r.width, 200 * 80 / 100);
        assert_eq!(r.height, 80 * 65 / 100);

        // Tiny viewport falls back to the minimum.
        let small = pending_actions_modal_area(
            Rect::new(0, 0, 60, 18),
            DEFAULT_WIDTH_PCT,
            DEFAULT_HEIGHT_PCT,
            0,
            0,
        );
        // viewport.width=60 < 72 min: clamps to viewport width
        assert_eq!(small.width, 60);
        // viewport.height=18 < 20 min: clamps to viewport height
        assert_eq!(small.height, 18);
    }

    #[test]
    fn modal_rects_wide_mode_splits_vertically_with_dynamic_list() {
        // 200×80 viewport → modal 160×52, body width 158. List at 60%
        // = 94, clamped to LIST_WIDTH_WIDE_MAX = 64. Explainer = body
        // width − list = 94. Separator is the explainer's LEFT border.
        let viewport = Rect::new(0, 0, 200, 80);
        let overlay = PendingActionsOverlay::new();
        let rects = pending_actions_modal_rects(viewport, &overlay);
        assert!(
            (LIST_WIDTH_WIDE_MIN..=LIST_WIDTH_WIDE_MAX).contains(&rects.list.width),
            "list width {} out of clamp range",
            rects.list.width
        );
        let sep_col = rects.list.x + rects.list.width;
        assert_eq!(
            rects.explainer.x, sep_col,
            "explainer rect's left edge IS the separator column (Borders::LEFT paints there)"
        );
        assert_eq!(
            rects.explainer.x + rects.explainer.width,
            rects.area.x + rects.area.width - 1, // -1: right border
            "explainer extends to the right border"
        );
        assert_eq!(rects.hint.height, 1);
        assert_eq!(rects.hint.y, rects.area.y + rects.area.height - 2);
    }

    #[test]
    fn wide_mode_list_width_clamps_at_min() {
        // Body width just past the SPLIT_THRESHOLD_WIDE (72): 60% of
        // 72 = 43, below the LIST_WIDTH_WIDE_MIN floor of 44 — so the
        // clamp pulls list up to 44.
        let list = wide_mode_list_width(72, None);
        assert_eq!(list, LIST_WIDTH_WIDE_MIN);
    }

    #[test]
    fn wide_mode_list_width_clamps_at_max() {
        // Huge body (e.g., 158-cell body on a 200-col terminal): 60%
        // = 94, capped at LIST_WIDTH_WIDE_MAX = 64 so the explainer
        // still has plenty of room and the list doesn't grow past
        // typical row content (~53 chars).
        let list = wide_mode_list_width(158, None);
        assert_eq!(list, LIST_WIDTH_WIDE_MAX);
    }

    #[test]
    fn wide_mode_list_width_uses_60pct_in_mid_range() {
        // 120-col terminal → modal 96, body 94. 60% = 56 (within
        // [LIST_WIDTH_WIDE_MIN, LIST_WIDTH_WIDE_MAX]), so the proportional
        // path drives the value.
        let list = wide_mode_list_width(94, None);
        assert_eq!(list, 56);
    }

    #[test]
    fn modal_rects_narrow_mode_splits_horizontally() {
        // body.width < 72 → list on top, explainer on bottom.
        let viewport = Rect::new(0, 0, 80, 30);
        let overlay = PendingActionsOverlay::new();
        let rects = pending_actions_modal_rects(viewport, &overlay);
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

    #[test]
    fn hint_caps_copy_count_at_one_when_multi_has_two_alerts() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        let y1 = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "context-pressure".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let y2 = PendingItem::Copy {
            alert_idx: 1,
            command: "/compact".into(),
            alert_title: "cache-drift".into(),
            severity: Severity::Concern,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let items = vec![y1, y2];
        let mut overlay = PendingActionsOverlay::new();
        overlay.toggle_group_all(&items);
        let hint = pending_actions_hint_text(&overlay, &items);
        assert!(
            hint.contains("y copy(1)"),
            "must cap at 1 (y dispatches first only): {hint}"
        );
        assert!(!hint.contains("y copy(2)"), "must NOT show 2: {hint}");
    }

    #[test]
    fn hint_text_reports_list_progress_and_end() {
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
        let items = vec![item.clone(), item.clone(), item];
        let mut overlay = PendingActionsOverlay::new();
        overlay.set_selected(1);
        let middle = pending_actions_hint_text(&overlay, &items);
        assert!(middle.contains("scroll 1/2 · more"), "{middle}");

        overlay.set_selected(2);
        let end = pending_actions_hint_text(&overlay, &items);
        assert!(end.contains("scroll 2/2 · END"), "{end}");

        overlay.set_selected(9);
        let clamped = pending_actions_hint_text(&overlay, &items);
        assert!(clamped.contains("scroll 2/2 · END"), "{clamped}");

        let empty = pending_actions_hint_text(&PendingActionsOverlay::new(), &[]);
        assert!(empty.contains("scroll 0/0 · END"), "{empty}");
    }

    #[test]
    fn hint_text_includes_confirm_actions_bypass_chip() {
        let overlay = PendingActionsOverlay::new();
        let items: Vec<PendingItem> = Vec::new();
        let hint = pending_actions_hint_text(&overlay, &items);
        assert!(
            hint.contains("(confirm_actions bypass)"),
            "hint must surface the confirm_actions bypass chip: {hint}"
        );
    }

    #[test]
    fn seen_first_open_starts_false_and_persists_across_close_open() {
        let mut o = PendingActionsOverlay::new();
        assert!(!o.seen_first_open(), "fresh overlay starts with seen=false");
        o.mark_first_open_seen();
        o.open();
        o.close();
        o.open();
        assert!(
            o.seen_first_open(),
            "seen flag persists across close/open in-session"
        );
    }

    #[test]
    fn wide_mode_renders_vertical_separator_at_list_right_edge() {
        use crate::app::config::ActionsMode;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(200, 80)).expect("new TestBackend");
        let overlay = PendingActionsOverlay::new();
        terminal
            .draw(|f| {
                render_pending_actions_modal(
                    f,
                    PendingActionsRenderCtx {
                        overlay: &overlay,
                        items: &[],
                        reports: &[],
                        mode: ActionsMode::RecommendOnly,
                        allow_auto_prompt_send: true,
                    },
                );
            })
            .expect("draw must succeed");
        let buffer = terminal.backend().buffer();
        let rects = pending_actions_modal_rects(Rect::new(0, 0, 200, 80), &overlay);
        // In wide mode explainer.x > list.x — confirm this viewport is wide.
        assert!(
            rects.explainer.x > rects.list.x,
            "this test requires wide mode (200 cols)"
        );
        let sep_col = rects.list.x + rects.list.width;
        let mid_y = rects.list.y + rects.list.height / 2;
        let cell = buffer.cell((sep_col, mid_y)).expect("cell in bounds");
        assert_eq!(
            cell.symbol(),
            "\u{2502}",
            "wide mode must render '│' at separator column {sep_col} row {mid_y}; got {:?}",
            cell.symbol()
        );
    }

    #[test]
    fn narrow_mode_renders_horizontal_separator_at_explainer_top() {
        use crate::app::config::ActionsMode;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        // Narrow viewport so the modal body width < SPLIT_THRESHOLD_WIDE (72).
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("new TestBackend");
        let overlay = PendingActionsOverlay::new();
        terminal
            .draw(|f| {
                render_pending_actions_modal(
                    f,
                    PendingActionsRenderCtx {
                        overlay: &overlay,
                        items: &[],
                        reports: &[],
                        mode: ActionsMode::RecommendOnly,
                        allow_auto_prompt_send: true,
                    },
                );
            })
            .expect("draw must succeed");
        let buffer = terminal.backend().buffer();
        let rects = pending_actions_modal_rects(Rect::new(0, 0, 80, 30), &overlay);
        // Sanity: confirm narrow mode (list.x == explainer.x).
        assert_eq!(
            rects.list.x, rects.explainer.x,
            "this test requires narrow mode"
        );
        let sep_row = rects.explainer.y;
        let mid_x = rects.explainer.x + rects.explainer.width / 2;
        let cell = buffer.cell((mid_x, sep_row)).expect("cell in bounds");
        assert_eq!(
            cell.symbol(),
            "\u{2500}",
            "narrow mode must render '─' at separator row {sep_row} col {mid_x}; got {:?}",
            cell.symbol()
        );
    }

    #[test]
    fn shrink_grow_clamp_to_min_max() {
        let mut o = PendingActionsOverlay::new();
        // shrink past min
        for _ in 0..20 {
            o.shrink();
        }
        assert_eq!(o.width_pct(), SIZE_MIN);
        assert_eq!(o.height_pct(), SIZE_MIN);
        // grow back to max
        for _ in 0..20 {
            o.grow();
        }
        assert_eq!(o.width_pct(), SIZE_MAX);
        assert_eq!(o.height_pct(), SIZE_MAX);
    }

    #[test]
    fn reset_size_returns_to_defaults_and_zeros_offset() {
        let mut o = PendingActionsOverlay::new();
        o.shrink();
        o.set_offset(7, 3);
        o.reset_size();
        assert_eq!(o.width_pct(), DEFAULT_WIDTH_PCT);
        assert_eq!(o.height_pct(), DEFAULT_HEIGHT_PCT);
        assert_eq!(o.offset_x(), 0);
        assert_eq!(o.offset_y(), 0);
    }

    #[test]
    fn close_preserves_size_and_offset() {
        let mut o = PendingActionsOverlay::new();
        o.open();
        o.shrink();
        o.set_offset(3, -2);
        let (w, h, ox, oy) = (o.width_pct(), o.height_pct(), o.offset_x(), o.offset_y());
        o.close();
        o.open();
        assert_eq!(
            (o.width_pct(), o.height_pct(), o.offset_x(), o.offset_y()),
            (w, h, ox, oy)
        );
    }

    #[test]
    fn list_width_override_clamps_to_min_max() {
        let mut o = PendingActionsOverlay::new();
        o.set_list_width(10);
        assert_eq!(o.list_width_override(), Some(LIST_WIDTH_WIDE_MIN));
        o.set_list_width(200);
        assert_eq!(o.list_width_override(), Some(LIST_WIDTH_WIDE_MAX));
    }

    #[test]
    fn widen_narrow_step_by_two() {
        let mut o = PendingActionsOverlay::new();
        // Start from a typical 120-col-terminal auto width (56) and
        // verify both directions move by LIST_WIDTH_WIDE_STEP from
        // whatever `current` the caller threads in.
        o.widen_list(56);
        assert_eq!(o.list_width_override(), Some(58));
        o.narrow_list(58);
        assert_eq!(o.list_width_override(), Some(56));
    }

    #[test]
    fn reset_size_also_clears_list_override() {
        let mut o = PendingActionsOverlay::new();
        o.set_list_width(50);
        o.set_offset(3, -1);
        o.reset_size();
        assert_eq!(o.list_width_override(), None);
        assert_eq!(o.offset_x(), 0);
        assert_eq!(o.offset_y(), 0);
    }

    #[test]
    fn close_preserves_list_width_override() {
        let mut o = PendingActionsOverlay::new();
        o.open();
        o.set_list_width(50);
        o.close();
        o.open();
        assert_eq!(o.list_width_override(), Some(50));
    }

    #[test]
    fn wide_mode_list_width_uses_override_when_set() {
        // Wide body so the auto path hits LIST_WIDTH_WIDE_MAX (64);
        // the override (Some(50)) should land at 50.
        let auto = wide_mode_list_width(158, None);
        let overridden = wide_mode_list_width(158, Some(50));
        assert_ne!(auto, overridden);
        assert_eq!(overridden, 50);
    }

    #[test]
    fn modal_area_zero_offset_centers_modal() {
        let viewport = Rect::new(0, 0, 200, 80);
        let r = pending_actions_modal_area(viewport, 80, 65, 0, 0);
        let expected_w = 200 * 80 / 100;
        let expected_h = 80 * 65 / 100;
        assert_eq!(r.width, expected_w);
        assert_eq!(r.height, expected_h);
        assert_eq!(r.x, viewport.x + (viewport.width - expected_w) / 2);
        assert_eq!(r.y, viewport.y + (viewport.height - expected_h) / 2);
    }

    #[test]
    fn modal_area_positive_offset_moves_right_down() {
        let viewport = Rect::new(0, 0, 200, 80);
        let r0 = pending_actions_modal_area(viewport, 80, 65, 0, 0);
        let r1 = pending_actions_modal_area(viewport, 80, 65, 5, 3);
        assert_eq!(r1.x, r0.x + 5);
        assert_eq!(r1.y, r0.y + 3);
    }

    #[test]
    fn modal_area_left_clamp_snaps_to_viewport_x() {
        // Non-origin viewport so the snap-to-zero shortcut can't hide a bug.
        let viewport = Rect::new(20, 5, 200, 80);
        let r = pending_actions_modal_area(viewport, 50, 50, i16::MIN, 0);
        assert_eq!(
            r.x, viewport.x,
            "left edge is hard bound — modal can't extend past viewport.x"
        );
    }

    #[test]
    fn modal_area_right_clamp_keeps_4_cells_visible() {
        let viewport = Rect::new(20, 5, 200, 80);
        let r = pending_actions_modal_area(viewport, 50, 50, i16::MAX, 0);
        let visible_right = (viewport.x + viewport.width).saturating_sub(r.x);
        assert!(
            visible_right >= 4,
            "right clamp must leave ≥ 4 cells inside viewport (got {visible_right})"
        );
    }

    #[test]
    fn modal_area_top_clamp_snaps_to_viewport_y() {
        let viewport = Rect::new(20, 5, 200, 80);
        let r = pending_actions_modal_area(viewport, 50, 50, 0, i16::MIN);
        assert_eq!(
            r.y, viewport.y,
            "top edge is hard bound — modal can't extend past viewport.y"
        );
    }

    #[test]
    fn modal_area_bottom_clamp_keeps_1_row_visible() {
        let viewport = Rect::new(20, 5, 200, 80);
        let r = pending_actions_modal_area(viewport, 50, 50, 0, i16::MAX);
        let visible_height = (viewport.y + viewport.height).saturating_sub(r.y);
        assert!(
            visible_height >= 1,
            "bottom clamp must leave ≥ 1 row inside viewport (got {visible_height})"
        );
    }
}
