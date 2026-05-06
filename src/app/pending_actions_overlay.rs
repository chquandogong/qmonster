//! Phase v1.40 — key + mouse dispatchers for the Pending Actions
//! overlay. The overlay state and renderer live in
//! `ui::pending_actions`; this module wires keystrokes to the
//! overlay so `tui_loop` can stay slim.

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::ui::dashboard::close_button_rect;
use crate::ui::pending_actions::{PendingActionsOverlay, PendingItem, pending_actions_modal_rects};

/// Outcome of a key press while the overlay is open.
///
/// - `None`         — event consumed, no further action needed.
/// - `Closed`       — overlay was closed (already reflected in overlay state).
/// - `AcceptItems`  — operator confirmed accept for these item indices (Task 10/12).
/// - `ClearItems`   — operator confirmed clear/reject for these item indices (Task 10/12).
/// - `CopyItem`     — operator confirmed copy for this item index (Task 10/12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingActionsOutcome {
    None,
    Closed,
    AcceptItems(Vec<usize>),
    ClearItems(Vec<usize>),
    CopyItem(usize),
}

pub fn handle_pending_actions_overlay_key(
    overlay: &mut PendingActionsOverlay,
    items: &[PendingItem],
    code: KeyCode,
) -> PendingActionsOutcome {
    if !overlay.is_open() {
        return PendingActionsOutcome::None;
    }
    match code {
        KeyCode::Char('[') => {
            overlay.shrink();
            PendingActionsOutcome::None
        }
        KeyCode::Char(']') => {
            overlay.grow();
            PendingActionsOutcome::None
        }
        KeyCode::Char('=') => {
            overlay.reset_size();
            PendingActionsOutcome::None
        }
        KeyCode::Char(',') => {
            overlay.narrow_list();
            PendingActionsOutcome::None
        }
        KeyCode::Char('.') => {
            overlay.widen_list();
            PendingActionsOutcome::None
        }
        KeyCode::Char('a') | KeyCode::Char('q') | KeyCode::Esc => {
            overlay.close();
            PendingActionsOutcome::Closed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            overlay.select_prev(items.len());
            PendingActionsOutcome::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            overlay.select_next(items.len());
            PendingActionsOutcome::None
        }
        KeyCode::Char(' ') => {
            if let Some(item) = items.get(overlay.selected()) {
                overlay.toggle_multi(item);
            }
            PendingActionsOutcome::None
        }
        KeyCode::Char('P') => {
            overlay.toggle_group_proposals(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('Y') => {
            overlay.toggle_group_alerts(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('A') => {
            overlay.toggle_group_all(items);
            PendingActionsOutcome::None
        }
        KeyCode::Char('c') => {
            overlay.clear_multi();
            PendingActionsOutcome::None
        }
        KeyCode::Char('p') => dispatch_accept(overlay, items),
        KeyCode::Char('d') => dispatch_clear(overlay, items),
        KeyCode::Char('y') => dispatch_copy(overlay, items),
        KeyCode::Enter => PendingActionsOutcome::None,
        _ => PendingActionsOutcome::None,
    }
}

/// Process a mouse event while the overlay is open.
///
/// - A left-click on the `[x]` close button closes the modal.
/// - A left-click in the list pane on cols 0..4 from `list.x` toggles
///   the multi-selection for that row without moving the cursor.
/// - A left-click on cols 4+ moves the cursor to that row.
/// - Scroll wheel moves the cursor up/down.
/// - Every other event is swallowed so it cannot leak through to the
///   dashboard.
pub fn handle_pending_actions_overlay_mouse(
    overlay: &mut PendingActionsOverlay,
    viewport: Rect,
    items: &[PendingItem],
    event: MouseEvent,
) -> PendingActionsOutcome {
    if !overlay.is_open() {
        return PendingActionsOutcome::None;
    }
    let rects = pending_actions_modal_rects(viewport, overlay);

    // [x] close button
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && crate::app::keymap::rect_contains(close_button_rect(rects.area), event.column, event.row)
    {
        overlay.close();
        return PendingActionsOutcome::Closed;
    }

    // Separator-zone resize drag. Active only in wide mode (when the
    // explainer pane is to the right of the list pane). Hit zone is
    // 3 cells: [separator_col - 1, separator_col + 2). We check this
    // BEFORE the title-row drag so a click on the separator never
    // accidentally starts a move-drag (separator lives in body rows
    // anyway, but the ordering keeps intent explicit).
    let wide_mode = rects.explainer.x > rects.list.x;
    if wide_mode {
        let separator_col = rects.explainer.x;
        if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
            && event.row > rects.area.y // not the title row
            && event.row + 1 < rects.area.y + rects.area.height // not the hint row
            && event.column + 1 >= separator_col
            && event.column < separator_col + 2
        {
            overlay.begin_resize_drag(crate::ui::pending_actions::ResizeDragAnchor {
                start_col: event.column,
                start_list_width: rects.list.width,
            });
            return PendingActionsOutcome::None;
        }
    }

    // Title-row drag (move modal). Down(Left) on top border row outside [x]
    // starts the drag; Drag(Left) updates offset; Up(Left) (or any non-Drag)
    // clears the anchor. Mirrors `handle_metrics_overlay_mouse`.
    let close = close_button_rect(rects.area);
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && event.row == rects.area.y
        && event.column >= rects.area.x
        && event.column < rects.area.x + rects.area.width
        && !crate::app::keymap::rect_contains(close, event.column, event.row)
    {
        overlay.begin_move_drag(crate::ui::pending_actions::MoveDragAnchor {
            start_col: event.column,
            start_row: event.row,
            start_offset_x: overlay.offset_x(),
            start_offset_y: overlay.offset_y(),
        });
        return PendingActionsOutcome::None;
    }

    // Drag dispatch — resize first (so a separator drag can't be
    // shadowed by a stray move anchor), then move. Each branch only
    // fires when its anchor is set.
    if let MouseEventKind::Drag(MouseButton::Left) = event.kind {
        if let Some(anchor) = overlay.resize_drag_anchor() {
            let delta = (event.column as i32) - (anchor.start_col as i32);
            let new_w = (anchor.start_list_width as i32 + delta).max(0) as u16;
            overlay.set_list_width(new_w);
            return PendingActionsOutcome::None;
        }
        if let Some(anchor) = overlay.move_drag_anchor() {
            let dx = (event.column as i32) - (anchor.start_col as i32);
            let dy = (event.row as i32) - (anchor.start_row as i32);
            let new_x = (anchor.start_offset_x as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32);
            let new_y = (anchor.start_offset_y as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32);
            overlay.set_offset(new_x as i16, new_y as i16);
            return PendingActionsOutcome::None;
        }
    }

    // Drag end / non-Drag event clears both anchors.
    if !matches!(event.kind, MouseEventKind::Drag(_)) {
        overlay.end_drag();
    }

    // List pane click: distinguish checkbox vs content.
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && crate::app::keymap::rect_contains(rects.list, event.column, event.row)
    {
        let leading_pad: u16 = 1;
        if event.row >= rects.list.y + leading_pad {
            let row_off = event.row - rects.list.y - leading_pad;
            let idx = row_off as usize;
            if idx < items.len() {
                let col_off = event.column.saturating_sub(rects.list.x);
                if col_off < 4 {
                    // checkbox zone (cols 0..4 from list.x — `[x]` glyph + trailing space)
                    if let Some(item) = items.get(idx) {
                        overlay.toggle_multi(item);
                    }
                } else {
                    // content / cursor zone (cols 4+)
                    overlay.set_selected(idx);
                }
            }
        }
        return PendingActionsOutcome::None;
    }

    // Wheel inside the modal scrolls the cursor (spec §5.9).
    if crate::app::keymap::rect_contains(rects.area, event.column, event.row) {
        match event.kind {
            MouseEventKind::ScrollUp => overlay.select_prev(items.len()),
            MouseEventKind::ScrollDown => overlay.select_next(items.len()),
            _ => {}
        }
    }

    // Swallow everything else (no leak to dashboard).
    PendingActionsOutcome::None
}

use crate::app::action_explainer::{
    ActionExplainView, PendingAction, build_accept_view, build_copy_view,
};
use crate::app::config::ActionsMode;
use crate::app::event_loop::PaneReport;

/// Build the `AcceptPromptSend` action for a proposal item, or `None`
/// if the item is not a proposal. Used by the bulk `p` dispatch in
/// `tui_loop`.
pub fn accept_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Proposal {
            target_pane_id,
            slash_command,
            proposal_id,
            ..
        } => Some(PendingAction::AcceptPromptSend {
            target_pane_id: target_pane_id.clone(),
            slash_command: slash_command.clone(),
            proposal_id: proposal_id.clone(),
        }),
        PendingItem::Copy { .. } => None,
    }
}

/// Build the `RejectPromptSend` action for a proposal item, or `None`
/// for non-proposal items. Used by the bulk `d` dispatch when the
/// item is a proposal (alerts use the hide path instead).
pub fn reject_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Proposal {
            target_pane_id,
            slash_command,
            proposal_id,
            ..
        } => Some(PendingAction::RejectPromptSend {
            target_pane_id: target_pane_id.clone(),
            slash_command: slash_command.clone(),
            proposal_id: proposal_id.clone(),
        }),
        PendingItem::Copy { .. } => None,
    }
}

