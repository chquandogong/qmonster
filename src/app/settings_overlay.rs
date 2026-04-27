use std::path::Path;

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::config::QmonsterConfig;
use crate::app::keymap::rect_contains;
use crate::ui::settings::{SettingsOverlay, settings_close_button_rect, settings_modal_rects};

const NO_CONFIG_PATH_SAVE_ERROR: &str =
    "no config path \u{2014} restart with `--config PATH` to enable save";

pub fn handle_settings_overlay_key(
    overlay: &mut SettingsOverlay,
    config: &mut QmonsterConfig,
    config_path: Option<&Path>,
    code: KeyCode,
) -> bool {
    if !overlay.is_open() {
        return false;
    }

    let editing = overlay.edit_buffer().is_some();
    match code {
        KeyCode::Esc => {
            if editing {
                overlay.cancel_edit();
            } else {
                overlay.close();
            }
        }
        KeyCode::Char('q') if !editing => overlay.close(),
        KeyCode::Up if !editing => overlay.prev_field(),
        KeyCode::Down if !editing => overlay.next_field(),
        KeyCode::Left if !editing => overlay.prev_field(),
        KeyCode::Right if !editing => overlay.next_field(),
        KeyCode::Char('e') if !editing => overlay.start_edit(config),
        KeyCode::Char('c') if !editing => overlay.clear_override(config),
        KeyCode::Char('w') if !editing => {
            if let Some(path) = config_path {
                let _ = overlay.save(config, path);
            } else {
                overlay.set_save_error(NO_CONFIG_PATH_SAVE_ERROR.to_string());
            }
        }
        KeyCode::Enter => {
            if editing {
                let _ = overlay.commit_edit(config);
            } else {
                overlay.start_edit(config);
            }
        }
        KeyCode::Backspace if editing => overlay.backspace(),
        KeyCode::Char(c) if editing => overlay.type_char(c),
        _ => {}
    }
    true
}

pub fn handle_settings_overlay_mouse(
    overlay: &mut SettingsOverlay,
    viewport: Rect,
    event: MouseEvent,
) -> bool {
    if !overlay.is_open() {
        return false;
    }

    let rects = settings_modal_rects(viewport);
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && rect_contains(
            settings_close_button_rect(rects.body),
            event.column,
            event.row,
        )
    {
        overlay.close();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEventKind};

    use crate::ui::settings::SettingsStatus;

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
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();

        assert!(!handle_settings_overlay_key(
            &mut overlay,
            &mut config,
            None,
            KeyCode::Esc,
        ));
    }

    #[test]
    fn escape_cancels_edit_before_closing_overlay() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('e'));
        assert!(overlay.edit_buffer().is_some());

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Esc);
        assert!(overlay.is_open());
        assert!(overlay.edit_buffer().is_none());

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Esc);
        assert!(!overlay.is_open());
    }

    #[test]
    fn save_without_config_path_surfaces_status_error() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('w'));

        assert!(matches!(
            overlay.status(),
            SettingsStatus::Error(msg) if msg.contains("--config PATH")
        ));
    }

    #[test]
    fn mouse_handler_closes_on_close_button() {
        let mut overlay = SettingsOverlay::new();
        overlay.open();
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let close = settings_close_button_rect(rects.body);

        assert!(handle_settings_overlay_mouse(
            &mut overlay,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
        ));

        assert!(!overlay.is_open());
    }
}
