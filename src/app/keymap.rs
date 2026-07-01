use crossterm::event::MouseEvent;
use ratatui::layout::{Margin, Rect};
use ratatui::widgets::ListState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Alerts,
    Panes,
}

pub fn toggle_focus(focus: FocusedPanel) -> FocusedPanel {
    match focus {
        FocusedPanel::Alerts => FocusedPanel::Panes,
        FocusedPanel::Panes => FocusedPanel::Alerts,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDir {
    Up,
    Down,
}

pub fn move_selection(state: &mut ListState, item_count: usize, step: isize) {
    if item_count == 0 {
        state.select(None);
        return;
    }
    let current = state.selected().unwrap_or(0) as isize;
    let next = (current + step).clamp(0, item_count.saturating_sub(1) as isize) as usize;
    state.select(Some(next));
}

pub fn page_selection(state: &mut ListState, total: usize, page: usize, dir: ScrollDir) {
    if total == 0 {
        state.select(None);
        return;
    }
    let step = page.max(1) as isize;
    match dir {
        ScrollDir::Up => move_selection(state, total, -step),
        ScrollDir::Down => move_selection(state, total, step),
    }
}

/// Pane-list navigation with a single "nothing selected" (clean) state reachable
/// off BOTH ends of the list (v3.1.3). Unlike `move_selection` (which clamps to
/// `[0, total-1]` and treats `None` as `0`), the selection can leave the list off
/// either end → `None`, so the operator can collapse every pane back to the
/// compact default gauge-tile view. Directional re-entry: from the clean state a
/// downward move enters at the FIRST pane and an upward move at the LAST pane —
/// the key you press decides where you land, so the single clean state needs no
/// top/bottom indicator. Deselecting happens only when you step off an end you
/// are already sitting on; a page move that would overshoot from the interior
/// clamps to that end instead. `step` is ±1 for Up/Down and ±page for
/// PageUp/PageDown.
pub fn move_pane_selection(state: &mut ListState, total: usize, step: isize) {
    if total == 0 {
        state.select(None);
        return;
    }
    let last = total - 1;
    match state.selected() {
        // From the clean state: down enters at the first pane, up at the last.
        None => state.select(Some(if step > 0 { 0 } else { last })),
        Some(cur) => {
            let next = cur as isize + step;
            if next < 0 {
                // Up past the top: deselect only if already at the top; a page
                // overshoot from the interior clamps to the first pane instead.
                state.select(if cur == 0 { None } else { Some(0) });
            } else if next as usize > last {
                // Down past the bottom: deselect only if already at the bottom;
                // otherwise clamp to the last pane.
                state.select(if cur == last { None } else { Some(last) });
            } else {
                state.select(Some(next as usize));
            }
        }
    }
}

pub fn select_first(state: &mut ListState, total: usize) {
    if total == 0 {
        state.select(None);
        return;
    }
    state.select(Some(0));
}

pub fn select_last(state: &mut ListState, total: usize) {
    if total == 0 {
        state.select(None);
        return;
    }
    state.select(Some(total.saturating_sub(1)));
}

pub fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

pub fn list_row_at(rect: Rect, event: MouseEvent) -> Option<u16> {
    let inner = rect.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    rect_contains(inner, event.column, event.row).then_some(event.row.saturating_sub(inner.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

    #[test]
    fn toggle_focus_switches_between_dashboard_lists() {
        assert_eq!(toggle_focus(FocusedPanel::Alerts), FocusedPanel::Panes);
        assert_eq!(toggle_focus(FocusedPanel::Panes), FocusedPanel::Alerts);
    }

    #[test]
    fn move_selection_clamps_to_list_bounds() {
        let mut state = ListState::default();
        state.select(Some(1));

        move_selection(&mut state, 3, 10);
        assert_eq!(state.selected(), Some(2));

        move_selection(&mut state, 3, -10);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn move_pane_selection_deselects_off_either_end_with_directional_entry() {
        let mut s = ListState::default(); // None = clean / nothing selected

        // from clean: down enters at the FIRST pane, up enters at the LAST pane
        move_pane_selection(&mut s, 3, 1);
        assert_eq!(s.selected(), Some(0));
        s.select(None);
        move_pane_selection(&mut s, 3, -1);
        assert_eq!(s.selected(), Some(2));

        // symmetric deselect: up off the top → clean; down off the bottom → clean
        s.select(Some(0));
        move_pane_selection(&mut s, 3, -1);
        assert_eq!(s.selected(), None, "up from the first pane returns to clean");
        s.select(Some(2));
        move_pane_selection(&mut s, 3, 1);
        assert_eq!(s.selected(), None, "down from the last pane returns to clean");

        // interior steps just move
        s.select(Some(1));
        move_pane_selection(&mut s, 3, 1);
        assert_eq!(s.selected(), Some(2));
        move_pane_selection(&mut s, 3, -1);
        move_pane_selection(&mut s, 3, -1);
        assert_eq!(s.selected(), Some(0));

        // page overshoot FROM the interior clamps to the edge (does not skip to clean)
        s.select(Some(3));
        move_pane_selection(&mut s, 5, 3); // 3+3=6 > last 4, cur 3 != last → clamp to 4
        assert_eq!(s.selected(), Some(4));
        s.select(Some(1));
        move_pane_selection(&mut s, 5, -3); // 1-3=-2 < 0, cur 1 != 0 → clamp to 0
        assert_eq!(s.selected(), Some(0));

        // …but a page move FROM the edge deselects, and the two-press wrap holds
        move_pane_selection(&mut s, 5, -3); // at 0, up → None
        assert_eq!(s.selected(), None);
        move_pane_selection(&mut s, 5, -1); // clean + up → last (directional entry)
        assert_eq!(s.selected(), Some(4));

        // empty list always clears
        s.select(Some(0));
        move_pane_selection(&mut s, 0, 1);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn empty_selection_helpers_clear_selection() {
        let mut state = ListState::default();
        state.select(Some(2));

        move_selection(&mut state, 0, 1);
        assert_eq!(state.selected(), None);

        select_first(&mut state, 0);
        assert_eq!(state.selected(), None);

        select_last(&mut state, 0);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn page_selection_uses_minimum_step_of_one() {
        let mut state = ListState::default();
        state.select(Some(1));

        page_selection(&mut state, 5, 0, ScrollDir::Down);
        assert_eq!(state.selected(), Some(2));

        page_selection(&mut state, 5, 0, ScrollDir::Up);
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn list_row_at_ignores_block_border_and_returns_body_row() {
        let rect = Rect::new(10, 4, 30, 8);
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 6,
            modifiers: KeyModifiers::empty(),
        };
        assert_eq!(list_row_at(rect, event), Some(1));
    }

    #[test]
    fn rect_contains_excludes_coordinates_on_outer_edge() {
        let rect = Rect::new(2, 3, 4, 5);
        assert!(rect_contains(rect, 2, 3));
        assert!(rect_contains(rect, 5, 7));
        assert!(!rect_contains(rect, 6, 7));
        assert!(!rect_contains(rect, 5, 8));
    }
}
