use super::*;

fn cfg() -> QmonsterConfig {
    QmonsterConfig::defaults()
}

fn rendered_text(lines: &[Line<'_>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// -----------------------------------------------------------------
// Cluster A: open / close / navigation.
// -----------------------------------------------------------------

#[test]
fn default_overlay_is_closed() {
    let s = SettingsOverlay::new();
    assert!(!s.is_open());
}

#[test]
fn settings_field_at_maps_body_rows_to_warning_and_critical_cells() {
    let viewport = Rect::new(0, 0, 120, 40);
    let rects = settings_modal_rects(viewport);
    let inner = rects.body.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });

    let warning = settings_field_at(rects.body, inner.x + 20, inner.y + 1)
        .expect("default cost warning cell should hit a field");
    assert_eq!(
        warning,
        FieldId::new(Section::Cost, Scope::Default, Bound::Warning)
    );

    let critical = settings_field_at(rects.body, inner.x + 34, inner.y + 1)
        .expect("default cost critical cell should hit a field");
    assert_eq!(
        critical,
        FieldId::new(Section::Cost, Scope::Default, Bound::Critical)
    );

    assert!(
        settings_field_at(rects.body, inner.x + 20, inner.y).is_none(),
        "section header rows are not editable fields"
    );
}

#[test]
fn open_sets_open_and_focuses_first_field() {
    let mut s = SettingsOverlay::new();
    s.open();
    assert!(s.is_open());
    assert_eq!(s.tab(), SettingsTab::Thresholds);
    assert_eq!(
        s.selected(),
        FieldId::new(Section::Cost, Scope::Default, Bound::Warning)
    );
}

#[test]
fn close_unsets_open_and_clears_edit_buffer() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.start_edit(&cfg());
    s.close();
    assert!(!s.is_open());
    assert_eq!(s.edit_buffer(), None);
}

#[test]
fn next_field_advances_through_all_fields_and_wraps() {
    let mut s = SettingsOverlay::new();
    s.open();
    let start = s.selected();
    for _ in 0..all_fields().len() {
        s.next_field();
    }
    assert_eq!(
        s.selected(),
        start,
        "one full all_fields cycle must wrap to start"
    );
}

#[test]
fn prev_field_reverses() {
    let mut s = SettingsOverlay::new();
    s.open();
    let first = s.selected();
    s.next_field();
    s.prev_field();
    assert_eq!(s.selected(), first);
}

#[test]
fn next_field_is_noop_during_edit() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.start_edit(&cfg());
    let before = s.selected();
    s.next_field();
    assert_eq!(s.selected(), before, "must not move focus while editing");
}

#[test]
fn tab_navigation_cycles_and_is_noop_during_edit() {
    let mut s = SettingsOverlay::new();
    s.open();

    s.next_tab();
    assert_eq!(s.tab(), SettingsTab::Integrations);
    s.next_tab();
    assert_eq!(s.tab(), SettingsTab::Parameters);
    s.next_tab();
    assert_eq!(s.tab(), SettingsTab::Thresholds);
    s.previous_tab();
    assert_eq!(s.tab(), SettingsTab::Parameters);

    s.switch_tab(SettingsTab::Thresholds);
    s.start_edit(&cfg());
    s.next_tab();
    assert_eq!(
        s.tab(),
        SettingsTab::Thresholds,
        "tab must not move while an edit buffer is active"
    );
}

#[test]
fn settings_tab_index_at_uses_rendered_label_boundaries() {
    // v1.51.0: labels now carry a numeric prefix ("1 Thresholds",
    // "2 Integrations", …) so the keyboard shortcut is visible
    // inside the tab strip itself. Hit-test column offsets shift
    // accordingly: each label gains 2 cells (digit + space).
    let tabs = Rect::new(10, 2, 80, 3);
    let inner_x = tabs.x + 1;

    // Layout (relative to inner_x):
    //   [0,13]  "1 Thresholds"   (12 chars + 2 padding)
    //   14      `│` divider
    //   [15,30] "2 Integrations" (14 chars + 2 padding)
    //   31      `│` divider
    //   [32,45] "3 Parameters"   (12 chars + 2 padding)
    assert_eq!(settings_tab_index_at(tabs, inner_x + 2), Some(0));
    assert_eq!(settings_tab_index_at(tabs, inner_x + 20), Some(1));
    assert_eq!(settings_tab_index_at(tabs, inner_x + 35), Some(2));
    // Past the final label there is no fourth/fifth tab to hit.
    assert_eq!(settings_tab_index_at(tabs, inner_x + 50), None);
    assert_eq!(settings_tab_index_at(tabs, inner_x + 80), None);
    assert_eq!(settings_tab_index_at(tabs, tabs.x - 1), None);
}

