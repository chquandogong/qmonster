use std::path::Path;

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::config::QmonsterConfig;
use crate::app::keymap::rect_contains;
use crate::ui::settings::{
    SettingsOverlay, SettingsTab, settings_close_button_rect, settings_field_at,
    settings_integration_field_at, settings_modal_rects, settings_tab_index_at,
};

const NO_CONFIG_PATH_SAVE_ERROR: &str =
    "no config path \u{2014} restart with `--config PATH` to enable save";
const TAB_BY_INDEX: [SettingsTab; 4] = [
    SettingsTab::Thresholds,
    SettingsTab::Integrations,
    SettingsTab::Parameters,
    SettingsTab::Rules,
];

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
        KeyCode::Char('1') if !editing => overlay.switch_tab(SettingsTab::Thresholds),
        KeyCode::Char('2') if !editing => overlay.switch_tab(SettingsTab::Integrations),
        KeyCode::Char('3') if !editing => overlay.switch_tab(SettingsTab::Parameters),
        KeyCode::Char('4') if !editing => overlay.switch_tab(SettingsTab::Rules),
        KeyCode::Tab if !editing => overlay.next_tab(),
        KeyCode::BackTab if !editing => overlay.previous_tab(),
        KeyCode::Up if !editing => move_selection_up(overlay),
        KeyCode::Down if !editing => move_selection_down(overlay),
        KeyCode::Left if !editing => move_selection_up(overlay),
        KeyCode::Right if !editing => move_selection_down(overlay),
        KeyCode::Char('e') if !editing => edit_or_toggle(overlay, config),
        KeyCode::Char(' ') if !editing => {
            if overlay.tab() == SettingsTab::Integrations {
                overlay.toggle_integration(config);
            }
        }
        KeyCode::Char('c') if !editing && overlay.tab() == SettingsTab::Thresholds => {
            overlay.clear_override(config)
        }
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
            } else if overlay.tab() == SettingsTab::Integrations {
                overlay.toggle_integration(config);
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

fn move_selection_up(overlay: &mut SettingsOverlay) {
    match overlay.tab() {
        SettingsTab::Thresholds => overlay.prev_field(),
        SettingsTab::Integrations => overlay.prev_integration(),
        SettingsTab::Parameters | SettingsTab::Rules => {}
    }
}

fn move_selection_down(overlay: &mut SettingsOverlay) {
    match overlay.tab() {
        SettingsTab::Thresholds => overlay.next_field(),
        SettingsTab::Integrations => overlay.next_integration(),
        SettingsTab::Parameters | SettingsTab::Rules => {}
    }
}

fn edit_or_toggle(overlay: &mut SettingsOverlay, config: &mut QmonsterConfig) {
    match overlay.tab() {
        SettingsTab::Thresholds => overlay.start_edit(config),
        SettingsTab::Integrations => overlay.toggle_integration(config),
        SettingsTab::Parameters | SettingsTab::Rules => {}
    }
}

pub fn handle_settings_overlay_mouse(
    overlay: &mut SettingsOverlay,
    config: &mut QmonsterConfig,
    viewport: Rect,
    event: MouseEvent,
) -> bool {
    if !overlay.is_open() {
        return false;
    }

    let rects = settings_modal_rects(viewport);
    match event.kind {
        MouseEventKind::Down(MouseButton::Left)
            if rect_contains(
                settings_close_button_rect(rects.body),
                event.column,
                event.row,
            ) =>
        {
            overlay.close();
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if rect_contains(rects.tabs, event.column, event.row) {
                if let Some(idx) = settings_tab_index_at(rects.tabs, event.column) {
                    overlay.switch_tab(TAB_BY_INDEX[idx]);
                }
            } else if overlay.tab() == SettingsTab::Thresholds
                && let Some(field) = settings_field_at(rects.body, event.column, event.row)
            {
                overlay.select_field(field);
            } else if overlay.tab() == SettingsTab::Integrations
                && let Some(field) =
                    settings_integration_field_at(rects.body, event.column, event.row)
            {
                overlay.select_integration(field);
                overlay.toggle_integration(config);
            }
        }
        MouseEventKind::ScrollUp
            if overlay.tab() == SettingsTab::Thresholds
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.prev_field();
        }
        MouseEventKind::ScrollDown
            if overlay.tab() == SettingsTab::Thresholds
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.next_field();
        }
        MouseEventKind::ScrollUp
            if overlay.tab() == SettingsTab::Integrations
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.prev_integration();
        }
        MouseEventKind::ScrollDown
            if overlay.tab() == SettingsTab::Integrations
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.next_integration();
        }
        _ => {}
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
    fn key_handler_switches_tabs_and_keeps_read_only_tabs_read_only() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('2'));
        assert_eq!(overlay.tab(), SettingsTab::Integrations);
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('3'));
        assert_eq!(overlay.tab(), SettingsTab::Parameters);
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('e'));
        assert!(overlay.edit_buffer().is_none());

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Tab);
        assert_eq!(overlay.tab(), SettingsTab::Rules);
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::BackTab);
        assert_eq!(overlay.tab(), SettingsTab::Parameters);
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('1'));
        assert_eq!(overlay.tab(), SettingsTab::Thresholds);

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('e'));
        assert!(overlay.edit_buffer().is_some());
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('2'));
        assert_eq!(
            overlay.tab(),
            SettingsTab::Thresholds,
            "numeric input during edit must not switch tabs"
        );
        assert!(overlay.edit_buffer().unwrap_or("").ends_with('2'));
    }

    #[test]
    fn key_handler_toggles_integrations_with_keyboard() {
        use crate::ui::settings::IntegrationField;
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('2'));
        assert_eq!(overlay.tab(), SettingsTab::Integrations);
        assert_eq!(
            overlay.selected_integration(),
            IntegrationField::ClaudeSidefile
        );
        let sidefile_before = config.provider_setup.claude_sidefile;
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('e'));
        assert_ne!(config.provider_setup.claude_sidefile, sidefile_before);

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Down);
        assert_eq!(
            overlay.selected_integration(),
            IntegrationField::CodexAppServer
        );
        let server_before = config.provider_setup.codex_app_server;
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char(' '));
        assert_ne!(config.provider_setup.codex_app_server, server_before);
    }

    #[test]
    fn mouse_handler_closes_on_close_button() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let close = settings_close_button_rect(rects.body);

        assert!(handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), close.x, close.y),
        ));

        assert!(!overlay.is_open());
    }

    #[test]
    fn mouse_handler_switches_tabs_on_tab_labels() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let row = rects.tabs.y + 1;
        let inner_x = rects.tabs.x + 1;

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 17, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Integrations);

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 29, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Parameters);

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 41, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Rules);

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 2, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Thresholds);
    }

    #[test]
    fn mouse_handler_toggles_integration_rows() {
        use crate::ui::settings::IntegrationField;
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        overlay.switch_tab(SettingsTab::Integrations);
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let body_inner = rects.body.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let sidefile_before = config.provider_setup.claude_sidefile;
        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                body_inner.x + 5,
                body_inner.y + 1,
            ),
        );
        assert_eq!(
            overlay.selected_integration(),
            IntegrationField::ClaudeSidefile
        );
        assert_ne!(config.provider_setup.claude_sidefile, sidefile_before);

        let server_before = config.provider_setup.codex_app_server;
        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                body_inner.x + 5,
                body_inner.y + 2,
            ),
        );
        assert_eq!(
            overlay.selected_integration(),
            IntegrationField::CodexAppServer
        );
        assert_ne!(config.provider_setup.codex_app_server, server_before);
    }

    #[test]
    fn mouse_handler_selects_fields_and_scrolls_selection() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let body_inner = rects.body.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        assert!(handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                body_inner.x + 34,
                body_inner.y + 1
            ),
        ));
        assert_eq!(
            overlay.selected(),
            crate::ui::settings::FieldId::new(
                crate::ui::settings::Section::Cost,
                crate::ui::settings::Scope::Default,
                crate::ui::settings::Bound::Critical,
            )
        );

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(
                MouseEventKind::ScrollDown,
                body_inner.x + 10,
                body_inner.y + 1,
            ),
        );
        assert_eq!(
            overlay.selected(),
            crate::ui::settings::FieldId::new(
                crate::ui::settings::Section::Cost,
                crate::ui::settings::Scope::Claude,
                crate::ui::settings::Bound::Warning,
            )
        );
    }

    #[test]
    fn mouse_body_field_selection_is_disabled_on_read_only_tabs() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        overlay.switch_tab(SettingsTab::Rules);
        let viewport = Rect::new(0, 0, 120, 40);
        let rects = settings_modal_rects(viewport);
        let body_inner = rects.body.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        let before = overlay.selected();

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                body_inner.x + 34,
                body_inner.y + 1,
            ),
        );
        assert_eq!(overlay.selected(), before);

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(
                MouseEventKind::ScrollDown,
                body_inner.x + 10,
                body_inner.y + 1,
            ),
        );
        assert_eq!(overlay.selected(), before);
    }
}
