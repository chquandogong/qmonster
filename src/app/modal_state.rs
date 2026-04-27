use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::keymap::rect_contains;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScrollModalState {
    open: bool,
    title: String,
    lines: Vec<String>,
    scroll: usize,
}

impl ScrollModalState {
    pub fn open(&mut self, title: impl Into<String>, lines: Vec<String>) {
        self.open = true;
        self.title = title.into();
        self.lines = lines;
        self.scroll = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.scroll = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    fn scroll_down(&mut self, amount: usize, max_scroll: usize) {
        self.scroll = self.scroll.saturating_add(amount).min(max_scroll);
    }
}

pub fn handle_scroll_modal_key(
    modal: &mut ScrollModalState,
    code: KeyCode,
    max_scroll: usize,
    extra_close_key: Option<KeyCode>,
) -> bool {
    if !modal.is_open() {
        return false;
    }

    if code == KeyCode::Esc || extra_close_key == Some(code) {
        modal.close();
        return true;
    }

    match code {
        KeyCode::Up | KeyCode::Char('k') => modal.scroll_up(1),
        KeyCode::Down | KeyCode::Char('j') => modal.scroll_down(1, max_scroll),
        KeyCode::PageUp => modal.scroll_up(8),
        KeyCode::PageDown => modal.scroll_down(8, max_scroll),
        KeyCode::Home => modal.scroll = 0,
        KeyCode::End => modal.scroll = max_scroll,
        _ => {}
    }
    true
}

pub fn handle_scroll_modal_mouse(
    modal: &mut ScrollModalState,
    event: MouseEvent,
    body: Rect,
    close_button: Rect,
    max_scroll: usize,
) -> bool {
    if !modal.is_open() {
        return false;
    }

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(close_button, event.column, event.row)
    {
        modal.close();
        return true;
    }

    if rect_contains(body, event.column, event.row) {
        match event.kind {
            MouseEventKind::ScrollUp => modal.scroll_up(1),
            MouseEventKind::ScrollDown => modal.scroll_down(1, max_scroll),
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEventKind};

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn open_resets_scroll_and_replaces_content() {
        let mut modal = ScrollModalState::default();
        modal.open("old", vec!["a".into()]);
        handle_scroll_modal_key(&mut modal, KeyCode::End, 10, None);

        modal.open("new", vec!["b".into(), "c".into()]);

        assert!(modal.is_open());
        assert_eq!(modal.title(), "new");
        assert_eq!(modal.lines(), ["b", "c"]);
        assert_eq!(modal.scroll(), 0);
    }

    #[test]
    fn key_handler_scrolls_clamps_and_closes() {
        let mut modal = ScrollModalState::default();
        modal.open("git", vec!["line".into()]);

        assert!(handle_scroll_modal_key(
            &mut modal,
            KeyCode::PageDown,
            3,
            None
        ));
        assert_eq!(modal.scroll(), 3);

        handle_scroll_modal_key(&mut modal, KeyCode::Down, 3, None);
        assert_eq!(modal.scroll(), 3);

        handle_scroll_modal_key(&mut modal, KeyCode::Up, 3, None);
        assert_eq!(modal.scroll(), 2);

        handle_scroll_modal_key(&mut modal, KeyCode::Esc, 3, None);
        assert!(!modal.is_open());
        assert_eq!(modal.scroll(), 0);
    }

    #[test]
    fn key_handler_supports_extra_close_key() {
        let mut modal = ScrollModalState::default();
        modal.open("help", vec![]);

        handle_scroll_modal_key(&mut modal, KeyCode::Char('?'), 0, Some(KeyCode::Char('?')));

        assert!(!modal.is_open());
    }

    #[test]
    fn mouse_handler_scrolls_body_and_closes_button() {
        let mut modal = ScrollModalState::default();
        modal.open("help", vec![]);
        let body = Rect::new(10, 10, 20, 10);
        let close = Rect::new(28, 10, 1, 1);

        assert!(handle_scroll_modal_mouse(
            &mut modal,
            mouse(MouseEventKind::ScrollDown, 12, 12),
            body,
            close,
            2,
        ));
        assert_eq!(modal.scroll(), 1);

        handle_scroll_modal_mouse(
            &mut modal,
            mouse(MouseEventKind::Down(MouseButton::Left), 28, 10),
            body,
            close,
            2,
        );

        assert!(!modal.is_open());
        assert_eq!(modal.scroll(), 0);
    }
}
