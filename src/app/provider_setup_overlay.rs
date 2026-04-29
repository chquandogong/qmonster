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

use crossterm::event::KeyCode;

use crate::ui::provider_setup::{ProviderSetupOverlay, ProviderSetupTab};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