/// Build the `CopyAlertCommand` action for an alert item, or `None`
/// for proposals. Used by the bulk `y` dispatch.
pub fn copy_action_for(item: &PendingItem) -> Option<PendingAction> {
    match item {
        PendingItem::Copy {
            command,
            alert_title,
            ..
        } => Some(PendingAction::CopyAlertCommand {
            command: command.clone(),
            alert_title: alert_title.clone(),
        }),
        PendingItem::Proposal { .. } => None,
    }
}

/// Build an `ActionExplainView` for the right-pane live panel. For
/// proposals this is the accept-flavored view (the "primary" action a
/// `p` keypress would do); the operator can still press `d` to reject
/// without changing what the panel shows. Returns `None` when the
/// caller should render the empty-state placeholder.
pub fn build_explainer_view_for_item(
    item: &PendingItem,
    reports: &[PaneReport],
    mode: ActionsMode,
    allow_auto_prompt_send: bool,
) -> Option<ActionExplainView> {
    match item {
        PendingItem::Proposal {
            pane_idx,
            slash_command,
            ..
        } => {
            let report = reports.get(*pane_idx)?;
            Some(build_accept_view(
                report,
                slash_command,
                mode,
                allow_auto_prompt_send,
            ))
        }
        PendingItem::Copy {
            command,
            alert_title,
            severity,
            source,
            ..
        } => Some(build_copy_view(
            alert_title,
            command,
            Some(*severity),
            *source,
        )),
    }
}