#[test]
fn settings_tab_strip_renders_single_group_dividers() {
    // narrow vNext: the three surviving tabs are all editable, so the
    // strip uses a `│` divider between every pair and never the old
    // `║` editable/reference seam.
    let line = settings_tab_strip_line(SettingsTab::Thresholds);
    let rendered: String = line
        .spans
        .iter()
        .map(|s| s.content.clone().into_owned())
        .collect();
    assert!(rendered.contains("1 Thresholds "), "rendered: {rendered}");
    assert!(rendered.contains("3 Parameters "), "rendered: {rendered}");
    assert!(
        rendered.contains("│"),
        "intra-group divider missing: {rendered}"
    );
    assert!(
        !rendered.contains("║"),
        "reference-group seam must be gone: {rendered}"
    );
    assert!(
        !rendered.contains("Rules") && !rendered.contains("Badges"),
        "reference tabs must not appear in the strip: {rendered}"
    );
}

#[test]
fn integration_tab_toggles_provider_setup_values() {
    let mut s = SettingsOverlay::new();
    let mut config = cfg();
    s.open();
    s.switch_tab(SettingsTab::Integrations);

    assert_eq!(s.selected_integration(), IntegrationField::ClaudeSidefile);
    let before_sidefile = config.provider_setup.claude_sidefile;
    s.toggle_integration(&mut config);
    assert_ne!(config.provider_setup.claude_sidefile, before_sidefile);
    assert!(s.is_dirty());
    assert!(matches!(s.status(), SettingsStatus::Dirty));

    // Claude sidefile is the only integration field now; next/previous
    // self-cycle back to it.
    s.next_integration();
    assert_eq!(s.selected_integration(), IntegrationField::ClaudeSidefile);
}

#[test]
fn settings_integration_field_at_maps_body_rows() {
    let viewport = Rect::new(0, 0, 120, 40);
    let rects = settings_modal_rects(viewport);
    let inner = rects.body.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });

    assert_eq!(
        settings_integration_field_at(rects.body, inner.x + 5, inner.y + 1),
        Some(IntegrationField::ClaudeSidefile)
    );
    assert_eq!(
        settings_integration_field_at(rects.body, inner.x + 5, inner.y + 2),
        None
    );
    assert_eq!(
        settings_integration_field_at(rects.body, inner.x + 5, inner.y),
        None
    );
}

#[test]
fn integrations_tab_shows_provider_setup_values_and_guidance() {
    let mut config = cfg();
    config.provider_setup.claude_sidefile = false;
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Integrations);

    let rendered = rendered_text(&build_body_lines(&s, &config));

    assert!(rendered.contains("[provider_setup]"));
    assert!(rendered.contains("Claude sidefile export"));
    assert!(rendered.contains("OFF"));
    assert!(rendered.contains("Provider Setup (P) is read-only"));
    assert!(rendered.contains("press w to write TOML"));
}

#[test]
fn parameters_tab_shows_custom_policy_inputs() {
    let mut config = cfg();
    config.token.quota_tight = true;
    config.cache.drift_min_samples = 6;
    config.reset.wait_eta_secs = 15 * 60;
    config.reset.snapshot_pressure_threshold = 0.65;
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);

    let rendered = rendered_text(&build_body_lines(&s, &config));

    assert!(rendered.contains("token quota_tight"));
    assert!(rendered.contains("on"));
    assert!(rendered.contains("default off"));
    assert!(rendered.contains("cache drift drop/samples"));
    assert!(rendered.contains("30%/6"));
    assert!(rendered.contains("reset wait pressure/eta"));
    assert!(rendered.contains("85%/15m"));
    assert!(rendered.contains("reset snapshot pressure/eta"));
    assert!(rendered.contains("65%/5m"));
}

#[test]
fn parameters_tab_shows_hover_help_settings() {
    let mut config = cfg();
    config.ux.hover_help = false;
    config.ux.help_language = HelpLanguage::En;
    config.ux.hover_help_trigger = crate::app::config::HoverHelpTrigger::Row;
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);

    let rendered = rendered_text(&build_body_lines(&s, &config));

    assert!(rendered.contains("ux hover/language/trigger"));
    assert!(rendered.contains("off/en/row"));
    assert!(rendered.contains("default on/ko/label"));
}

#[test]
fn parameters_tab_explains_selected_hover_help_parameter() {
    let config = cfg();
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    s.select_parameter(ParameterField::UxHoverHelp);

    let rendered = rendered_text(&build_body_lines(&s, &config));

    assert!(rendered.contains("Selected parameter help"));
    assert!(rendered.contains("TOML: ux.hover_help"));
    assert!(rendered.contains("Shortcut: H"));
    assert!(rendered.contains("floating help"));
}

#[test]
fn parameters_hint_mentions_hover_help_keys() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);

    let rendered = rendered_text(&hint_lines(&s));

    assert!(rendered.contains("H hover"));
    assert!(rendered.contains("L language"));
}

#[test]
fn parameters_tab_uses_side_help_panel_on_wide_modal() {
    let config = cfg();
    let viewport = Rect::new(0, 0, 160, 44);
    let body = settings_modal_rects(viewport).body;
    let rects =
        settings_parameter_side_panel_rects(body).expect("wide Parameters body should split");
    assert!(rects.help.x > rects.list.x.saturating_add(rects.list.width));

    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    s.select_parameter(ParameterField::UxHoverHelp);

    let list_text = rendered_text(&build_parameter_list_lines(&s, &config));
    let help_text = rendered_text(&build_parameter_help_panel_lines(&s, &config));

    assert!(
        !list_text.contains("Selected parameter help"),
        "side-panel list must not inline the selected parameter help"
    );
    assert!(help_text.contains("Selected parameter help"));
    assert!(help_text.contains("TOML: ux.hover_help"));
}

