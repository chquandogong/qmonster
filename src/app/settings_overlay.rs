use std::path::Path;

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::config::QmonsterConfig;
use crate::app::keymap::rect_contains;
use crate::ui::settings::{
    SettingsOverlay, SettingsTab, settings_close_button_rect, settings_field_at_with_scroll,
    settings_integration_field_at_with_scroll, settings_max_scroll, settings_modal_rects,
    settings_parameter_field_at_with_scroll, settings_parameter_field_line_index,
    settings_parameter_list_rect_for_viewport, settings_parameter_visible_rows_for_viewport,
    settings_tab_index_at, settings_visible_body_rows,
};

const NO_CONFIG_PATH_SAVE_ERROR: &str =
    "no config path \u{2014} restart with `--config PATH` to enable save";
const TAB_BY_INDEX: [SettingsTab; 5] = [
    SettingsTab::Thresholds,
    SettingsTab::Integrations,
    SettingsTab::Parameters,
    SettingsTab::Rules,
    SettingsTab::Badges,
];

/// Returns `true` when the operator pressed the entry key on a
/// non-editing open overlay; the caller should close() and skip
/// the per-overlay dispatcher. Preserves the existing rule that
/// while a numeric edit is in flight, the keystroke is consumed
/// as input rather than closing the overlay.
pub fn settings_entry_key_closes(
    overlay: &crate::ui::settings::SettingsOverlay,
    code: crossterm::event::KeyCode,
) -> bool {
    overlay.is_open()
        && overlay.edit_buffer().is_none()
        && code == crossterm::event::KeyCode::Char('S')
}

pub fn handle_settings_overlay_key(
    overlay: &mut SettingsOverlay,
    config: &mut QmonsterConfig,
    config_path: Option<&Path>,
    code: KeyCode,
) -> bool {
    handle_settings_overlay_key_with_viewport(
        overlay,
        config,
        config_path,
        Rect::new(0, 0, 120, 40),
        code,
    )
}