fn dispatch_accept(
    overlay: &PendingActionsOverlay,
    items: &[PendingItem],
) -> PendingActionsOutcome {
    let idxs = if overlay.multi_len() == 0 {
        match items.get(overlay.selected()) {
            Some(PendingItem::Proposal { .. }) => vec![overlay.selected()],
            _ => Vec::new(),
        }
    } else {
        let mut out: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| matches!(it, PendingItem::Proposal { .. }))
            .filter(|(_, it)| {
                overlay.multi_contains(&crate::ui::pending_actions::pending_item_key(it))
            })
            .map(|(idx, _)| idx)
            .collect();
        out.sort_unstable();
        out
    };
    if idxs.is_empty() {
        PendingActionsOutcome::None
    } else {
        PendingActionsOutcome::AcceptItems(idxs)
    }
}

fn dispatch_clear(overlay: &PendingActionsOverlay, items: &[PendingItem]) -> PendingActionsOutcome {
    let idxs = if overlay.multi_len() == 0 {
        if items.get(overlay.selected()).is_some() {
            vec![overlay.selected()]
        } else {
            Vec::new()
        }
    } else {
        let mut out: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                overlay.multi_contains(&crate::ui::pending_actions::pending_item_key(it))
            })
            .map(|(idx, _)| idx)
            .collect();
        out.sort_unstable();
        out
    };
    if idxs.is_empty() {
        PendingActionsOutcome::None
    } else {
        PendingActionsOutcome::ClearItems(idxs)
    }
}