#[test]
fn settings_hint_reports_scroll_progress_and_end() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);

    let mid = rendered_text(&hint_lines_with_scroll(&s, Some((1, 3))));
    assert!(mid.contains("scroll 1/3"));
    assert!(mid.contains("more"));

    let end = rendered_text(&hint_lines_with_scroll(&s, Some((3, 3))));
    assert!(end.contains("scroll 3/3"));
    assert!(end.contains("END"));
}

#[test]
fn settings_edit_hint_distinguishes_text_parameters() {
    let config = cfg();
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    s.select_parameter(ParameterField::StorageRoot);
    s.start_edit(&config);

    let text = rendered_text(&hint_lines(&s));

    assert!(text.contains("type text"), "text edit hint missing: {text}");
    assert!(
        !text.contains("type digits"),
        "text edit hint must not advertise numeric-only input: {text}"
    );
}

#[test]
fn parameters_scroll_end_uses_last_selectable_row_not_status_footer() {
    let config = cfg();
    let viewport = Rect::new(0, 0, 160, 44);
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    let last = *all_parameter_fields()
        .last()
        .expect("Parameters must expose at least one editable field");
    s.select_parameter(last);

    let visible = settings_parameter_visible_rows_for_viewport(viewport).max(1);
    let last_row = settings_parameter_field_line_index(&s, &config, last)
        .expect("last editable parameter must have a rendered row");
    let selectable_end = last_row.saturating_sub(visible.saturating_sub(1));

    assert_eq!(
        settings_max_scroll(&s, &config, viewport),
        selectable_end,
        "Parameters scroll end should match the last selectable row, not trailing blank/status rows"
    );
}

#[test]
fn parameters_tab_exposes_editable_fields_for_all_config_sections() {
    let fields = all_parameter_fields();

    assert!(fields.contains(&ParameterField::TmuxSource));
    assert!(fields.contains(&ParameterField::ActionsMode));
    assert!(fields.contains(&ParameterField::RefreshPolicy));
    assert!(fields.contains(&ParameterField::LoggingSensitivity));
    assert!(fields.contains(&ParameterField::TokenQuotaTight));
    assert!(fields.contains(&ParameterField::StorageRoot));
    assert!(fields.contains(&ParameterField::IdleStillnessPolls));
    assert!(fields.contains(&ParameterField::SecurityPostureAdvisories));
    assert!(fields.contains(&ParameterField::CacheHotRatioThreshold));
    assert!(fields.contains(&ParameterField::ResetAutoSnapshot));
    assert!(fields.contains(&ParameterField::AnomalyEnabled));
    assert!(fields.contains(&ParameterField::CostBudgetUsd));
    assert!(fields.contains(&ParameterField::ProfileSwitchWindowPolls));
    assert!(fields.contains(&ParameterField::UxConfirmActions));
    assert!(fields.contains(&ParameterField::UxHoverHelp));
    assert!(fields.contains(&ParameterField::UxHoverHelpTrigger));
    assert!(fields.contains(&ParameterField::UxHelpLanguage));
}

#[test]
fn parameter_bool_enum_number_and_string_edits_update_config() {
    let mut s = SettingsOverlay::new();
    let mut config = cfg();
    s.open();
    s.switch_tab(SettingsTab::Parameters);

    // v1.51.0: anomaly.enabled default flipped to true; force a
    // known starting state so this test exercises the toggle
    // mechanic regardless of which way the default points.
    config.anomaly.enabled = false;
    s.select_parameter(ParameterField::AnomalyEnabled);
    s.activate_parameter(&mut config).expect("bool toggle");
    assert!(config.anomaly.enabled);

    s.select_parameter(ParameterField::AnomalyMinConfidence);
    s.activate_parameter(&mut config).expect("enum cycle");
    assert_eq!(config.anomaly.min_confidence, "high");

    s.select_parameter(ParameterField::ResetWaitEtaSecs);
    s.start_edit(&config);
    s.replace_edit_buffer_for_test("900");
    s.commit_edit(&mut config).expect("number edit");
    assert_eq!(config.reset.wait_eta_secs, 900);

    s.select_parameter(ParameterField::StorageRoot);
    s.start_edit(&config);
    s.replace_edit_buffer_for_test("/tmp/qmonster-root");
    s.commit_edit(&mut config).expect("string edit");
    assert_eq!(config.storage.root.as_deref(), Some("/tmp/qmonster-root"));
}

