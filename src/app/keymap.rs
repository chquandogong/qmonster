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