fn dispatch_copy(overlay: &PendingActionsOverlay, items: &[PendingItem]) -> PendingActionsOutcome {
    if overlay.multi_len() == 0 {
        return match items.get(overlay.selected()) {
            Some(PendingItem::Copy { .. }) => PendingActionsOutcome::CopyItem(overlay.selected()),
            _ => PendingActionsOutcome::None,
        };
    }
    let first = items.iter().enumerate().find(|(_, it)| {
        matches!(it, PendingItem::Copy { .. })
            && overlay.multi_contains(&crate::ui::pending_actions::pending_item_key(it))
    });
    match first {
        Some((idx, _)) => PendingActionsOutcome::CopyItem(idx),
        None => PendingActionsOutcome::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_three_proposals() -> Vec<crate::ui::pending_actions::PendingItem> {
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
        (0..3)
            .map(|i| PendingItem::Proposal {
                pane_idx: i,
                pane_label: format!("pane {i}"),
                slash_command: "/compact".into(),
                severity: None,
                source: SourceKind::ProjectCanonical,
                proposal_id: format!("%{i}:/compact"),
                target_pane_id: format!("%{i}"),
            })
            .collect()
    }

    #[test]
    fn closed_overlay_swallows_keys() {
        let mut overlay = PendingActionsOverlay::new();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Enter);
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert!(!overlay.is_open());
    }

    #[test]
    fn a_key_closes_overlay() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('a'));
        assert_eq!(outcome, PendingActionsOutcome::Closed);
        assert!(!overlay.is_open());
    }

    #[test]
    fn q_and_esc_close_overlay() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Esc);
        assert_eq!(outcome, PendingActionsOutcome::Closed);
        assert!(!overlay.is_open());

        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('q'));
        assert_eq!(outcome, PendingActionsOutcome::Closed);
        assert!(!overlay.is_open());
    }

    #[test]
    fn arrow_keys_navigate() {
        let items = make_three_proposals();
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Down);
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert_eq!(overlay.selected(), 1);
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Up);
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert_eq!(overlay.selected(), 0);
    }

    #[test]
    fn jk_navigates_like_arrows() {
        let items = make_three_proposals();
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('j'));
        handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('j'));
        assert_eq!(overlay.selected(), 2);
        handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('k'));
        assert_eq!(overlay.selected(), 1);
    }

    #[test]
    fn enter_is_silently_swallowed() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Enter);
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert!(overlay.is_open());
    }

    #[test]
    fn accept_action_for_proposal_returns_accept_variant() {
        use crate::app::action_explainer::PendingAction;
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let action = accept_action_for(&item).expect("proposal must produce an Accept action");
        match action {
            PendingAction::AcceptPromptSend {
                target_pane_id,
                slash_command,
                proposal_id,
            } => {
                assert_eq!(target_pane_id, "%1");
                assert_eq!(slash_command, "/compact");
                assert_eq!(proposal_id, "%1:/compact");
            }
            _ => panic!("expected AcceptPromptSend"),
        }
    }

    #[test]
    fn accept_action_for_alert_returns_none() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "ctx".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        assert!(accept_action_for(&item).is_none());
    }

    #[test]
    fn reject_action_for_proposal_returns_reject_variant() {
        use crate::app::action_explainer::PendingAction;
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Proposal {
            pane_idx: 0,
            pane_label: "claude:1:main · %1".into(),
            slash_command: "/compact".into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: "%1:/compact".into(),
            target_pane_id: "%1".into(),
        };
        let action = reject_action_for(&item).expect("proposal must produce a Reject action");
        assert!(matches!(action, PendingAction::RejectPromptSend { .. }));
    }

    #[test]
    fn copy_action_for_alert_returns_copy_variant() {
        use crate::app::action_explainer::PendingAction;
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        use crate::ui::pending_actions::PendingItem;
        let item = PendingItem::Copy {
            alert_idx: 0,
            command: "/clear".into(),
            alert_title: "ctx".into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        };
        let action = copy_action_for(&item).expect("alert must produce a Copy action");
        match action {
            PendingAction::CopyAlertCommand {
                command,
                alert_title,
            } => {
                assert_eq!(command, "/clear");
                assert_eq!(alert_title, "ctx");
            }
            _ => panic!("expected CopyAlertCommand"),
        }
    }

    #[test]
    fn space_toggles_multi_on_cursor_item() {
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::{PendingItem, pending_item_key};
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
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char(' '),
        );
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert!(overlay.multi_contains(&pending_item_key(&item)));
    }

    #[test]
    fn shift_p_toggles_proposal_group() {
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
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
        overlay.open();
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char('P'),
        );
        assert_eq!(overlay.multi_len(), 1);
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char('P'),
        );
        assert_eq!(overlay.multi_len(), 0);
    }

    fn fixture_proposal(id: &str, command: &str) -> crate::ui::pending_actions::PendingItem {
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
        PendingItem::Proposal {
            pane_idx: 0,
            pane_label: format!("claude:1:main · {id}"),
            slash_command: command.into(),
            severity: None,
            source: SourceKind::ProjectCanonical,
            proposal_id: format!("{id}:{command}"),
            target_pane_id: id.into(),
        }
    }

    fn fixture_copy(title: &str, command: &str) -> crate::ui::pending_actions::PendingItem {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;
        use crate::ui::pending_actions::PendingItem;
        PendingItem::Copy {
            alert_idx: 0,
            command: command.into(),
            alert_title: title.into(),
            severity: Severity::Warning,
            source: SourceKind::Estimated,
            pane_idx: None,
        }
    }

    #[test]
    fn p_no_multi_returns_cursor_proposal() {
        let items = vec![fixture_proposal("%1", "/compact")];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('p'));
        assert_eq!(outcome, PendingActionsOutcome::AcceptItems(vec![0]));
    }

    #[test]
    fn p_no_multi_with_alert_cursor_returns_none() {
        let items = vec![fixture_copy("ctx", "/clear")];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('p'));
        assert_eq!(outcome, PendingActionsOutcome::None);
    }

    #[test]
    fn p_with_multi_returns_proposal_indices_only() {
        let items = vec![
            fixture_proposal("%1", "/compact"),
            fixture_copy("ctx", "/clear"),
            fixture_proposal("%2", "/clear"),
        ];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.toggle_group_all(&items);
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('p'));
        assert_eq!(outcome, PendingActionsOutcome::AcceptItems(vec![0, 2]));
    }

    #[test]
    fn d_no_multi_returns_cursor() {
        let items = vec![fixture_copy("ctx", "/clear")];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('d'));
        assert_eq!(outcome, PendingActionsOutcome::ClearItems(vec![0]));
    }

    #[test]
    fn d_with_multi_returns_all_indices_ascending() {
        let items = vec![
            fixture_proposal("%1", "/compact"),
            fixture_copy("ctx", "/clear"),
            fixture_proposal("%2", "/clear"),
        ];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.toggle_group_all(&items);
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('d'));
        assert_eq!(outcome, PendingActionsOutcome::ClearItems(vec![0, 1, 2]));
    }

    #[test]
    fn y_with_multi_picks_first_alert() {
        let items = vec![
            fixture_proposal("%1", "/compact"),
            fixture_copy("ctx-a", "/clear"),
            fixture_copy("ctx-b", "/compact"),
        ];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.toggle_group_all(&items);
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('y'));
        assert_eq!(outcome, PendingActionsOutcome::CopyItem(1));
    }

    #[test]
    fn y_no_multi_with_proposal_cursor_returns_none() {
        let items = vec![fixture_proposal("%1", "/compact")];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let outcome = handle_pending_actions_overlay_key(&mut overlay, &items, KeyCode::Char('y'));
        assert_eq!(outcome, PendingActionsOutcome::None);
    }

    #[test]
    fn c_clears_multi_only_keeps_cursor() {
        use crate::domain::origin::SourceKind;
        use crate::ui::pending_actions::PendingItem;
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
        overlay.open();
        overlay.set_selected(0);
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char(' '),
        );
        assert_eq!(overlay.multi_len(), 1);
        handle_pending_actions_overlay_key(
            &mut overlay,
            std::slice::from_ref(&item),
            KeyCode::Char('c'),
        );
        assert_eq!(overlay.multi_len(), 0);
        assert_eq!(overlay.selected(), 0, "c does not move the cursor");
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    #[test]
    fn click_checkbox_toggles_multi_keeps_cursor() {
        use crate::ui::pending_actions::pending_actions_modal_rects;
        let items = vec![fixture_proposal("%1", "/compact")];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.set_selected(0);
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        // Row 0 lives at rects.list.y + 1 (1-row top padding inside list pane).
        let row = rects.list.y + 1;
        let col = rects.list.x + 1; // inside [x] glyph (cols 0..3 from list.x)
        let outcome = handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(MouseEventKind::Down(MouseButton::Left), col, row),
        );
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert_eq!(overlay.multi_len(), 1, "checkbox click toggles multi");
        assert_eq!(overlay.selected(), 0, "cursor unchanged");
    }

    #[test]
    fn click_content_moves_cursor() {
        use crate::ui::pending_actions::pending_actions_modal_rects;
        let items = vec![
            fixture_proposal("%1", "/compact"),
            fixture_proposal("%2", "/clear"),
        ];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.set_selected(0);
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        let row = rects.list.y + 1 + 1; // second item
        let col = rects.list.x + 10; // content area (>= 4)
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(MouseEventKind::Down(MouseButton::Left), col, row),
        );
        assert_eq!(overlay.selected(), 1);
        assert_eq!(overlay.multi_len(), 0);
    }

    #[test]
    fn wheel_moves_cursor() {
        use crate::ui::pending_actions::pending_actions_modal_rects;
        let items = vec![
            fixture_proposal("%1", "/compact"),
            fixture_proposal("%2", "/clear"),
        ];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.set_selected(0);
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        let inside_col = rects.area.x + 5;
        let inside_row = rects.area.y + 5;
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(MouseEventKind::ScrollDown, inside_col, inside_row),
        );
        assert_eq!(overlay.selected(), 1);
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(MouseEventKind::ScrollUp, inside_col, inside_row),
        );
        assert_eq!(overlay.selected(), 0);
    }

    #[test]
    fn click_close_button_closes() {
        use crate::ui::dashboard::close_button_rect;
        use crate::ui::pending_actions::pending_actions_modal_rects;
        let items = vec![fixture_proposal("%1", "/compact")];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        let close = close_button_rect(rects.area);
        let outcome = handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
        );
        assert_eq!(outcome, PendingActionsOutcome::Closed);
        assert!(!overlay.is_open());
    }

    #[test]
    fn click_explainer_is_noop() {
        use crate::ui::pending_actions::pending_actions_modal_rects;
        let items = vec![fixture_proposal("%1", "/compact")];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        let outcome = handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                rects.explainer.x + 5,
                rects.explainer.y + 1,
            ),
        );
        assert_eq!(outcome, PendingActionsOutcome::None);
        assert_eq!(overlay.selected(), 0);
        assert_eq!(overlay.multi_len(), 0);
        assert!(
            overlay.is_open(),
            "click on explainer must NOT close the overlay"
        );
    }

    #[test]
    fn wheel_outside_modal_is_noop() {
        let items = vec![
            fixture_proposal("%1", "/compact"),
            fixture_proposal("%2", "/clear"),
        ];
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        overlay.set_selected(0);
        let viewport = Rect::new(0, 0, 200, 80);
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(MouseEventKind::ScrollDown, 0, 0),
        );
        assert_eq!(
            overlay.selected(),
            0,
            "wheel outside modal must not scroll the overlay's cursor"
        );
    }

    #[test]
    fn bracket_keys_resize_modal() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let (w0, _h0) = (overlay.width_pct(), overlay.height_pct());
        handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('['));
        assert!(overlay.width_pct() < w0);
        handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char(']'));
        handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char(']'));
        assert!(overlay.width_pct() > w0);
        handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('='));
        assert_eq!(overlay.width_pct(), 80);
        assert_eq!(overlay.height_pct(), 65);
    }

    #[test]
    fn title_row_drag_updates_offset() {
        use crate::ui::pending_actions::pending_actions_modal_rects;
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        let title_col = rects.area.x + 5;
        let title_row = rects.area.y;
        let items: Vec<crate::ui::pending_actions::PendingItem> = Vec::new();
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                title_col,
                title_row,
            ),
        );
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                title_col + 4,
                title_row + 2,
            ),
        );
        assert_eq!(overlay.offset_x(), 4);
        assert_eq!(overlay.offset_y(), 2);
    }

    #[test]
    fn comma_period_keys_resize_list() {
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char(','));
        let after_narrow = overlay.list_width_override();
        handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('.'));
        handle_pending_actions_overlay_key(&mut overlay, &[], KeyCode::Char('.'));
        let after_widen = overlay.list_width_override();
        assert!(after_narrow.is_some() && after_widen.is_some());
        assert!(after_widen.unwrap() > after_narrow.unwrap());
    }

    #[test]
    fn separator_drag_updates_list_width() {
        use crate::ui::pending_actions::{
            LIST_WIDTH_WIDE_MAX, pending_actions_modal_rects, wide_mode_list_width,
        };
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        let separator_col = rects.explainer.x;
        let drag_row = rects.area.y + 5;
        let items: Vec<crate::ui::pending_actions::PendingItem> = Vec::new();
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                separator_col,
                drag_row,
            ),
        );
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                separator_col + 5,
                drag_row,
            ),
        );
        // List width should have grown (clamped to range).
        assert!(overlay.list_width_override().is_some());
        let new_w = overlay.list_width_override().unwrap();
        let auto_w = wide_mode_list_width(rects.area.width.saturating_sub(2), None);
        assert!(
            new_w > auto_w || new_w == LIST_WIDTH_WIDE_MAX,
            "drag right should widen list (or hit max): auto={auto_w}, new={new_w}"
        );
    }

    #[test]
    fn separator_drag_does_not_start_in_title_or_body() {
        use crate::ui::pending_actions::pending_actions_modal_rects;
        let mut overlay = PendingActionsOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 200, 80);
        let rects = pending_actions_modal_rects(viewport, &overlay);
        let items: Vec<crate::ui::pending_actions::PendingItem> = Vec::new();
        // Click in the body, not on the separator zone.
        let body_col = rects.list.x + 5; // well inside the list
        let body_row = rects.area.y + 3;
        handle_pending_actions_overlay_mouse(
            &mut overlay,
            viewport,
            &items,
            mouse_event(MouseEventKind::Down(MouseButton::Left), body_col, body_row),
        );
        assert!(
            overlay.resize_drag_anchor().is_none(),
            "click in list body must not start separator drag"
        );
    }
}