#[test]
fn save_writes_parameter_values_into_existing_toml() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    let original = "# header\n[anomaly]\nenabled = false\n\n[reset]\nwait_eta_secs = 1800\n";
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(original.as_bytes()))
        .expect("write seed");

    let mut s = SettingsOverlay::new();
    let mut config = cfg();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    // v1.51.0: anomaly.enabled default flipped to true; toggling
    // from a known false starts the test consistently regardless
    // of the default's polarity.
    config.anomaly.enabled = false;
    s.select_parameter(ParameterField::AnomalyEnabled);
    s.activate_parameter(&mut config).expect("toggle enabled");
    s.select_parameter(ParameterField::ResetWaitEtaSecs);
    s.start_edit(&config);
    s.replace_edit_buffer_for_test("900");
    s.commit_edit(&mut config).expect("commit eta");

    s.save(&config, &path).expect("save ok");

    let saved = std::fs::read_to_string(&path).expect("read back");
    assert!(saved.contains("# header"));
    assert!(saved.contains("enabled = true"));
    assert!(saved.contains("wait_eta_secs = 900"));
    let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
    assert!(reloaded.anomaly.enabled);
    assert_eq!(reloaded.reset.wait_eta_secs, 900);
}

#[test]
fn parameters_tab_shows_reset_auto_snapshot_row() {
    // default: auto_snapshot = false
    let config_default = cfg();
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    let rendered_default = rendered_text(&build_body_lines(&s, &config_default));
    assert!(
        rendered_default.contains("reset auto_snapshot"),
        "Parameters tab must have a reset auto_snapshot row"
    );
    assert!(
        rendered_default.contains("off"),
        "default auto_snapshot must render as 'off'"
    );

    // non-default: auto_snapshot = true
    let mut config_on = cfg();
    config_on.reset.auto_snapshot = true;
    let rendered_on = rendered_text(&build_body_lines(&s, &config_on));
    assert!(
        rendered_on.contains("reset auto_snapshot"),
        "Parameters tab must have a reset auto_snapshot row when enabled"
    );
    assert!(
        rendered_on.contains("on"),
        "enabled auto_snapshot must render as 'on'"
    );
    assert!(
        rendered_on.contains("default off"),
        "enabled auto_snapshot must show 'default off'"
    );
}

// -----------------------------------------------------------------
// Cluster B: read_field / effective_field.
// -----------------------------------------------------------------

#[test]
fn read_field_returns_default_warning_for_cost() {
    let s = SettingsOverlay::new();
    let v = s.read_field(
        &cfg(),
        FieldId::new(Section::Cost, Scope::Default, Bound::Warning),
    );
    assert_eq!(v, Some(5.0));
}

#[test]
fn read_field_returns_none_for_unset_per_provider_override() {
    let s = SettingsOverlay::new();
    let v = s.read_field(
        &cfg(),
        FieldId::new(Section::Context, Scope::Codex, Bound::Warning),
    );
    assert_eq!(
        v, None,
        "ContextConfig::default() leaves codex unset → read_field is None"
    );
}

#[test]
fn effective_field_falls_through_to_default_when_unset() {
    let s = SettingsOverlay::new();
    let v = s.effective_field(
        &cfg(),
        FieldId::new(Section::Context, Scope::Codex, Bound::Warning),
    );
    assert!(
        (v - 0.75).abs() < 1e-6,
        "unset codex inherits 0.75 from default"
    );
}

#[test]
fn quota_fields_split_claude_and_codex_5h_weekly_scopes() {
    let fields = all_fields();
    assert!(fields.contains(&FieldId::new(
        Section::Quota,
        Scope::Claude5h,
        Bound::Warning
    )));
    assert!(fields.contains(&FieldId::new(
        Section::Quota,
        Scope::ClaudeWeekly,
        Bound::Warning
    )));
    assert!(fields.contains(&FieldId::new(
        Section::Quota,
        Scope::Codex5h,
        Bound::Warning
    )));
    assert!(fields.contains(&FieldId::new(
        Section::Quota,
        Scope::CodexWeekly,
        Bound::Warning
    )));
    assert!(!fields.contains(&FieldId::new(
        Section::Cost,
        Scope::Claude5h,
        Bound::Warning
    )));
    assert_eq!(fields.len(), 28);
}

// -----------------------------------------------------------------
// Cluster C: edit buffer / type_char / backspace / cancel.
// -----------------------------------------------------------------

#[test]
fn start_edit_initializes_buffer_with_effective_value() {
    let mut s = SettingsOverlay::new();
    s.open();
    // Select first field (cost / default / warning) — value is 5.0.
    s.start_edit(&cfg());
    assert_eq!(s.edit_buffer(), Some("5"));
}

#[test]
fn type_char_appends_digits_and_dot_only() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.start_edit(&cfg());
    // Buffer starts at "5"; clear then type "12.34a.5" expecting
    // "12.345" (the second '.' and 'a' are dropped).
    s.cancel_edit();
    s.edit_buffer = Some(String::new());
    for c in "12.34a.5".chars() {
        s.type_char(c);
    }
    assert_eq!(s.edit_buffer(), Some("12.345"));
}

#[test]
fn backspace_pops_one_char() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.edit_buffer = Some("5.50".into());
    s.backspace();
    assert_eq!(s.edit_buffer(), Some("5.5"));
}

#[test]
fn cancel_edit_clears_buffer_without_changing_config() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.start_edit(&cfg());
    s.type_char('9');
    s.cancel_edit();
    assert_eq!(s.edit_buffer(), None);
    // Verify config is unchanged via read_field.
    assert_eq!(
        s.read_field(
            &cfg(),
            FieldId::new(Section::Cost, Scope::Default, Bound::Warning),
        ),
        Some(5.0)
    );
}

