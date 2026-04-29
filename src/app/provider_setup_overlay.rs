//! Phase G-1 Task 2 (v0.4.0): keymap dispatch for the
//! `ProviderSetupOverlay`. Mirrors the shape of
//! `app::settings_overlay::handle_settings_overlay_key` — returns
//! `false` when the overlay is closed (so the caller can fall
//! through to the dashboard's main key handler) and `true` once it
//! has consumed the keystroke.
//!
//! The overlay is read-only — it never writes provider config
//! files. Operators copy the displayed snippet and apply it
//! manually. So the handler does NOT need a `&mut QmonsterConfig`
//! the way `handle_settings_overlay_key` does.

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::keymap::rect_contains;
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::ui::dashboard::{
    close_button_rect, provider_setup_modal_rects, provider_setup_tab_index_at,
};
use crate::ui::provider_setup::{ProviderSetupOverlay, ProviderSetupTab, snippet_for_tab};

const TAB_BY_INDEX: [ProviderSetupTab; 3] = [
    ProviderSetupTab::Claude,
    ProviderSetupTab::Codex,
    ProviderSetupTab::Gemini,
];

/// Dispatch a single key event to the Provider Setup overlay.
/// Returns `true` if the overlay was open and the key was handled
/// (or ignored) by the overlay; returns `false` when the overlay
/// is closed so the caller can pass the key through to the main
/// dashboard handler.
pub fn handle_provider_setup_overlay_key(
    overlay: &mut ProviderSetupOverlay,
    code: KeyCode,
) -> bool {
    if !overlay.is_open() {
        return false;
    }
    match code {
        KeyCode::Esc | KeyCode::Char('q') => overlay.close(),
        KeyCode::Char('1') => overlay.switch_tab(ProviderSetupTab::Claude),
        KeyCode::Char('2') => overlay.switch_tab(ProviderSetupTab::Codex),
        KeyCode::Char('3') => overlay.switch_tab(ProviderSetupTab::Gemini),
        KeyCode::Char('s') => overlay.toggle(),
        KeyCode::Up | KeyCode::Char('k') => overlay.scroll_up(),
        KeyCode::Down | KeyCode::Char('j') => overlay.scroll_down(),
        _ => {}
    }
    true
}

/// Copy the active tab's snippet to the clipboard via the supplied
/// `copy_text` function and produce a SystemNotice describing the
/// outcome. Pure with respect to the clipboard backend so tests can
/// inject a stub. The Gemini auth note is informational and is
/// excluded by `snippet_for_tab` — only config snippets are copied.
pub fn copy_active_tab_snippet<F>(overlay: &ProviderSetupOverlay, copy_text: F) -> SystemNotice
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let (label, text) = snippet_for_tab(overlay);
    let char_count = text.chars().count();
    match copy_text(&text) {
        Ok(()) => SystemNotice {
            title: format!("{label} copied"),
            body: format!("{char_count} chars in clipboard — paste into your editor"),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
        },
        Err(e) => SystemNotice {
            title: "clipboard unavailable".into(),
            body: format!("could not copy {label}: {e}"),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
        },
    }
}