pub fn handle_settings_overlay_key_with_viewport(
    overlay: &mut SettingsOverlay,
    config: &mut QmonsterConfig,
    config_path: Option<&Path>,
    viewport: Rect,
    code: KeyCode,
) -> bool {
    if !overlay.is_open() {
        return false;
    }

    let editing = overlay.edit_buffer().is_some();
    // v1.58.0: parameter filter mode is a separate input pipeline. While
    // it's active, Esc cancels the filter (not the overlay), Enter
    // confirms the filter and exits input, Backspace edits the filter
    // string, and printable chars append. All other keys fall through
    // to the normal handler so navigation still works under a frozen
    // filter.
    let filtering = !editing && overlay.parameter_filter().is_some();
    if filtering {
        match code {
            KeyCode::Esc => {
                overlay.cancel_parameter_filter();
                return true;
            }
            KeyCode::Enter => {
                overlay.confirm_parameter_filter();
                return true;
            }
            KeyCode::Backspace => {
                overlay.parameter_filter_backspace();
                return true;
            }
            KeyCode::Char(c) if c != '/' => {
                overlay.parameter_filter_type_char(c);
                return true;
            }
            // `/` while filtering = restart fresh filter (clear buffer).
            KeyCode::Char('/') => {
                overlay.cancel_parameter_filter();
                overlay.start_parameter_filter();
                return true;
            }
            _ => {}
        }
    }

    let max_scroll = settings_max_scroll(overlay, config, viewport);
    let page_rows = settings_visible_body_rows(viewport)
        .saturating_sub(1)
        .max(1);
    match code {
        KeyCode::Char('/') if !editing && overlay.tab() == SettingsTab::Parameters => {
            overlay.start_parameter_filter();
        }
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
        KeyCode::Char('5') if !editing => overlay.switch_tab(SettingsTab::Badges),
        KeyCode::Tab if !editing => overlay.next_tab(),
        KeyCode::BackTab if !editing => overlay.previous_tab(),
        KeyCode::Up if !editing && tab_uses_body_scroll(overlay.tab()) => overlay.scroll_up(),
        KeyCode::Down if !editing && tab_uses_body_scroll(overlay.tab()) => {
            overlay.scroll_down(max_scroll)
        }
        KeyCode::Char('k') if !editing && tab_uses_body_scroll(overlay.tab()) => {
            overlay.scroll_up()
        }
        KeyCode::Char('j') if !editing && tab_uses_body_scroll(overlay.tab()) => {
            overlay.scroll_down(max_scroll)
        }
        KeyCode::PageUp if !editing => overlay.page_up(page_rows),
        KeyCode::PageDown if !editing => overlay.page_down(page_rows, max_scroll),
        KeyCode::Home if !editing => overlay.scroll_top(),
        KeyCode::End if !editing => overlay.scroll_bottom(max_scroll),
        KeyCode::Up if !editing => move_selection_up(overlay, config, viewport),
        KeyCode::Down if !editing => move_selection_down(overlay, config, viewport),
        KeyCode::Left if !editing => move_selection_up(overlay, config, viewport),
        KeyCode::Right if !editing => move_selection_down(overlay, config, viewport),
        KeyCode::Char('e') if !editing => edit_or_toggle(overlay, config),
        KeyCode::Char('H') if !editing => overlay.toggle_hover_help_setting(config),
        KeyCode::Char('L') if !editing => overlay.toggle_help_language_setting(config),
        KeyCode::Char(' ') if !editing => {
            if overlay.tab() == SettingsTab::Integrations {
                overlay.toggle_integration(config);
            } else if overlay.tab() == SettingsTab::Parameters {
                let _ = overlay.activate_parameter(config);
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
            } else if overlay.tab() == SettingsTab::Parameters {
                let _ = overlay.activate_parameter(config);
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

fn tab_uses_body_scroll(tab: SettingsTab) -> bool {
    matches!(tab, SettingsTab::Rules | SettingsTab::Badges)
}

fn move_selection_up(overlay: &mut SettingsOverlay, config: &QmonsterConfig, viewport: Rect) {
    match overlay.tab() {
        SettingsTab::Thresholds => overlay.prev_field(),
        SettingsTab::Integrations => overlay.prev_integration(),
        SettingsTab::Parameters => {
            overlay.prev_parameter();
            keep_selected_parameter_visible(overlay, config, viewport);
        }
        SettingsTab::Rules | SettingsTab::Badges => {}
    }
}

fn move_selection_down(overlay: &mut SettingsOverlay, config: &QmonsterConfig, viewport: Rect) {
    match overlay.tab() {
        SettingsTab::Thresholds => overlay.next_field(),
        SettingsTab::Integrations => overlay.next_integration(),
        SettingsTab::Parameters => {
            overlay.next_parameter();
            keep_selected_parameter_visible(overlay, config, viewport);
        }
        SettingsTab::Rules | SettingsTab::Badges => {}
    }
}

fn edit_or_toggle(overlay: &mut SettingsOverlay, config: &mut QmonsterConfig) {
    match overlay.tab() {
        SettingsTab::Thresholds => overlay.start_edit(config),
        SettingsTab::Integrations => overlay.toggle_integration(config),
        SettingsTab::Parameters => {
            let _ = overlay.activate_parameter(config);
        }
        SettingsTab::Rules | SettingsTab::Badges => {}
    }
}

fn keep_selected_parameter_visible(
    overlay: &mut SettingsOverlay,
    config: &QmonsterConfig,
    viewport: Rect,
) {
    let Some(row) =
        settings_parameter_field_line_index(overlay, config, overlay.selected_parameter())
    else {
        return;
    };
    let visible = settings_visible_body_rows(viewport).max(1);
    let visible = if overlay.tab() == SettingsTab::Parameters {
        settings_parameter_visible_rows_for_viewport(viewport).max(1)
    } else {
        visible
    };
    let top = overlay.scroll_offset();
    let bottom = top.saturating_add(visible.saturating_sub(1));
    if row < top {
        overlay.set_scroll_offset(row);
    } else if row > bottom {
        overlay.set_scroll_offset(row.saturating_sub(visible.saturating_sub(1)));
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
                && let Some(field) = settings_field_at_with_scroll(
                    rects.body,
                    event.column,
                    event.row,
                    overlay.scroll_offset(),
                )
            {
                overlay.select_field(field);
            } else if overlay.tab() == SettingsTab::Integrations
                && let Some(field) = settings_integration_field_at_with_scroll(
                    rects.body,
                    event.column,
                    event.row,
                    overlay.scroll_offset(),
                )
            {
                overlay.select_integration(field);
                overlay.toggle_integration(config);
            } else if overlay.tab() == SettingsTab::Parameters
                && let Some(field) = settings_parameter_field_at_with_scroll(
                    overlay,
                    config,
                    settings_parameter_list_rect_for_viewport(viewport),
                    event.column,
                    event.row,
                    overlay.scroll_offset(),
                )
            {
                overlay.select_parameter(field);
                keep_selected_parameter_visible(overlay, config, viewport);
            }
        }
        MouseEventKind::ScrollUp
            if tab_uses_body_scroll(overlay.tab())
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.scroll_up();
        }
        MouseEventKind::ScrollDown
            if tab_uses_body_scroll(overlay.tab())
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.scroll_down(settings_max_scroll(overlay, config, viewport));
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
        MouseEventKind::ScrollUp
            if overlay.tab() == SettingsTab::Parameters
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.prev_parameter();
            keep_selected_parameter_visible(overlay, config, viewport);
        }
        MouseEventKind::ScrollDown
            if overlay.tab() == SettingsTab::Parameters
                && rect_contains(rects.body, event.column, event.row) =>
        {
            overlay.next_parameter();
            keep_selected_parameter_visible(overlay, config, viewport);
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
    fn settings_entry_key_closes_when_open_and_not_editing() {
        use crossterm::event::KeyCode;
        let mut overlay = crate::ui::settings::SettingsOverlay::new();
        assert!(!settings_entry_key_closes(&overlay, KeyCode::Char('S')));
        overlay.open();
        assert!(settings_entry_key_closes(&overlay, KeyCode::Char('S')));
        assert!(!settings_entry_key_closes(&overlay, KeyCode::Char('q')));
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
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('5'));
        assert_eq!(overlay.tab(), SettingsTab::Badges);
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('e'));
        assert!(overlay.edit_buffer().is_none());
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::BackTab);
        assert_eq!(overlay.tab(), SettingsTab::Rules);
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

        // Claude sidefile is the only integration field now; Down keeps
        // the selection on it (single-entry self-cycle).
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Down);
        assert_eq!(
            overlay.selected_integration(),
            IntegrationField::ClaudeSidefile
        );
    }

    #[test]
    fn h_and_l_toggle_hover_help_settings_inside_settings_overlay() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        overlay.switch_tab(SettingsTab::Parameters);

        assert!(config.ux.hover_help);
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('H'));
        assert!(!config.ux.hover_help);
        assert!(overlay.is_dirty());

        assert_eq!(
            config.ux.help_language,
            crate::app::config::HelpLanguage::Ko
        );
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('L'));
        assert_eq!(
            config.ux.help_language,
            crate::app::config::HelpLanguage::En
        );
    }

    #[test]
    fn key_handler_commits_numeric_parameter_edit() {
        use crate::ui::settings::ParameterField;

        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        overlay.switch_tab(SettingsTab::Parameters);
        overlay.select_parameter(ParameterField::InsightsIgnoredTtlSecs);

        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char('e'));
        assert!(overlay.edit_buffer().is_some());
        overlay.replace_edit_buffer_for_test("120");
        handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Enter);

        assert_eq!(config.insights.ignored_ttl_secs, 120);
        assert!(overlay.edit_buffer().is_none());
    }

    #[test]
    fn key_handler_routes_global_like_chars_to_text_edit_buffer() {
        use crate::ui::settings::ParameterField;

        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        overlay.switch_tab(SettingsTab::Parameters);
        overlay.select_parameter(ParameterField::FxText);
        overlay.start_edit(&config);
        overlay.replace_edit_buffer_for_test("");

        for key in ['Q', 'H', 'L', 'S', 'q'] {
            handle_settings_overlay_key(&mut overlay, &mut config, None, KeyCode::Char(key));
        }

        assert_eq!(overlay.edit_buffer(), Some("QHLSq"));
        assert!(
            overlay.is_open(),
            "text input must not close the settings overlay"
        );
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

        // v1.51.0: column offsets follow the numbered-label layout
        // documented in `settings_tab_index_at_uses_rendered_label_boundaries`.
        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 20, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Integrations);

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 35, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Parameters);

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 50, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Rules);

        handle_settings_overlay_mouse(
            &mut overlay,
            &mut config,
            viewport,
            mouse(MouseEventKind::Down(MouseButton::Left), inner_x + 60, row),
        );
        assert_eq!(overlay.tab(), SettingsTab::Badges);

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

        // Row 2 is no longer an integration field (Claude sidefile is the
        // only one); clicking it must not toggle anything.
        let sidefile_after_first = config.provider_setup.claude_sidefile;
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
            config.provider_setup.claude_sidefile, sidefile_after_first,
            "clicking the empty row 2 must not toggle the sidefile field"
        );
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

    #[test]
    fn read_only_tabs_scroll_body_with_keyboard() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        let viewport = Rect::new(0, 0, 80, 12);
        overlay.open();
        overlay.switch_tab(SettingsTab::Rules);

        assert_eq!(overlay.scroll_offset(), 0);
        handle_settings_overlay_key_with_viewport(
            &mut overlay,
            &mut config,
            None,
            viewport,
            KeyCode::Down,
        );
        assert_eq!(overlay.scroll_offset(), 1);
        handle_settings_overlay_key_with_viewport(
            &mut overlay,
            &mut config,
            None,
            viewport,
            KeyCode::Char('j'),
        );
        assert_eq!(overlay.scroll_offset(), 2);
        handle_settings_overlay_key_with_viewport(
            &mut overlay,
            &mut config,
            None,
            viewport,
            KeyCode::PageDown,
        );
        assert!(overlay.scroll_offset() > 2);
        handle_settings_overlay_key_with_viewport(
            &mut overlay,
            &mut config,
            None,
            viewport,
            KeyCode::Home,
        );
        assert_eq!(overlay.scroll_offset(), 0);
        handle_settings_overlay_key_with_viewport(
            &mut overlay,
            &mut config,
            None,
            viewport,
            KeyCode::End,
        );
        assert!(overlay.scroll_offset() > 0);
        handle_settings_overlay_key_with_viewport(
            &mut overlay,
            &mut config,
            None,
            viewport,
            KeyCode::PageUp,
        );
        assert!(
            overlay.scroll_offset()
                < crate::ui::settings::settings_max_scroll(&overlay, &config, viewport)
        );
    }

    #[test]
    fn mouse_wheel_scrolls_read_only_body() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        overlay.switch_tab(SettingsTab::Rules);
        let viewport = Rect::new(0, 0, 80, 12);
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
                MouseEventKind::ScrollDown,
                body_inner.x + 1,
                body_inner.y + 1,
            ),
        ));

        assert_eq!(overlay.scroll_offset(), 1);
    }

    #[test]
    fn editable_tabs_keep_arrow_and_wheel_selection_without_body_scroll() {
        let mut overlay = SettingsOverlay::new();
        let mut config = QmonsterConfig::defaults();
        overlay.open();
        let viewport = Rect::new(0, 0, 80, 12);
        let rects = settings_modal_rects(viewport);
        let body_inner = rects.body.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        handle_settings_overlay_key_with_viewport(
            &mut overlay,
            &mut config,
            None,
            viewport,
            KeyCode::Down,
        );
        assert_eq!(
            overlay.selected(),
            crate::ui::settings::FieldId::new(
                crate::ui::settings::Section::Cost,
                crate::ui::settings::Scope::Default,
                crate::ui::settings::Bound::Critical,
            )
        );
        assert_eq!(overlay.scroll_offset(), 0);

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
        assert_eq!(overlay.scroll_offset(), 0);
    }
}