// -----------------------------------------------------------------
// Cluster D: commit_edit + validation.
// -----------------------------------------------------------------

#[test]
fn commit_valid_cost_default_warning_updates_config() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.start_edit(&cfg());
    s.cancel_edit();
    s.edit_buffer = Some("7.5".into());
    let mut config = cfg();
    s.commit_edit(&mut config).expect("valid edit must commit");
    assert!((config.cost.warning_usd - 7.5).abs() < f64::EPSILON);
    assert!(s.is_dirty());
}

#[test]
fn commit_unparseable_buffer_returns_error() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.edit_buffer = Some("abc".into());
    let mut config = cfg();
    let err = s.commit_edit(&mut config).unwrap_err();
    assert!(err.contains("not a number"), "got: {err}");
    assert!(matches!(s.status(), SettingsStatus::Error(_)));
    // Buffer must survive a validation failure so the operator can
    // see what they typed and either correct or cancel.
    assert!(s.edit_buffer().is_some());
}

#[test]
fn commit_pct_outside_unit_range_returns_error() {
    let mut s = SettingsOverlay::new();
    s.open();
    // Walk to context / default / warning (8 next_field steps to skip
    // the 8 cost fields).
    for _ in 0..8 {
        s.next_field();
    }
    assert_eq!(
        s.selected(),
        FieldId::new(Section::Context, Scope::Default, Bound::Warning)
    );
    s.edit_buffer = Some("1.5".into());
    let mut config = cfg();
    let err = s.commit_edit(&mut config).unwrap_err();
    assert!(err.contains("[0.0, 1.0]"), "got: {err}");
}

#[test]
fn commit_warning_above_critical_returns_error() {
    let mut s = SettingsOverlay::new();
    s.open();
    // First field is cost / default / warning. Default critical is 20.
    // Try to set warning to 25 → should reject.
    s.edit_buffer = Some("25".into());
    let mut config = cfg();
    let err = s.commit_edit(&mut config).unwrap_err();
    assert!(err.contains("warning"), "got: {err}");
    assert!(err.contains("critical"), "got: {err}");
}

#[test]
fn commit_negative_cost_returns_error() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.edit_buffer = Some("-1".into());
    let mut config = cfg();
    let err = s.commit_edit(&mut config).unwrap_err();
    assert!(err.contains("cost"), "got: {err}");
}

#[test]
fn commit_creates_provider_override_when_editing_unset_provider_field() {
    let mut s = SettingsOverlay::new();
    s.open();
    // Walk to cost / claude / warning (2 next_field steps).
    s.next_field();
    s.next_field();
    assert_eq!(
        s.selected(),
        FieldId::new(Section::Cost, Scope::Claude, Bound::Warning)
    );
    let mut config = cfg();
    // Default cost config sets claude warning_usd = 10.0; we change it.
    s.edit_buffer = Some("12.5".into());
    s.commit_edit(&mut config).expect("valid edit must commit");
    let claude = config
        .cost
        .claude
        .as_ref()
        .expect("claude override present");
    assert!((claude.warning_usd - 12.5).abs() < f64::EPSILON);
    // Critical stays at the default-loaded 30.0 for claude.
    assert!((claude.critical_usd - 30.0).abs() < f64::EPSILON);
}

#[test]
fn commit_quota_codex_weekly_creates_split_override_only() {
    let mut s = SettingsOverlay::new();
    s.open();
    while s.selected() != FieldId::new(Section::Quota, Scope::CodexWeekly, Bound::Warning) {
        s.next_field();
    }
    let mut config = cfg();
    s.edit_buffer = Some("0.66".into());
    s.commit_edit(&mut config).expect("valid edit must commit");

    assert!(config.quota.codex_5h.is_none());
    let weekly = config
        .quota
        .codex_weekly
        .as_ref()
        .expect("weekly override present");
    assert!((weekly.warning_pct - 0.66).abs() < f32::EPSILON);
    assert!((weekly.critical_pct - config.quota.critical_pct).abs() < f32::EPSILON);
}

// -----------------------------------------------------------------
// Cluster E: clear_override.
// -----------------------------------------------------------------

#[test]
fn clear_override_on_provider_field_restores_default() {
    let mut s = SettingsOverlay::new();
    s.open();
    // Cost / claude / warning has an override out of the box (10.0).
    s.next_field(); // critical
    s.next_field(); // claude warning
    let mut config = cfg();
    assert!(config.cost.claude.is_some(), "default has claude override");
    s.clear_override(&mut config);
    assert!(
        config.cost.claude.is_none(),
        "clear must drop the entire claude provider override"
    );
    assert!(s.is_dirty());
    let v = s.effective_field(
        &config,
        FieldId::new(Section::Cost, Scope::Claude, Bound::Warning),
    );
    assert!(
        (v - config.cost.warning_usd).abs() < f64::EPSILON,
        "claude warning must now read the section default"
    );
}

#[test]
fn clear_override_on_default_field_is_noop() {
    let mut s = SettingsOverlay::new();
    s.open(); // first field is default scope
    let mut config = cfg();
    s.clear_override(&mut config);
    assert!(!s.is_dirty(), "default-scope clear must not mark dirty");
}