/// Dispatch a mouse event to the Provider Setup overlay. Returns
/// `true` if the overlay was open and consumed the event (so the
/// caller skips the dashboard's main mouse handler).
///
/// Behaviors:
/// - Left-click on the `[x]` button (top-right of the tabs row): close.
/// - Left-click anywhere else on the tabs row: switch to the tab whose
///   horizontal slot was clicked (3-way equal split).
/// - Scroll wheel up/down anywhere over the modal body: scroll content.
pub fn handle_provider_setup_overlay_mouse(
    overlay: &mut ProviderSetupOverlay,
    viewport: Rect,
    event: MouseEvent,
) -> bool {
    if !overlay.is_open() {
        return false;
    }
    let rects = provider_setup_modal_rects(viewport);
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if rect_contains(close_button_rect(rects.tabs), event.column, event.row) {
                overlay.close();
            } else if rect_contains(rects.tabs, event.column, event.row)
                && let Some(idx) = provider_setup_tab_index_at(rects.tabs, event.column)
            {
                overlay.switch_tab(TAB_BY_INDEX[idx]);
            }
        }
        MouseEventKind::ScrollUp if rect_contains(rects.body, event.column, event.row) => {
            overlay.scroll_up();
        }
        MouseEventKind::ScrollDown if rect_contains(rects.body, event.column, event.row) => {
            overlay.scroll_down();
        }
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn key_handler_returns_false_when_overlay_is_closed() {
        let mut overlay = ProviderSetupOverlay::new();
        assert!(!handle_provider_setup_overlay_key(
            &mut overlay,
            KeyCode::Char('1')
        ));
    }

    #[test]
    fn esc_closes_overlay() {
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        assert!(handle_provider_setup_overlay_key(
            &mut overlay,
            KeyCode::Esc
        ));
        assert!(!overlay.is_open());
    }

    #[test]
    fn q_closes_overlay() {
        // Mirror the `SettingsOverlay` close affordance: both `Esc`
        // and `q` terminate the modal. Without this assertion an
        // operator who learned to close Settings with `q` would get
        // a confusing no-op when they tried the same key on Provider
        // Setup.
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        assert!(handle_provider_setup_overlay_key(
            &mut overlay,
            KeyCode::Char('q')
        ));
        assert!(!overlay.is_open());
    }

    #[test]
    fn number_keys_switch_tabs() {
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        handle_provider_setup_overlay_key(&mut overlay, KeyCode::Char('2'));
        assert_eq!(overlay.tab, ProviderSetupTab::Codex);
        handle_provider_setup_overlay_key(&mut overlay, KeyCode::Char('3'));
        assert_eq!(overlay.tab, ProviderSetupTab::Gemini);
        handle_provider_setup_overlay_key(&mut overlay, KeyCode::Char('1'));
        assert_eq!(overlay.tab, ProviderSetupTab::Claude);
    }

    #[test]
    fn s_toggles_per_tab_state() {
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        assert!(!overlay.claude_sidefile_enabled);
        handle_provider_setup_overlay_key(&mut overlay, KeyCode::Char('s'));
        assert!(overlay.claude_sidefile_enabled);
    }

    #[test]
    fn arrow_keys_scroll() {
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        handle_provider_setup_overlay_key(&mut overlay, KeyCode::Down);
        handle_provider_setup_overlay_key(&mut overlay, KeyCode::Down);
        assert_eq!(overlay.scroll_offset, 2);
        handle_provider_setup_overlay_key(&mut overlay, KeyCode::Up);
        assert_eq!(overlay.scroll_offset, 1);
    }

    #[test]
    fn mouse_handler_returns_false_when_overlay_is_closed() {
        let mut overlay = ProviderSetupOverlay::new();
        let viewport = Rect::new(0, 0, 120, 40);
        assert!(!handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), 10, 10),
        ));
    }

    #[test]
    fn left_click_on_close_button_closes_overlay() {
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = provider_setup_modal_rects(viewport);
        let close = close_button_rect(rects.tabs);
        assert!(handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
        ));
        assert!(!overlay.is_open());
    }

    #[test]
    fn left_click_on_tabs_row_switches_tabs() {
        // Click positions target the actual rendered label cells of
        // the ratatui Tabs widget (left-aligned ` Claude │ Codex │
        // Gemini ` inside a single-cell border), not the equal-thirds
        // approximation that an earlier implementation used. A wide
        // modal would otherwise mis-route clicks on Codex/Gemini to
        // the always-Claude left third.
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = provider_setup_modal_rects(viewport);
        // Avoid the close button, which lives at the top-right corner of
        // the tabs row — click on the second visible row of the tab
        // block instead.
        let row = rects.tabs.y + 1;
        let inner_x = rects.tabs.x + 1;

        // Click on a 'd' in "Codex" (inner_x + 12).
        let codex_x = inner_x + 12;
        handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), codex_x, row),
        );
        assert_eq!(overlay.tab, ProviderSetupTab::Codex);

        // Click on a 'm' in "Gemini" (inner_x + 20).
        let gemini_x = inner_x + 20;
        handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), gemini_x, row),
        );
        assert_eq!(overlay.tab, ProviderSetupTab::Gemini);

        // Click on a 'l' in "Claude" (inner_x + 2).
        let claude_x = inner_x + 2;
        handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), claude_x, row),
        );
        assert_eq!(overlay.tab, ProviderSetupTab::Claude);

        // Click on whitespace past "Gemini" (inner_x + 60) does
        // nothing — current tab stays Claude.
        let empty_x = inner_x + 60;
        handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), empty_x, row),
        );
        assert_eq!(overlay.tab, ProviderSetupTab::Claude);
    }

    #[test]
    fn copy_active_tab_snippet_reports_success_with_clipboard_chars_count() {
        use std::cell::RefCell;
        let overlay = ProviderSetupOverlay::default();
        let captured: RefCell<Option<String>> = RefCell::new(None);
        let stub = |text: &str| -> Result<(), String> {
            *captured.borrow_mut() = Some(text.to_string());
            Ok(())
        };
        let notice = copy_active_tab_snippet(&overlay, stub);
        assert_eq!(notice.severity, Severity::Good);
        assert!(notice.title.contains("Claude statusline.sh"));
        let copied = captured.borrow().clone().expect("stub must capture text");
        assert!(copied.starts_with("#!/usr/bin/env bash"));
        assert!(
            !copied.contains("ai-cli-status/claude"),
            "default Claude snippet must not include the sidefile addon"
        );
    }

    #[test]
    fn copy_active_tab_snippet_reports_failure_when_clipboard_unavailable() {
        let overlay = ProviderSetupOverlay::default();
        let stub = |_: &str| -> Result<(), String> { Err("no clipboard backend".into()) };
        let notice = copy_active_tab_snippet(&overlay, stub);
        assert_eq!(notice.severity, Severity::Warning);
        assert!(notice.title.contains("clipboard unavailable"));
        assert!(notice.body.contains("no clipboard backend"));
    }

    #[test]
    fn copy_active_tab_snippet_includes_sidefile_addon_when_toggled() {
        let mut overlay = ProviderSetupOverlay::default();
        overlay.claude_sidefile_enabled = true;
        let stub = |text: &str| -> Result<(), String> {
            assert!(
                text.contains("ai-cli-status/claude"),
                "toggled-on snippet must carry the addon block to the clipboard: {text}"
            );
            Ok(())
        };
        copy_active_tab_snippet(&overlay, stub);
    }

    #[test]
    fn scroll_wheel_over_body_scrolls_content() {
        let mut overlay = ProviderSetupOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = provider_setup_modal_rects(viewport);
        let body_col = rects.body.x + rects.body.width / 2;
        let body_row = rects.body.y + rects.body.height / 2;
        handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::ScrollDown, body_col, body_row),
        );
        handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::ScrollDown, body_col, body_row),
        );
        assert_eq!(overlay.scroll_offset, 2);
        handle_provider_setup_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::ScrollUp, body_col, body_row),
        );
        assert_eq!(overlay.scroll_offset, 1);
    }
}