// -----------------------------------------------------------------
// Cluster F: save.
// -----------------------------------------------------------------

#[test]
fn save_writes_toml_to_path_and_marks_clean() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    // Pre-create the file so write replaces it (not strictly needed,
    // but mirrors the real flow where the operator's file already exists).
    let _ = std::fs::File::create(&path).and_then(|mut f| f.write_all(b"# placeholder\n"));
    let mut s = SettingsOverlay::new();
    s.open();
    let mut config = cfg();
    // Make a small edit so dirty=true.
    s.edit_buffer = Some("6".into());
    s.commit_edit(&mut config).expect("commit ok");
    assert!(s.is_dirty());
    s.save(&config, &path).expect("save ok");
    assert!(!s.is_dirty(), "save clears dirty flag");
    assert!(matches!(s.status(), SettingsStatus::Saved(_)));
    // Round-trip parse to confirm the new value is on disk.
    let raw = std::fs::read_to_string(&path).expect("read back");
    let reloaded: QmonsterConfig = toml::from_str(&raw).expect("parse");
    assert!((reloaded.cost.warning_usd - 6.0).abs() < f64::EPSILON);
}

#[test]
fn save_writes_dirty_hover_help_parameters() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    let original = "[ux]\nconfirm_actions = \"always\"\n";
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(original.as_bytes()))
        .expect("write seed");

    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    let mut config = cfg();
    s.select_parameter(ParameterField::UxHoverHelp);
    s.activate_parameter(&mut config)
        .expect("toggle hover help");
    s.select_parameter(ParameterField::UxHelpLanguage);
    s.activate_parameter(&mut config).expect("toggle language");
    s.select_parameter(ParameterField::UxHoverHelpTrigger);
    s.activate_parameter(&mut config).expect("toggle trigger");

    s.save(&config, &path).expect("save ok");

    let raw = std::fs::read_to_string(&path).expect("read back");
    let reloaded: QmonsterConfig = toml::from_str(&raw).expect("parse");
    assert!(!reloaded.ux.hover_help);
    assert_eq!(reloaded.ux.help_language, HelpLanguage::En);
    assert_eq!(
        reloaded.ux.hover_help_trigger,
        crate::app::config::HoverHelpTrigger::Row
    );
    assert!(raw.contains("hover_help = false"), "raw: {raw}");
    assert!(raw.contains("help_language = \"en\""), "raw: {raw}");
    assert!(raw.contains("hover_help_trigger = \"row\""), "raw: {raw}");
}

#[test]
fn save_preserves_top_level_comments_in_existing_file() {
    // Phase E E2 (v1.20.0): the operator's hand-written
    // `qmonster.toml` may begin with explanatory comments that
    // describe why specific thresholds were chosen. The previous
    // `toml::to_string_pretty` save path stripped them; this test
    // locks the new contract — top-level comments must survive
    // a round-trip through the settings overlay.
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    let original = "# Qmonster operator config\n# Edited by ops 2026-04-28\n\n[cost]\nwarning_usd = 5.0\ncritical_usd = 20.0\n";
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(original.as_bytes()))
        .expect("write seed");

    let mut s = SettingsOverlay::new();
    s.open();
    let mut config = cfg();
    // Bump the cost warning so save() actually writes a value.
    s.edit_buffer = Some("7.5".into());
    s.commit_edit(&mut config).expect("commit ok");

    s.save(&config, &path).expect("save ok");

    let saved = std::fs::read_to_string(&path).expect("read back");
    assert!(
        saved.contains("# Qmonster operator config"),
        "leading comment must survive save: {saved}"
    );
    assert!(
        saved.contains("# Edited by ops 2026-04-28"),
        "second comment line must survive save: {saved}"
    );
}

#[test]
fn save_preserves_unrelated_section_comments_and_keys() {
    // Settings overlay edits its owned sections and preserves any
    // other section the operator has authored — `[tmux]`,
    // `[security]`, `[idle]`, custom comments, key order — after
    // save.
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    let original = "[cost]\nwarning_usd = 5.0\ncritical_usd = 20.0\n\n# tmux source preference; auto picks control-mode first\n[tmux]\nsource = \"control_mode\"\n\n[security]\n# raise this when a remote pair-programmer joins\nposture_advisories = true\n";
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(original.as_bytes()))
        .expect("write seed");

    let mut s = SettingsOverlay::new();
    s.open();
    let mut config = cfg();
    s.edit_buffer = Some("8".into());
    s.commit_edit(&mut config).expect("commit ok");
    s.save(&config, &path).expect("save ok");

    let saved = std::fs::read_to_string(&path).expect("read back");
    assert!(
        saved.contains("# tmux source preference; auto picks control-mode first"),
        "section comment must survive: {saved}"
    );
    assert!(
        saved.contains("source = \"control_mode\""),
        "[tmux] body unrelated to overlay must survive: {saved}"
    );
    assert!(
        saved.contains("# raise this when a remote pair-programmer joins"),
        "inline section comment must survive: {saved}"
    );
    assert!(
        saved.contains("posture_advisories = true"),
        "[security] body must survive: {saved}"
    );
}

#[test]
fn save_updates_target_threshold_value_in_existing_file() {
    // The whole point of preserving the file is to also still
    // write the operator's edits. Lock the round-trip: the new
    // value reaches disk under the existing key path.
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    let original = "# header\n[cost]\nwarning_usd = 5.0\ncritical_usd = 20.0\n";
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(original.as_bytes()))
        .expect("write seed");

    let mut s = SettingsOverlay::new();
    s.open();
    let mut config = cfg();
    s.edit_buffer = Some("9.25".into());
    s.commit_edit(&mut config).expect("commit ok");
    s.save(&config, &path).expect("save ok");

    let saved = std::fs::read_to_string(&path).expect("read back");
    assert!(
        saved.contains("warning_usd = 9.25") || saved.contains("warning_usd = 9.25"),
        "new value must be present: {saved}"
    );
    let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
    assert!((reloaded.cost.warning_usd - 9.25).abs() < f64::EPSILON);
}

#[test]
fn save_updates_provider_setup_values_in_existing_file() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    let original = "# header\n[provider_setup]\nclaude_sidefile = true\n";
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(original.as_bytes()))
        .expect("write seed");

    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Integrations);
    let mut config = cfg();
    config.provider_setup.claude_sidefile = false;

    s.save(&config, &path).expect("save ok");

    let saved = std::fs::read_to_string(&path).expect("read back");
    assert!(
        saved.contains("# header"),
        "leading comment must survive: {saved}"
    );
    assert!(
        saved.contains("claude_sidefile = false"),
        "sidefile setting must be written: {saved}"
    );
    let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
    assert!(!reloaded.provider_setup.claude_sidefile);
}

#[test]
fn save_falls_back_to_pretty_serialize_when_file_does_not_exist() {
    // Fresh write path — no existing file, no comments to
    // preserve. The legacy `toml::to_string_pretty` scaffold
    // continues to work so an operator with no prior config
    // gets a sensible starting file.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    assert!(!path.exists());

    let mut s = SettingsOverlay::new();
    s.open();
    let mut config = cfg();
    s.edit_buffer = Some("11".into());
    s.commit_edit(&mut config).expect("commit ok");
    s.save(&config, &path).expect("save ok");

    let saved = std::fs::read_to_string(&path).expect("read back");
    let reloaded: QmonsterConfig = toml::from_str(&saved).expect("parse");
    assert!((reloaded.cost.warning_usd - 11.0).abs() < f64::EPSILON);
}

#[test]
fn save_with_invalid_pair_returns_error_without_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("qmonster.toml");
    let mut s = SettingsOverlay::new();
    s.open();
    // Direct mutation of the config — bypass commit's per-field
    // validation to simulate a corrupted-on-disk start state, then
    // verify save() catches it on the second-pass full validation.
    let mut config = cfg();
    config.cost.warning_usd = 99.0; // > critical 20
    let err = s.save(&config, &path).unwrap_err();
    assert!(err.contains("warning"), "got: {err}");
    assert!(err.contains("critical"), "got: {err}");
    assert!(!path.exists(), "save must not write when validation fails");
}

// -----------------------------------------------------------------
// Cluster G: close-button hit-test (v1.15.19).
// -----------------------------------------------------------------

#[test]
fn close_button_rect_sits_at_top_right_of_body() {
    // Body of width 80 → close button starts at column 76 (80 - 4).
    let body = Rect::new(10, 5, 80, 20);
    let rect = settings_close_button_rect(body);
    assert_eq!(rect.x, 10 + 80 - 4);
    assert_eq!(rect.y, 5);
    assert_eq!(rect.width, 3);
    assert_eq!(rect.height, 1);
}

#[test]
fn close_button_rect_stays_inside_body_bounds() {
    let body = Rect::new(0, 0, 60, 30);
    let rect = settings_close_button_rect(body);
    assert!(rect.x + rect.width <= body.x + body.width);
    assert!(rect.y + rect.height <= body.y + body.height);
}

#[test]
fn close_button_rect_clamps_to_tiny_body() {
    // Degenerate body smaller than the [x] glyph — the rect must still
    // fit inside without underflowing.
    let body = Rect::new(0, 0, 2, 1);
    let rect = settings_close_button_rect(body);
    assert!(rect.x + rect.width <= body.x + body.width);
    assert!(rect.y + rect.height <= body.y + body.height);
}

// -----------------------------------------------------------------
// Cluster: Phase 7 anomaly settings rows
// -----------------------------------------------------------------

#[test]
fn parameters_tab_shows_anomaly_section() {
    let mut config = QmonsterConfig::defaults();
    config.anomaly.enabled = true;
    config.anomaly.window_polls = 25;
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    let lines = build_body_lines(&s, &config);
    let rendered = rendered_text(&lines);
    assert!(
        rendered.contains("anomaly enabled"),
        "missing enabled row: {rendered}"
    );
    assert!(
        rendered.contains("on"),
        "expected on for enabled=true: {rendered}"
    );
    assert!(
        rendered.contains("anomaly window_polls"),
        "missing window_polls row: {rendered}"
    );
    assert!(
        rendered.contains("25"),
        "expected 25 for window_polls: {rendered}"
    );
}

#[test]
fn parameters_tab_shows_v2_anomaly_thresholds() {
    let config = QmonsterConfig::defaults();
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    let lines = build_body_lines(&s, &config);
    let rendered = rendered_text(&lines);
    assert!(
        rendered.contains("anomaly cost_slope_usd_per_hour"),
        "missing cost_slope_usd_per_hour row: {rendered}"
    );
    assert!(
        rendered.contains("20.00"),
        "expected 20.00 for cost_slope_usd_per_hour: {rendered}"
    );
    assert!(
        rendered.contains("anomaly token_slope_input_per_poll"),
        "missing token_slope_input_per_poll row: {rendered}"
    );
    assert!(
        rendered.contains("20000"),
        "expected 20000 for token_slope_input_per_poll: {rendered}"
    );
    assert!(
        rendered.contains("anomaly memory_growth_mb"),
        "missing memory_growth_mb row: {rendered}"
    );
    assert!(
        rendered.contains("1024"),
        "expected 1024 for memory_growth_mb: {rendered}"
    );
}

#[test]
fn parameters_tab_includes_anomaly_promote_section() {
    let config = QmonsterConfig::defaults();
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    let lines = build_body_lines(&s, &config);
    let rendered = rendered_text(&lines);
    assert!(
        rendered.contains("Anomaly Promote (per-kind threshold)"),
        "section header missing: {rendered}"
    );
    assert!(
        rendered.contains("anomaly.promote subagent_side_effect"),
        "subagent_side_effect row missing"
    );
    assert!(
        rendered.contains("anomaly.promote cost_slope"),
        "cost_slope row missing"
    );
}

#[test]
fn parameters_tab_includes_anomaly_retention_days_row() {
    let config = QmonsterConfig::defaults();
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    let lines = build_body_lines(&s, &config);
    let rendered = rendered_text(&lines);
    assert!(
        rendered.contains("anomaly retention_days"),
        "row label missing: {rendered}"
    );
    assert!(rendered.contains("30"), "default value 30 missing");
}

/// v1.58.1 regression guard: when a parameter filter is active,
/// `next_parameter` / `prev_parameter` must skip fields whose
/// labels don't match. Pre-fix the navigation cycled through
/// `all_parameter_fields()` blindly, so arrow keys could land on
/// hidden fields and the visible cursor appeared to freeze.
#[test]
fn next_parameter_skips_fields_filtered_out_by_active_filter() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    s.select_parameter(ParameterField::TmuxSource);
    s.start_parameter_filter();
    for c in "anomaly".chars() {
        s.parameter_filter_type_char(c);
    }
    s.confirm_parameter_filter();

    // Selection started at TmuxSource (no "anomaly" in label) — first
    // ↓ should jump to the first visible (anomaly-prefixed) field.
    s.next_parameter();
    assert!(
        parameter_label(s.selected_parameter())
            .to_ascii_lowercase()
            .contains("anomaly"),
        "first next_parameter under `anomaly` filter should jump to a visible field; got {}",
        parameter_label(s.selected_parameter())
    );

    // Subsequent ↓ presses should stay within the filtered set.
    for _ in 0..50 {
        s.next_parameter();
        assert!(
            parameter_label(s.selected_parameter())
                .to_ascii_lowercase()
                .contains("anomaly"),
            "next_parameter must stay within the filtered set; got {}",
            parameter_label(s.selected_parameter())
        );
    }
}

#[test]
fn prev_parameter_skips_fields_filtered_out_by_active_filter() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    s.select_parameter(ParameterField::TmuxSource);
    s.start_parameter_filter();
    for c in "ux".chars() {
        s.parameter_filter_type_char(c);
    }
    s.confirm_parameter_filter();

    // Selection at TmuxSource is hidden under `ux` filter — ↑
    // should jump to the last visible field (last ux* row).
    s.prev_parameter();
    let label = parameter_label(s.selected_parameter()).to_ascii_lowercase();
    assert!(
        label.contains("ux"),
        "first prev_parameter under `ux` filter should jump to a visible field; got {label}"
    );

    for _ in 0..30 {
        s.prev_parameter();
        assert!(
            parameter_label(s.selected_parameter())
                .to_ascii_lowercase()
                .contains("ux"),
            "prev_parameter must stay within the filtered set; got {}",
            parameter_label(s.selected_parameter())
        );
    }
}

#[test]
fn next_parameter_no_op_when_filter_matches_nothing() {
    let mut s = SettingsOverlay::new();
    s.open();
    s.switch_tab(SettingsTab::Parameters);
    s.select_parameter(ParameterField::TmuxSource);
    let before = s.selected_parameter();
    s.start_parameter_filter();
    for c in "zzznonsense".chars() {
        s.parameter_filter_type_char(c);
    }
    s.confirm_parameter_filter();

    // Empty filtered set → next/prev are no-ops (selection stays).
    s.next_parameter();
    assert_eq!(s.selected_parameter(), before);
    s.prev_parameter();
    assert_eq!(s.selected_parameter(), before);
}
