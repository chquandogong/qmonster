use super::*;
use crate::domain::identity::{PaneIdentity, Provider, ResolvedIdentity, Role};
use crate::domain::origin::SourceKind;
use crate::domain::signal::{MetricValue, RuntimeFact, RuntimeFactKind, SignalSet};

fn base_report() -> PaneReport {
    PaneReport {
        pane_id: "%1".into(),
        session_name: "qwork".into(),
        window_index: "1".into(),
        provider: Provider::Claude,
        identity: ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        },
        signals: SignalSet::default(),
        recommendations: vec![],
        effects: vec![],
        dead: false,
        current_path: "/repo".into(),
        worktree_role: None,
        current_command: "claude".into(),
        cross_pane_findings: vec![],
        idle_state: None,
        idle_state_entered_at: None,
        recent_token_samples: Vec::new(), // F-3: test fixture; production fetches via event_loop
        anomalies: vec![],
    }
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn line_texts(lines: &[Line<'_>]) -> Vec<String> {
    lines.iter().map(line_text).collect()
}

fn index_containing(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?} in lines: {lines:#?}"))
}

fn future_unix_seconds(offset: std::time::Duration) -> u64 {
    std::time::SystemTime::now()
        .checked_add(offset)
        .expect("test timestamp must be representable")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("test timestamp must be after unix epoch")
        .as_secs()
}

#[test]
fn pane_panel_title_appends_cli_version_after_provider_role() {
    let mut r = base_report();
    r.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::CliVersion,
        "1.2.3",
        SourceKind::ProviderOfficial,
    ));

    assert_eq!(
        pane_panel_title(&r),
        "qwork:1 · Claude main · CLI 1.2.3 [Official] · %1"
    );
}

#[test]
fn pane_panel_title_omits_cli_version_for_qmonster_pane() {
    let mut r = base_report();
    r.provider = Provider::Qmonster;
    r.identity.identity.provider = Provider::Qmonster;
    r.identity.identity.role = Role::Monitor;
    r.current_command = "qmonster".into();
    r.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::CliVersion,
        "9.9.9",
        SourceKind::ProviderOfficial,
    ));

    assert_eq!(pane_panel_title(&r), "qwork:1 · Qmonster monitor · %1");
}

#[test]
fn panel_title_includes_identity_and_confidence() {
    let rep = base_report();
    assert_eq!(pane_panel_title(&rep), "qwork:1 · Claude main · %1");
}

#[test]
fn panel_title_marks_identity_conflict() {
    let mut rep = base_report();
    rep.identity.confidence = IdentityConfidence::Conflict;
    assert_eq!(
        pane_panel_title(&rep),
        "qwork:1 · IDENTITY CONFLICT · Claude main · %1"
    );
}

#[test]
fn panel_title_omits_unknown_role_word() {
    let mut rep = base_report();
    rep.identity.identity.role = Role::Unknown;
    assert_eq!(pane_panel_title(&rep), "qwork:1 · Claude · %1");
}

#[test]
fn pane_cards_render_current_command_row() {
    let mut rep = base_report();
    rep.current_command =
        "target/release/qmonster --config /home/me/.qmonster/config/qmonster.toml".into();

    let lines = pane_list_lines(&rep, true, false);
    let text: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    // cmd is the last row in IDENTITY (path then cmd), so it renders with
    // the `└ ` last-child glyph.
    assert!(
        text.iter()
            .any(|line| line.starts_with("└ cmd     : target/release/qmonster")),
        "pane list must expose the tmux current command: {text:?}"
    );

    let mut buf = Buffer::empty(Rect::new(0, 0, 90, 8));
    render_pane_panel(Rect::new(0, 0, 90, 8), &mut buf, &rep);
    let rendered: String = (0..8)
        .map(|y| (0..90).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("cmd     : target/release/qmonster"),
        "selected pane panel must expose the tmux current command: {rendered}"
    );
}

#[test]
fn pane_panel_uses_sectioned_order_and_keeps_top_six_recommendations() {
    use crate::domain::recommendation::{Recommendation, Severity};

    let mut rep = sample_pane_report();
    rep.signals.cost_usd = Some(MetricValue::new(3.0, SourceKind::ProviderOfficial));
    rep.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::Sandbox,
        "workspace-write",
        SourceKind::ProviderOfficial,
    ));
    rep.recommendations = (0..7)
        .map(|idx| Recommendation {
            action: "panel-test",
            reason: format!("panel recommendation {idx}"),
            severity: Severity::Concern,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        })
        .collect();

    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 24));
    render_pane_panel(Rect::new(0, 0, 120, 24), &mut buf, &rep);
    let rendered: String = (0..24)
        .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    let identity = rendered
        .find("IDENTITY")
        .unwrap_or_else(|| panic!("IDENTITY missing in panel: {rendered}"));
    let now = rendered
        .find("NOW")
        .unwrap_or_else(|| panic!("NOW missing in panel: {rendered}"));
    let pressure = rendered
        .find("PRESSURE")
        .unwrap_or_else(|| panic!("PRESSURE missing in panel: {rendered}"));
    let next = rendered
        .find("NEXT")
        .unwrap_or_else(|| panic!("NEXT missing in panel: {rendered}"));
    // RUNTIME merged into PRESSURE — its own header must be gone.
    assert!(
        !rendered.contains("RUNTIME"),
        "RUNTIME header should be merged into PRESSURE: {rendered}"
    );

    assert!(
        identity < now,
        "IDENTITY should precede NOW in panel: {rendered}"
    );
    assert!(
        now < pressure,
        "NOW should precede PRESSURE in panel: {rendered}"
    );
    assert!(
        pressure < next,
        "PRESSURE should precede NEXT in panel: {rendered}"
    );
    assert!(
        rendered.contains("panel recommendation 5"),
        "panel should keep the current top 6 recommendation limit: {rendered}"
    );
    assert!(
        !rendered.contains("panel recommendation 6"),
        "panel should not render a seventh recommendation: {rendered}"
    );
}

#[test]
fn pane_panel_keeps_empty_recommendations_state_under_section() {
    let rep = sample_pane_report();

    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 12));
    render_pane_panel(Rect::new(0, 0, 100, 12), &mut buf, &rep);
    let rendered: String = (0..12)
        .map(|y| (0..100).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    let next = rendered
        .find("NEXT")
        .unwrap_or_else(|| panic!("NEXT missing in panel: {rendered}"));
    let empty = rendered
        .find("status  : no active recommendations")
        .unwrap_or_else(|| panic!("empty recommendation state missing in panel: {rendered}"));

    assert!(
        next < empty,
        "empty recommendation state should live under NEXT in panel: {rendered}"
    );
}

#[test]
fn expanded_recommendation_detail_fields_align_to_label_column() {
    assert_eq!(
        expanded_detail_field("next    : press s to snapshot"),
        "next    : press s to snapshot"
    );
    assert_eq!(
        expanded_detail_field("profile: claude-default (3 levers) [Qmonster]"),
        "profile : claude-default (3 levers) [Qmonster]"
    );
    assert_eq!(
        expanded_detail_field("[Official] BASH_MAX_OUTPUT_LENGTH = 30000"),
        "lever   : [Official] BASH_MAX_OUTPUT_LENGTH = 30000"
    );
    assert_eq!(
        expanded_detail_field("- disables provider auto-memory"),
        "effect  : disables provider auto-memory"
    );
}

#[test]
fn signal_chips_reflect_booleans() {
    let s = SignalSet {
        idle_state: Some(IdleCause::InputWait),
        log_storm: true,
        ..SignalSet::default()
    };
    assert_eq!(signal_chips(&s), vec!["waiting for input", "log storm"]);
}

#[test]
fn metric_row_renders_badge_per_metric() {
    let s = SignalSet {
        context_pressure: Some(MetricValue::new(0.71, SourceKind::Estimated).with_confidence(0.5)),
        ..SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("context 71%"));
    assert!(row.contains("[Estimate]"));
}

#[test]
fn metric_row_renders_quota_pressure_when_populated() {
    // S3-3: Gemini quota_pressure surfaces in the --once metric
    // row alongside context_pressure. Same percent + source-kind
    // shape as context.
    let s = SignalSet {
        quota_pressure: Some(
            MetricValue::new(0.47, SourceKind::ProviderOfficial).with_confidence(0.95),
        ),
        ..SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("quota 47%"));
    assert!(row.contains("[Official]"));
}

#[test]
fn metric_row_includes_context_window_size_when_populated() {
    // Task D-3: context window size renders alongside context pressure
    // e.g. "context 21% [Official] of 1.00M"
    let s = SignalSet {
        context_pressure: Some(MetricValue::new(0.21, SourceKind::ProviderOfficial)),
        context_window_size: Some(MetricValue::new(
            1_000_000_u64,
            SourceKind::ProviderOfficial,
        )),
        ..SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("context 21%"));
    assert!(row.contains("of 1.00M"));
}

#[test]
fn metric_row_renders_context_without_window_size_when_absent() {
    // Task D-3: when context_window_size is None, render exactly as before
    // (no regression: just "context 21%", not "context 21% of ?")
    let s = SignalSet {
        context_pressure: Some(MetricValue::new(0.21, SourceKind::ProviderOfficial)),
        context_window_size: None,
        ..SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("context 21%"));
    assert!(
        !row.contains(" of "),
        "should not contain ' of ' when window_size is None"
    );
}

#[test]
fn cost_metric_severity_thresholds_match_advisory_rule_pair() {
    // v1.15.15: cost severity thresholds must stay in sync with
    // the v1.15.14 cost_pressure_warning ($5.00) and
    // cost_pressure_critical ($20.00) advisory thresholds, plus
    // an early Concern band at $2.00 so the COST badge starts
    // tinting before the advisory fires.
    use crate::domain::recommendation::Severity;
    assert_eq!(cost_metric_severity(0.0), Severity::Good);
    assert_eq!(cost_metric_severity(1.99), Severity::Good);
    assert_eq!(cost_metric_severity(2.0), Severity::Concern);
    assert_eq!(cost_metric_severity(4.99), Severity::Concern);
    assert_eq!(cost_metric_severity(5.0), Severity::Warning);
    assert_eq!(cost_metric_severity(19.99), Severity::Warning);
    assert_eq!(cost_metric_severity(20.0), Severity::Risk);
    assert_eq!(cost_metric_severity(100.0), Severity::Risk);
}

#[test]
fn state_summary_line_uses_full_words_instead_of_chips() {
    let mut rep = base_report();
    rep.signals = SignalSet {
        idle_state: Some(IdleCause::InputWait),
        verbose_answer: true,
        ..SignalSet::default()
    };
    let summary = state_summary_line(&rep);
    assert!(summary.contains("high confidence"));
    assert!(!summary.contains("waiting for input"));
    assert!(!summary.contains("verbose output"));
}

#[test]
fn metric_row_renders_model_name_line_when_populated() {
    let s = crate::domain::signal::SignalSet {
        model_name: Some(crate::domain::signal::MetricValue::new(
            "gpt-5.4".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        ..crate::domain::signal::SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("model gpt-5.4"), "row: {row}");
    assert!(row.contains("Official"), "row: {row}");
}

#[test]
fn metric_row_uses_count_suffix_for_tokens() {
    let s = crate::domain::signal::SignalSet {
        token_count: Some(crate::domain::signal::MetricValue::new(
            1_530_000,
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        ..crate::domain::signal::SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("tokens 1.53M"), "got: {row}");
}

#[test]
fn selected_pane_panel_renders_token_breakdown_when_available() {
    let mut rep = base_report();
    rep.signals.input_tokens = Some(crate::domain::signal::MetricValue::new(
        1_510_000_u64,
        crate::domain::origin::SourceKind::ProviderOfficial,
    ));
    rep.signals.output_tokens = Some(crate::domain::signal::MetricValue::new(
        20_400_u64,
        crate::domain::origin::SourceKind::ProviderOfficial,
    ));

    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 10));
    render_pane_panel(Rect::new(0, 0, 100, 10), &mut buf, &rep);
    let rendered: String = (0..10)
        .map(|y| (0..100).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("tokens : Main 1.51M in / 20.4K out [Official]")
            || rendered.contains("tokens  : Main 1.51M in / 20.4K out [Official]"),
        "selected pane panel must show input/output token split: {rendered}"
    );
}

#[test]
fn metric_row_renders_git_branch_and_worktree_and_effort() {
    let s = crate::domain::signal::SignalSet {
        git_branch: Some(crate::domain::signal::MetricValue::new(
            "main".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        worktree_path: Some(crate::domain::signal::MetricValue::new(
            "~/Qmonster".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        reasoning_effort: Some(crate::domain::signal::MetricValue::new(
            "xhigh".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        ..crate::domain::signal::SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("branch main"), "row: {row}");
    assert!(row.contains("path ~/Qmonster"), "row: {row}");
    assert!(row.contains("effort xhigh"), "row: {row}");
}

#[test]
fn runtime_row_groups_modes_access_loaded_and_restricted_facts() {
    let s = crate::domain::signal::SignalSet {
        runtime_facts: vec![
            RuntimeFact::new(
                RuntimeFactKind::PermissionMode,
                "Full Access",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::Sandbox,
                "danger-full-access",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::AllowedDirectory,
                "~/Qmonster",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::LoadedSkill,
                "superpowers:executing-plans",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::RestrictedTool,
                "Bash(rm *)",
                SourceKind::ProviderOfficial,
            ),
        ],
        ..crate::domain::signal::SignalSet::default()
    };
    let row = runtime_row(&s);
    assert!(row.contains("modes:"), "row: {row}");
    assert!(row.contains("PERM Full Access [Official]"), "row: {row}");
    assert!(
        row.contains("SANDBOX danger-full-access [Official]"),
        "row: {row}"
    );
    assert!(row.contains("access:"), "row: {row}");
    assert!(row.contains("DIR ~/Qmonster [Official]"), "row: {row}");
    assert!(row.contains("loaded:"), "row: {row}");
    assert!(
        row.contains("SKILL superpowers:executing-plans [Official]"),
        "row: {row}"
    );
    assert!(row.contains("restrict:"), "row: {row}");
    assert!(row.contains("TOOL Bash(rm *) [Official]"), "row: {row}");
}

#[test]
fn runtime_badge_lines_returns_one_row_per_populated_group() {
    let s = crate::domain::signal::SignalSet {
        runtime_facts: vec![
            RuntimeFact::new(
                RuntimeFactKind::AutoMode,
                "YOLO mode",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::LoadedTool,
                "Bash",
                SourceKind::ProviderOfficial,
            ),
        ],
        ..crate::domain::signal::SignalSet::default()
    };
    let rows = runtime_badge_lines(&s);
    assert_eq!(rows.len(), 2);
    let text: Vec<String> = rows
        .into_iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    assert!(text.iter().any(|line| line.starts_with("modes")));
    assert!(text.iter().any(|line| line.starts_with("loaded")));
}

#[test]
fn runtime_badge_lines_render_session_facts() {
    let s = crate::domain::signal::SignalSet {
        runtime_facts: vec![
            RuntimeFact::new(
                RuntimeFactKind::SessionId,
                "fbd8bf4a-6dc7-4b50-9823-ab69bb3c5c26",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::ToolCalls,
                "42",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::TranscriptPath,
                "/home/chquan/.claude/projects/-home-chquan-Qmonster/fbd8bf4a-6dc7-4b50-9823-ab69bb3c5c26.jsonl",
                SourceKind::ProviderOfficial,
            ),
        ],
        ..crate::domain::signal::SignalSet::default()
    };

    let text = runtime_badge_lines_wrapped(&s, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains("session"), "text = {text:?}");
    assert!(text.contains("SID fbd8bf4a"), "text = {text:?}");
    assert!(text.contains("CALLS 42"), "text = {text:?}");
    assert!(text.contains("XSCRIPT"), "text = {text:?}");
}

#[test]
fn runtime_badge_lines_render_model_reset_facts() {
    let s = crate::domain::signal::SignalSet {
        runtime_facts: vec![RuntimeFact::new(
            RuntimeFactKind::ModelReset,
            "Gemini 2.5 Pro 5:00 PM (in 1h 12m)",
            SourceKind::ProviderOfficial,
        )],
        ..crate::domain::signal::SignalSet::default()
    };

    let text = runtime_badge_lines_wrapped(&s, 120)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains("limits"), "text = {text:?}");
    assert!(text.contains("RESET Gemini 2.5 Pro"), "text = {text:?}");
    assert!(text.contains("1h 12m"), "text = {text:?}");
}

#[test]
fn metric_badge_lines_surface_first_model_reset_fact() {
    let s = crate::domain::signal::SignalSet {
        runtime_facts: vec![RuntimeFact::new(
            RuntimeFactKind::ModelReset,
            "Pro 6:45 PM (19h 32m)",
            SourceKind::ProviderOfficial,
        )],
        ..crate::domain::signal::SignalSet::default()
    };

    let text = metric_badge_lines(&s, Provider::Claude, 120, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains("metrics"), "text = {text:?}");
    assert!(text.contains("RESET Pro 6:45 PM"), "text = {text:?}");
    assert!(text.contains("19h 32m"), "text = {text:?}");
}

#[test]
fn metric_badge_lines_wrap_many_badges_with_indented_continuations() {
    let s = crate::domain::signal::SignalSet {
        context_pressure: Some(MetricValue::new(0.71, SourceKind::Estimated)),
        quota_5h_pressure: Some(MetricValue::new(0.44, SourceKind::ProviderOfficial)),
        quota_weekly_pressure: Some(MetricValue::new(0.22, SourceKind::ProviderOfficial)),
        token_count: Some(MetricValue::new(
            1_530_000_u64,
            SourceKind::ProviderOfficial,
        )),
        cost_usd: Some(MetricValue::new(4.25, SourceKind::ProviderOfficial)),
        model_name: Some(MetricValue::new(
            "gpt-5.4".to_string(),
            SourceKind::ProviderOfficial,
        )),
        process_memory_mb: Some(MetricValue::new(188.3, SourceKind::Heuristic)),
        input_tokens: Some(MetricValue::new(200_000_u64, SourceKind::ProviderOfficial)),
        cached_input_tokens: Some(MetricValue::new(800_000_u64, SourceKind::ProviderOfficial)),
        ..SignalSet::default()
    };

    let rows = metric_badge_lines(&s, Provider::Claude, 48, false);
    let text: Vec<String> = rows.iter().map(line_text).collect();

    assert!(text.len() > 2, "badges should wrap: {text:?}");
    assert!(
        text.iter()
            .skip(1)
            .any(|line| line.starts_with("          ")),
        "continuation rows should hang under the label column: {text:?}"
    );
    assert!(text.join(" ").contains("CACHE 80.0%"), "text = {text:?}");
    assert!(
        text.iter().all(|line| line.chars().count() <= 48),
        "wrapped metric rows must fit the requested width: {text:?}"
    );
}

#[test]
fn metric_badge_lines_render_reset_etas() {
    let s = crate::domain::signal::SignalSet {
        quota_5h_resets_at: Some(MetricValue::new(
            future_unix_seconds(std::time::Duration::from_secs(2 * 3600)),
            SourceKind::ProviderOfficial,
        )),
        quota_weekly_resets_at: Some(MetricValue::new(
            future_unix_seconds(std::time::Duration::from_secs(8 * 3600)),
            SourceKind::ProviderOfficial,
        )),
        ..SignalSet::default()
    };

    let rows = metric_badge_lines(&s, Provider::Claude, 120, false);
    let text = rows.iter().map(line_text).collect::<Vec<_>>().join(" ");

    assert!(text.contains("RESET 5H"), "text = {text:?}");
    assert!(text.contains("RESET 7D"), "text = {text:?}");
    assert!(text.contains("[Official]"), "text = {text:?}");
}

#[test]
fn gauge_filled_empty_splits_by_ratio() {
    let (f, e) = gauge_filled_empty(0.0, GAUGE_WIDTH);
    assert_eq!(f, "");
    assert_eq!(e.chars().count(), GAUGE_WIDTH);
    let (f, e) = gauge_filled_empty(1.0, GAUGE_WIDTH);
    assert_eq!(f.chars().count(), GAUGE_WIDTH);
    assert_eq!(e, "");
    let (f, e) = gauge_filled_empty(0.5, GAUGE_WIDTH);
    assert_eq!(
        f.chars().count() + e.chars().count(),
        GAUGE_WIDTH,
        "filled+empty must always equal the track width: {f:?} + {e:?}"
    );
    assert!(f.contains('█'), "a half bar must have full blocks: {f:?}");
}

#[test]
fn gauge_rows_show_reset_only_when_quota_pressure_absent() {
    // F3 (diff-review): S1 can fill quota_*_resets_at without a status-line quota
    // pressure; the reset ETA must still render (not silently drop) under gauges.
    let s = crate::domain::signal::SignalSet {
        quota_5h_resets_at: Some(MetricValue::new(
            future_unix_seconds(std::time::Duration::from_secs(2 * 3600)),
            SourceKind::ProviderOfficial,
        )),
        ..crate::domain::signal::SignalSet::default()
    };
    let text = metric_badge_lines(&s, Provider::Codex, 120, true)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("5H") && text.contains("resets"),
        "reset-only 5H window must still show its eta under gauges: {text:?}"
    );
    assert!(
        !text.contains('%'),
        "a reset-only window must not render a bogus 0% gauge: {text:?}"
    );
}

#[test]
fn gauge_bar_line_fits_narrow_width() {
    // F4 (diff-review): on a narrow split the line must fit max_width — the bar
    // shrinks and the trailing is dropped rather than overflowing.
    let narrow = line_text(&gauge_bar_line("CTX", 0.58, Some("of 1.00M".into()), 18));
    assert!(
        narrow.chars().count() <= 18,
        "gauge line must fit max_width=18, got {} chars: {narrow:?}",
        narrow.chars().count()
    );
    let wide = line_text(&gauge_bar_line("CTX", 0.58, Some("of 1.00M".into()), 120));
    assert!(
        wide.contains("of 1.00M"),
        "wide keeps the trailing: {wide:?}"
    );
}

#[test]
fn expanded_card_renders_bounded_metrics_as_gauge_bars_not_badges() {
    let s = crate::domain::signal::SignalSet {
        context_pressure: Some(MetricValue::new(0.58, SourceKind::ProviderOfficial)),
        quota_5h_pressure: Some(MetricValue::new(0.88, SourceKind::ProviderOfficial)),
        ..crate::domain::signal::SignalSet::default()
    };
    // bounded_as_gauges = true → the expanded-card path (S3).
    let rows = metric_badge_lines(&s, Provider::Claude, 120, true);
    let text = rows.iter().map(line_text).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains('█'),
        "expanded card must render gauge bars: {text:?}"
    );
    assert!(text.contains("CTX"), "CTX bar label present: {text:?}");
    assert!(text.contains("5H"), "5H bar present: {text:?}");
    // The bounded value shows exactly once (as the bar), never also as a badge.
    assert_eq!(
        text.matches("58%").count(),
        1,
        "CTX value must appear once (bar only, no badge duplicate): {text:?}"
    );
}

#[test]
fn policy_mini_strip_provider_matrix_known_and_unknown_states() {
    use crate::domain::identity::Provider;
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{RuntimeFact, RuntimeFactKind};

    for provider in [Provider::Claude, Provider::Codex, Provider::Gemini] {
        let known = SignalSet {
            runtime_facts: vec![
                RuntimeFact::new(
                    RuntimeFactKind::PermissionMode,
                    "auto approve",
                    SourceKind::ProviderOfficial,
                ),
                RuntimeFact::new(
                    RuntimeFactKind::Sandbox,
                    "workspace-write",
                    SourceKind::ProviderOfficial,
                ),
            ],
            ..SignalSet::default()
        };
        assert_eq!(
            policy_mini_strip(&known, provider).as_deref(),
            Some("POLICY approval=auto sandbox=on [Heur]")
        );

        let unknown = SignalSet::default();
        assert_eq!(
            policy_mini_strip(&unknown, provider).as_deref(),
            Some("POLICY ?")
        );
    }
}

#[test]
fn metric_badge_line_includes_policy_mini_strip() {
    use crate::domain::identity::Provider;
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{RuntimeFact, RuntimeFactKind};
    let signals = SignalSet {
        runtime_facts: vec![
            RuntimeFact::new(
                RuntimeFactKind::PermissionMode,
                "manual approval",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::Sandbox,
                "off",
                SourceKind::ProviderOfficial,
            ),
        ],
        ..SignalSet::default()
    };
    let text = metric_badge_line(&signals, Provider::Codex)
        .into_iter()
        .map(|line| line_text(&line))
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains("POLICY approval=manual sandbox=off [Heur]"));
}

#[test]
fn runtime_badge_lines_wrap_many_facts_with_indented_continuations() {
    let s = crate::domain::signal::SignalSet {
        runtime_facts: vec![
            RuntimeFact::new(
                RuntimeFactKind::PermissionMode,
                "FullAccess",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::AutoMode,
                "YOLO",
                SourceKind::ProviderOfficial,
            ),
            RuntimeFact::new(
                RuntimeFactKind::Sandbox,
                "danger-full-access",
                SourceKind::ProviderOfficial,
            ),
        ],
        ..SignalSet::default()
    };

    let rows = runtime_badge_lines_wrapped(&s, 44);
    let text: Vec<String> = rows.iter().map(line_text).collect();

    assert!(text.len() >= 2, "runtime badges should wrap: {text:?}");
    assert!(
        text[1].starts_with("          "),
        "runtime continuation should be indented: {text:?}"
    );
    assert!(text.iter().any(|line| line.contains("SANDBOX")));
}

#[test]
fn metric_badge_line_returns_two_rows_when_context_fields_present() {
    let s = crate::domain::signal::SignalSet {
        token_count: Some(crate::domain::signal::MetricValue::new(
            1_530_000_u64,
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        git_branch: Some(crate::domain::signal::MetricValue::new(
            "main".to_string(),
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        ..crate::domain::signal::SignalSet::default()
    };
    let rows = metric_badge_line(&s, Provider::Claude);
    assert_eq!(
        rows.len(),
        2,
        "TOKENS on row 1 + BRANCH on row 2 → exactly two rows"
    );
}

#[test]
fn metric_badge_line_returns_single_row_when_only_primary_fields_present() {
    let s = crate::domain::signal::SignalSet {
        token_count: Some(crate::domain::signal::MetricValue::new(
            1_530_000_u64,
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        ..crate::domain::signal::SignalSet::default()
    };
    let rows = metric_badge_line(&s, Provider::Claude);
    assert_eq!(rows.len(), 1, "primary fields only → one row");
}

#[test]
fn metric_badge_line_returns_empty_vec_when_no_fields() {
    // v1.56.0 provider-honesty: Provider::Unknown / Qmonster suppress
    // the CACHE placeholder, so a fully empty SignalSet under those
    // providers still renders no badges. Claude/Codex would now emit
    // a `CACHE ?` placeholder; that case is covered by
    // metric_badge_line_emits_cache_placeholder_per_provider.
    let rows = metric_badge_line(
        &crate::domain::signal::SignalSet::default(),
        Provider::Unknown,
    );
    assert!(
        rows.is_empty(),
        "no fields populated → empty Vec (not a single empty Line)"
    );
}

#[test]
fn metric_badge_line_emits_cache_placeholder_per_provider() {
    // v1.56.0 provider-honesty regression guard: empty SignalSet
    // (no cache_hit_ratio, no token counts) must surface a per-
    // provider chip so operators don't silently lose situational
    // awareness about cache state.
    let s = crate::domain::signal::SignalSet::default();

    let claude = metric_badge_line(&s, Provider::Claude);
    let claude_dump: String = claude
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.clone().into_owned())
        .collect();
    assert!(
        claude_dump.contains("CACHE ?"),
        "Claude pane with no cache value should show ` CACHE ? `; got: {claude_dump}"
    );

    let gemini = metric_badge_line(&s, Provider::Gemini);
    let gemini_dump: String = gemini
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.clone().into_owned())
        .collect();
    assert!(
        gemini_dump.contains("CACHE ?"),
        "Gemini pane with no stats yet should show ` CACHE ? `; got: {gemini_dump}"
    );

    let qmonster = metric_badge_line(&s, Provider::Qmonster);
    assert!(
        qmonster.is_empty(),
        "Qmonster monitor pane should not surface a cache chip"
    );
}

#[test]
fn cache_chip_text_renders_per_provider_placeholder() {
    let s = crate::domain::signal::SignalSet::default();
    assert_eq!(
        cache_chip_text(&s, Provider::Claude).as_deref(),
        Some("cache ?"),
        "Claude exposes cache_hit_ratio but hasn't observed yet"
    );
    assert_eq!(
        cache_chip_text(&s, Provider::Codex).as_deref(),
        Some("cache ?"),
        "Codex computes cache via input/cached counts; no value yet → `cache ?`"
    );
    assert_eq!(
        cache_chip_text(&s, Provider::Gemini).as_deref(),
        Some("cache ?"),
        "Gemini has conditional cache stats; no stats yet → `cache ?`"
    );
    assert_eq!(
        cache_chip_text(&s, Provider::Qmonster),
        None,
        "Monitor pane has no meaningful cache chip"
    );
    assert_eq!(
        cache_chip_text(&s, Provider::Unknown),
        None,
        "Unknown provider stays mute rather than guessing"
    );
}

#[test]
fn cache_chip_text_marks_gemini_stats_without_cache_reads_as_unsupported() {
    let s = crate::domain::signal::SignalSet {
        input_tokens: Some(crate::domain::signal::MetricValue::new(
            100_u64,
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        output_tokens: Some(crate::domain::signal::MetricValue::new(
            10_u64,
            crate::domain::origin::SourceKind::ProviderOfficial,
        )),
        ..crate::domain::signal::SignalSet::default()
    };

    assert_eq!(
        cache_chip_text(&s, Provider::Gemini).as_deref(),
        Some("cache \u{2014}"),
        "Gemini stats are present but Cache Reads is absent → unsupported for this auth surface"
    );
}

#[test]
fn state_row_renders_input_wait_with_yellow_glyph_and_label() {
    let entered_at = std::time::Instant::now();
    let line = format_state_row(IdleCause::InputWait, entered_at);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.starts_with("state   : "),
        "field alignment missing: {text}"
    );
    assert!(text.contains("⏸"), "glyph missing: {text}");
    assert!(text.contains("WAIT (input)"), "label missing: {text}");
    assert!(
        text.contains("INPUT NEEDED"),
        "persistent action marker missing: {text}"
    );
    assert!(text.contains("⏱"), "elapsed badge missing: {text}");
}

#[test]
fn state_row_renders_limit_hit_as_usage_limit() {
    let entered_at = std::time::Instant::now();
    let line = format_state_row(IdleCause::LimitHit, entered_at);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("⛔"), "glyph missing: {text}");
    assert!(
        text.contains("USAGE LIMIT"),
        "limit state must be distinct from normal idle: {text}"
    );
}

#[test]
fn state_row_marks_recent_transition_as_changed() {
    let now = std::time::Instant::now();
    let flash = PaneStateFlash::new(Some(IdleCause::InputWait), now);
    let line = format_state_row_with_flash(IdleCause::InputWait, now, now, Some(&flash));
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("WAIT (input)"), "label missing: {text}");
    assert!(text.contains("CHANGED"), "changed badge missing: {text}");
}

#[test]
fn selection_highlight_stays_stable_during_state_flash() {
    let style = highlight_style(true);
    assert_eq!(
        style.fg, None,
        "selected panes must not override state badge foregrounds"
    );
    assert_eq!(
        style.bg, None,
        "selected panes must not override state flash backgrounds"
    );
    assert!(
        !style.add_modifier.contains(Modifier::BOLD),
        "selection should not bold every row in an expanded pane"
    );
    assert!(
        !style.add_modifier.contains(Modifier::UNDERLINED),
        "selection should not underline every row in an expanded pane"
    );
}

#[test]
fn selection_marker_does_not_encode_state_flash() {
    assert_eq!(highlight_symbol(), "▶ ");
    assert!(!repeat_highlight_symbol());
}

#[test]
fn state_flash_header_names_state_changed_for_all_selection_states() {
    let rep = base_report();
    let now = std::time::Instant::now();
    let flash = PaneStateFlash::new(None, now);
    let expanded_lines = pane_list_lines_with_flash(
        &rep,
        true,
        false,
        now,
        Some(&flash),
        BADGE_WRAP_FALLBACK_WIDTH,
    );
    let collapsed_lines = pane_list_lines_with_flash(
        &rep,
        false,
        false,
        now,
        Some(&flash),
        BADGE_WRAP_FALLBACK_WIDTH,
    );
    let expanded_text: String = expanded_lines[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let collapsed_text: String = collapsed_lines[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    for text in [&expanded_text, &collapsed_text] {
        assert!(text.contains("STATE CHANGED"), "flash missing: {text}");
        assert!(text.contains("ACTIVE"), "active badge missing: {text}");
        assert!(text.contains("qwork:1"), "pane title missing: {text}");
    }
    assert!(
        expanded_lines[0].spans[0].style.bg.is_some(),
        "STATE CHANGED title prefix must be a visible badge"
    );
    assert!(
        expanded_lines[0].spans[2].style.bg.is_some(),
        "ACTIVE title prefix must be a visible badge"
    );
}

#[test]
fn idle_header_state_label_persists_after_flash_window() {
    let mut rep = base_report();
    rep.idle_state = Some(IdleCause::InputWait);
    rep.idle_state_entered_at = Some(std::time::Instant::now());
    let now = std::time::Instant::now();
    let expired_flash = PaneStateFlash::new(Some(IdleCause::InputWait), now - STATE_FLASH_DURATION);
    let lines = pane_list_lines_with_flash(
        &rep,
        true,
        false,
        now,
        Some(&expired_flash),
        BADGE_WRAP_FALLBACK_WIDTH,
    );
    let title: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(title.contains("WAIT INPUT"), "idle title missing: {title}");
    assert!(title.contains("qwork:1"), "pane title missing: {title}");
    assert!(
        !title.contains("STATE CHANGED"),
        "expired flash must not keep transition wording: {title}"
    );
    assert!(
        lines[0].spans[0].style.bg.is_some(),
        "persistent idle title prefix must be rendered as a visible badge"
    );
}

#[test]
fn idle_state_title_prefix_is_same_for_expanded_and_collapsed_cards() {
    let mut rep = base_report();
    rep.idle_state = Some(IdleCause::PermissionWait);
    rep.idle_state_entered_at = Some(std::time::Instant::now());
    let expanded = pane_list_lines_with_flash(
        &rep,
        true,
        false,
        Instant::now(),
        None,
        BADGE_WRAP_FALLBACK_WIDTH,
    );
    let collapsed = pane_list_lines_with_flash(
        &rep,
        false,
        false,
        Instant::now(),
        None,
        BADGE_WRAP_FALLBACK_WIDTH,
    );
    let expanded_title: String = expanded[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let collapsed_title: String = collapsed[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    assert!(expanded_title.contains("WAIT APPROVAL"));
    assert!(expanded_title.contains("qwork:1"));
    assert!(collapsed_title.contains("WAIT APPROVAL"));
    assert!(collapsed_title.contains("qwork:1"));
    assert!(
        expanded[0].spans[0].style.bg.is_some(),
        "expanded selected title prefix must be badge-styled"
    );
    assert!(
        collapsed[0].spans[0].style.bg.is_some(),
        "collapsed unselected title prefix must be badge-styled"
    );
}

#[test]
fn active_transition_renders_temporary_state_row() {
    let rep = base_report();
    let now = std::time::Instant::now();
    let flash = PaneStateFlash::new(None, now);
    let lines =
        render_pane_state_row_with_flash(&rep, now, Some(&flash), BADGE_WRAP_FALLBACK_WIDTH);
    assert_eq!(lines.len(), 1);
    let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("ACTIVE"), "active badge missing: {text}");
    assert!(text.contains("CHANGED"), "changed badge missing: {text}");
}

#[test]
fn expired_active_transition_flash_removes_state_row() {
    let rep = base_report();
    let now = std::time::Instant::now();
    let flash = PaneStateFlash::new(None, now - STATE_FLASH_DURATION - Duration::from_millis(1));
    let lines =
        render_pane_state_row_with_flash(&rep, now, Some(&flash), BADGE_WRAP_FALLBACK_WIDTH);
    assert!(
        lines.is_empty(),
        "expired flash should not render state row"
    );
}

#[test]
fn format_elapsed_uses_clock_style_for_fast_scanning() {
    assert_eq!(format_elapsed(std::time::Duration::from_secs(5)), "00:05");
    assert_eq!(format_elapsed(std::time::Duration::from_secs(65)), "01:05");
    assert_eq!(
        format_elapsed(std::time::Duration::from_secs(3_665)),
        "1:01:05"
    );
}

#[test]
fn idle_state_tints_header_when_no_recommendations_are_active() {
    let mut rep = base_report();
    rep.idle_state = Some(IdleCause::PermissionWait);
    assert_eq!(
        pane_header_color(&rep),
        idle_header_color(IdleCause::PermissionWait)
    );
}

#[test]
fn state_row_omitted_for_idle_state_none() {
    let rep = base_report();
    let lines = render_pane_state_row(&rep);
    assert!(lines.is_empty(), "no state row when idle_state is None");
}

#[test]
fn format_agent_memory_bytes_below_mib_renders_kb() {
    assert_eq!(format_agent_memory_bytes(48_000), "46 KB");
    assert_eq!(format_agent_memory_bytes(1023 * 1024), "1023 KB");
}

#[test]
fn format_agent_memory_bytes_at_or_above_mib_renders_mb() {
    assert_eq!(format_agent_memory_bytes(1024 * 1024), "1.0 MB");
    assert_eq!(format_agent_memory_bytes(2_500_000), "2.4 MB");
}

#[test]
fn format_agent_memory_bytes_below_kib_renders_lt_1_kb() {
    assert_eq!(format_agent_memory_bytes(0), "<1 KB");
    assert_eq!(format_agent_memory_bytes(536), "<1 KB");
    assert_eq!(format_agent_memory_bytes(1023), "<1 KB");
    assert_eq!(format_agent_memory_bytes(1024), "1 KB");
}

#[test]
fn metric_row_emits_memory_file_when_agent_memory_bytes_set() {
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, SignalSet};
    let s = SignalSet {
        agent_memory_bytes: Some(MetricValue::new(48_000_u64, SourceKind::Heuristic)),
        ..SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("memory-file 46 KB [Heur]"), "row = {row}");
}

#[test]
fn metric_badge_line_includes_mem_file_badge_when_agent_memory_bytes_set() {
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, SignalSet};
    let s = SignalSet {
        agent_memory_bytes: Some(MetricValue::new(48_000_u64, SourceKind::Heuristic)),
        ..SignalSet::default()
    };
    let lines = metric_badge_line(&s, Provider::Claude);
    assert!(
        !lines.is_empty(),
        "MEM-FILE should render at least one Line"
    );
    let rendered: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|sp| sp.content.as_ref())
        .collect();
    assert!(rendered.contains("MEM-FILE 46 KB"), "rendered = {rendered}");
    assert!(rendered.contains("[Heur]"), "rendered = {rendered}");
}

#[test]
fn metric_badge_line_omits_mem_file_when_agent_memory_bytes_none() {
    use crate::domain::signal::SignalSet;
    let s = SignalSet::default();
    let lines = metric_badge_line(&s, Provider::Claude);
    let rendered: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|sp| sp.content.as_ref())
        .collect();
    assert!(
        !rendered.contains("MEM-FILE"),
        "MEM-FILE must not render when agent_memory_bytes is None; rendered = {rendered}"
    );
}

#[test]
fn render_token_sparkline_emits_token_label_and_seven_block_chars() {
    use crate::domain::identity::Provider;
    use crate::store::TokenSample;
    let samples = (0..8)
        .map(|i| TokenSample {
            ts_unix_ms: (i * 1000) as i64,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some((i as u64) * 100),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        })
        .collect::<Vec<_>>();
    let line = render_token_sparkline(&samples).expect("8 samples must render");
    let rendered: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
    assert!(rendered.starts_with("TOKENS "), "rendered = {rendered}");
    let body = &rendered["TOKENS ".len()..];
    let block_chars: Vec<char> = body.chars().filter(|c| "▁▂▃▄▅▆▇█".contains(*c)).collect();
    assert_eq!(
        block_chars.len(),
        7,
        "expected 7 block chars; got body={body:?}"
    );
}

#[test]
fn render_token_sparkline_returns_none_when_fewer_than_two_samples() {
    use crate::domain::identity::Provider;
    use crate::store::TokenSample;
    assert!(render_token_sparkline(&[]).is_none());
    assert!(
        render_token_sparkline(&[TokenSample {
            ts_unix_ms: 0,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(100),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        }])
        .is_none(),
        "one sample = no delta = no line"
    );
}

#[test]
fn selected_pane_shows_token_collecting_state_near_metrics() {
    let rep = base_report();
    let expanded = pane_list_lines(&rep, true, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    let collapsed = pane_list_lines(&rep, false, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert!(
        expanded
            .iter()
            .any(|line| line.contains("tokens  :  TOKENS collecting 0/2")),
        "selected pane should explain why sparkline is not ready: {expanded:?}"
    );
    assert!(
        !collapsed
            .iter()
            .any(|line| line.contains("TOKENS collecting")),
        "collapsed panes should stay compact: {collapsed:?}"
    );
}

#[test]
fn selected_qmonster_pane_omits_token_row_even_with_stale_samples() {
    use crate::store::TokenSample;

    let mut rep = base_report();
    rep.provider = Provider::Qmonster;
    rep.identity.identity.provider = Provider::Qmonster;
    rep.identity.identity.role = Role::Monitor;
    rep.recent_token_samples = (0..3)
        .map(|i| TokenSample {
            ts_unix_ms: (i * 1000) as i64,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(100_000 + (i as u64) * 1_000),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        })
        .collect();

    let lines = pane_list_lines(&rep, true, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert!(
        !lines.iter().any(|line| line.contains("TOKENS")),
        "Qmonster monitor pane should not render token sparkline rows: {lines:?}"
    );
}

#[test]
fn selected_pane_renders_token_sparkline_before_recommendation_details() {
    use crate::domain::identity::Provider;
    use crate::domain::recommendation::{Recommendation, Severity};
    use crate::store::TokenSample;

    let mut rep = base_report();
    rep.recent_token_samples = (0..4)
        .map(|i| TokenSample {
            ts_unix_ms: (i * 1000) as i64,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some((i as u64) * 250),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        })
        .collect();
    rep.recommendations = vec![Recommendation {
        action: "ctx: compact soon",
        reason: "context pressure is high".into(),
        severity: Severity::Warning,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: Some("/compact".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: None,
    }];

    let lines = pane_list_lines(&rep, true, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    let tokens_idx = lines
        .iter()
        .position(|line| line.contains("tokens  :  TOKENS"))
        .expect("sparkline row should render near metrics");
    let rec_idx = lines
        .iter()
        .position(|line| line.contains("context pressure is high"))
        .expect("recommendation detail should render");

    assert!(
        tokens_idx < rec_idx,
        "sparkline should be visible before long recommendation details: {lines:?}"
    );
}

#[test]
fn selected_pane_renders_token_input_output_when_available() {
    let mut rep = base_report();
    rep.signals.input_tokens = Some(MetricValue::new(1_234_567, SourceKind::ProviderOfficial));
    rep.signals.output_tokens = Some(MetricValue::new(45_678, SourceKind::ProviderOfficial));

    let lines = pane_list_lines(&rep, true, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    let io = lines
        .iter()
        .find(|line| line.contains("token io"))
        .expect("expanded selected pane should show token input/output line");

    assert!(io.contains("Main 1.23M in / 45.7K out [Official]"));
}

#[test]
fn selected_pane_renders_cache_read_and_create_counts_when_available() {
    let mut rep = base_report();
    rep.signals.cached_input_tokens = Some(MetricValue::new(150_000, SourceKind::ProviderOfficial));
    rep.signals.cache_creation_input_tokens =
        Some(MetricValue::new(770, SourceKind::ProviderOfficial));

    let lines = pane_list_lines(&rep, true, false)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    let io = lines
        .iter()
        .find(|line| line.contains("cache io"))
        .expect("expanded selected pane should show cache read/create counts");

    assert!(io.contains("read 150.0K [Official] / create 770 [Official]"));
}

#[test]
fn selected_pane_token_sparkline_renders_latest_and_delta_values() {
    use crate::domain::identity::Provider;
    use crate::store::TokenSample;

    let samples = vec![
        TokenSample {
            ts_unix_ms: 3_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1_300),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: Some(2_200),
        },
        TokenSample {
            ts_unix_ms: 2_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1_100),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: Some(2_100),
        },
        TokenSample {
            ts_unix_ms: 1_000,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some(1_000),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: Some(2_000),
        },
    ];

    let signals = SignalSet {
        token_count: Some(MetricValue::new(9_000, SourceKind::ProviderOfficial)),
        ..SignalSet::default()
    };
    let line = token_sparkline_status_line(&samples, &signals);
    let rendered = line_text(&line);

    assert!(
        rendered.contains("TOKENS 9.0K · prompt 3.5K"),
        "rendered = {rendered}"
    );
    assert!(rendered.contains("+500/3p"), "rendered = {rendered}");
}

#[test]
fn selected_pane_token_sparkline_uses_no_badge_background() {
    use crate::domain::identity::Provider;
    use crate::store::TokenSample;

    let samples = (0..4)
        .map(|i| TokenSample {
            ts_unix_ms: (i * 1000) as i64,
            pane_id: "%1".into(),
            provider: Provider::Codex,
            input_tokens: Some((i as u64) * 250),
            output_tokens: None,
            cost_usd: None,
            cached_input_tokens: None,
        })
        .collect::<Vec<_>>();
    let line = token_sparkline_status_line(&samples, &SignalSet::default());

    for span in line.spans {
        assert!(
            span.style.bg.is_none(),
            "sparkline row spans must avoid badge backgrounds: {:?}",
            span.content.as_ref()
        );
    }
}

#[test]
fn format_cache_hit_ratio_returns_none_when_both_inputs_are_none() {
    assert!(format_cache_hit_ratio(None, None).is_none());
}

#[test]
fn format_cache_hit_ratio_returns_none_when_cached_is_zero_and_input_is_zero() {
    assert!(format_cache_hit_ratio(Some(0), Some(0)).is_none());
}

#[test]
fn format_cache_hit_ratio_codex_welcome_panel_example() {
    // input=189703, cached=1317376 → ratio = 1317376/(189703+1317376) ≈ 0.8741 → "87.4%"
    assert_eq!(
        format_cache_hit_ratio(Some(189_703), Some(1_317_376)).as_deref(),
        Some("87.4%")
    );
}

#[test]
fn metric_row_emits_cache_when_cached_input_tokens_set() {
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, SignalSet};
    let s = SignalSet {
        input_tokens: Some(MetricValue::new(189_703_u64, SourceKind::ProviderOfficial)),
        cached_input_tokens: Some(MetricValue::new(
            1_317_376_u64,
            SourceKind::ProviderOfficial,
        )),
        ..SignalSet::default()
    };
    let row = metric_row(&s, Provider::Claude);
    assert!(row.contains("cache 87.4% [Official]"), "row = {row}");
}

#[test]
fn metric_badge_line_includes_cache_badge_when_cached_input_tokens_set() {
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, SignalSet};
    let s = SignalSet {
        input_tokens: Some(MetricValue::new(189_703_u64, SourceKind::ProviderOfficial)),
        cached_input_tokens: Some(MetricValue::new(
            1_317_376_u64,
            SourceKind::ProviderOfficial,
        )),
        ..SignalSet::default()
    };
    let lines = metric_badge_line(&s, Provider::Claude);
    let rendered: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|sp| sp.content.as_ref())
        .collect();
    assert!(rendered.contains("CACHE 87.4%"), "rendered = {rendered}");
    assert!(rendered.contains("[Official]"), "rendered = {rendered}");
}

#[test]
fn wrap_aligned_field_no_wrap_when_value_fits() {
    let lines = wrap_aligned_field("path", "/home/u/repo", 80);
    assert_eq!(lines.len(), 1);
    assert!(line_text(&lines[0]).starts_with("path    : /home/u/repo"));
}

#[test]
fn wrap_aligned_field_continuation_indent_matches_label_col() {
    let value = "a/very/long/path/that/will/definitely/wrap/across/lines";
    let lines = wrap_aligned_field("path", value, 30);
    assert!(lines.len() >= 2);
    let cont = line_text(&lines[1]);
    assert!(cont.starts_with("          ")); // 8 (label) + 2 (": ") = 10 spaces
    assert!(!cont.trim_start().is_empty());
}

#[test]
fn wrap_aligned_field_handles_narrow_width_gracefully() {
    let lines = wrap_aligned_field("path", "/x/y/z", 5);
    assert_eq!(lines.len(), 1, "narrow width returns single uncut line");
}

fn sample_pane_report() -> PaneReport {
    base_report()
}

#[test]
fn pane_card_path_line_wraps_when_narrow() {
    let mut rep = sample_pane_report();
    rep.current_path = "/home/operator/long/worktree/path/that/exceeds/width".into();
    let lines = pane_list_lines_with_width(&rep, true, false, 30);
    // Sectioned rows render as a tree: the path row sits first in IDENTITY
    // (not last), so its first line is prefixed with `├ ` and its
    // wrapped continuation lines carry the parent's `│ ` bar followed by
    // the usual `cont_indent` (label_col + ": " worth of spaces = 10).
    let start = lines
        .iter()
        .position(|l| line_text(l).starts_with("├ path    : "))
        .expect("path row must be present");
    let cont_prefix = "│           "; // │ + space + 10-space cont_indent
    let mut path_lines: Vec<&Line> = vec![&lines[start]];
    for line in lines.iter().skip(start + 1) {
        if line_text(line).starts_with(cont_prefix) {
            path_lines.push(line);
        } else {
            break;
        }
    }
    assert!(path_lines.len() >= 2, "long path must wrap into ≥ 2 lines");
    let cont = line_text(path_lines[1]);
    assert!(
        cont.starts_with(cont_prefix),
        "continuation must carry the parent bar and value-column indent, got: {cont:?}"
    );
}

#[test]
fn pane_state_row_wraps_long_marker_text() {
    let mut rep = sample_pane_report();
    rep.idle_state = Some(crate::domain::signal::IdleCause::InputWait);
    let now = std::time::Instant::now();
    rep.idle_state_entered_at = Some(now);
    let flash = PaneStateFlash::new(rep.idle_state, now);

    // Narrow viewport: the state row's badge sequence (label + idle glyph
    // + persistent marker + elapsed clock + CHANGED) totals ≥ 30 cells, so
    // a 25-col wrap budget must produce ≥ 2 lines via wrap_badge_line.
    let lines = render_pane_state_row_with_flash(&rep, now, Some(&flash), 25);
    assert!(
        lines.len() >= 2,
        "state row must wrap on narrow width, got {} lines",
        lines.len()
    );

    // First line begins with the "state   : " label column.
    assert!(line_text(&lines[0]).starts_with("state"));

    // Wide viewport: same row fits in one line.
    let wide_lines =
        render_pane_state_row_with_flash(&rep, now, Some(&flash), BADGE_WRAP_FALLBACK_WIDTH);
    assert_eq!(
        wide_lines.len(),
        1,
        "state row must NOT wrap at default width"
    );
}

#[test]
fn pane_card_severity_reason_wraps_long_text() {
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::{Recommendation, Severity};

    let mut rep = sample_pane_report();
    rep.recommendations = vec![Recommendation {
        action: "test",
        reason: "x ".repeat(60), // 120 chars
        severity: Severity::Concern,
        source_kind: SourceKind::ProjectCanonical,
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: None,
    }];
    let lines = pane_list_lines_with_width(&rep, true, false, 40);
    let reason_lines: Vec<&Line> = lines
        .iter()
        .filter(|l| line_text(l).contains("x x"))
        .collect();
    assert!(
        reason_lines.len() >= 2,
        "long reason must wrap, got {} matching lines",
        reason_lines.len()
    );
}

#[test]
fn pane_index_at_row_handles_wrapped_path_line() {
    let mut rep_a = sample_pane_report();
    rep_a.current_path = "/extremely/long/path/".repeat(8);
    let mut rep_b = sample_pane_report();
    rep_b.pane_id = "%999".into();
    let reports = vec![rep_a, rep_b];
    let mut state = ListState::default();
    state.select(Some(0));

    let wrap_width = 40u16;
    let first_height = pane_list_lines_with_width(&reports[0], true, true, wrap_width).len() as u16;
    assert!(
        first_height >= 5,
        "expanded card with long path should produce many rows, got {first_height}"
    );

    let idx = pane_index_at_row(&reports, &state, first_height, wrap_width);
    assert_eq!(idx, Some(1));
}

#[test]
fn pane_help_topic_at_row_maps_sectioned_core_rows() {
    use crate::ui::help_glossary::HelpTopic;

    let mut rep = sample_pane_report();
    rep.current_path = "/repo".into();
    rep.current_command = "claude --dangerously-skip-permissions".into();
    rep.signals.cost_usd = Some(MetricValue::new(3.0, SourceKind::ProviderOfficial));
    let reports = vec![rep];
    let mut state = ListState::default();
    state.select(Some(0));

    // New 4-section order: IDENTITY (header, path, cmd) → NOW (header,
    // status) → PRESSURE → NEXT. This report has no idle_state, so the NOW
    // section has no state row — only the `status` row (moved here from the
    // old WHERE section).
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, 0, 100),
        Some(HelpTopic::PaneHeader)
    );
    // IDENTITY header carries the section topic (PanePath).
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, 1, 100),
        Some(HelpTopic::PanePath)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, 2, 100),
        Some(HelpTopic::PanePath)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, 3, 100),
        Some(HelpTopic::PaneCommand)
    );
    // NOW header carries PaneState.
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, 4, 100),
        Some(HelpTopic::PaneState)
    );
    // status row moved into NOW.
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, 5, 100),
        Some(HelpTopic::PaneStatus)
    );
    // PRESSURE header carries PaneMetrics.
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, 6, 100),
        Some(HelpTopic::PaneMetrics)
    );
}

#[test]
fn expanded_pane_uses_sectioned_single_column_order() {
    use crate::domain::recommendation::{Recommendation, RequestedEffect, Severity};

    let mut rep = sample_pane_report();
    rep.current_path = "/home/chquan/Qmonster".into();
    rep.current_command = "codex".into();
    rep.signals.cost_usd = Some(MetricValue::new(3.0, SourceKind::ProviderOfficial));
    rep.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::Sandbox,
        "workspace-write",
        SourceKind::ProviderOfficial,
    ));
    rep.effects.push(RequestedEffect::PromptSendProposed {
        target_pane_id: rep.pane_id.clone(),
        slash_command: "/compact".into(),
        proposal_id: "proposal-1".into(),
    });
    rep.recommendations = vec![Recommendation {
        action: "cache: drift detected",
        reason: "cache drift detected - /compact will let cache rebuild".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: Some("/compact".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: Some("snapshot before compact".into()),
    }];

    let lines = pane_list_lines_with_width(&rep, true, false, 120);
    let text = line_texts(&lines);

    // New 4-section order: IDENTITY → NOW → PRESSURE (metrics + merged
    // runtime) → NEXT.
    let title = index_containing(&text, "qwork:1");
    let identity = index_containing(&text, "IDENTITY");
    let path = index_containing(&text, "path    : /home/chquan/Qmonster");
    let command = index_containing(&text, "cmd     : codex");
    let now = index_containing(&text, "NOW");
    let proposal = index_containing(&text, "proposal");
    let pressure = index_containing(&text, "PRESSURE");
    let metrics = index_containing(&text, "metrics");
    let modes = index_containing(&text, "modes");
    let next = index_containing(&text, "NEXT");
    let rec = index_containing(&text, "CONCERN");

    // RUNTIME header is gone — runtime facts (modes) live inside PRESSURE.
    assert!(
        !text.iter().any(|line| line.trim() == "RUNTIME"),
        "RUNTIME header should be merged into PRESSURE: {text:#?}"
    );

    assert!(title < identity, "title should precede IDENTITY: {text:#?}");
    assert!(
        identity < path,
        "IDENTITY should contain path rows: {text:#?}"
    );
    assert!(path < command, "path should precede command: {text:#?}");
    assert!(command < now, "IDENTITY should precede NOW: {text:#?}");
    assert!(
        now < proposal,
        "NOW should contain proposal rows: {text:#?}"
    );
    assert!(
        proposal < pressure,
        "NOW should precede PRESSURE: {text:#?}"
    );
    assert!(
        pressure < metrics,
        "PRESSURE should contain metrics: {text:#?}"
    );
    assert!(
        metrics < modes,
        "PRESSURE should contain merged runtime facts after metrics: {text:#?}"
    );
    assert!(
        modes < next,
        "PRESSURE (incl. runtime) should precede NEXT: {text:#?}"
    );
    assert!(
        next < rec,
        "NEXT should contain recommendation rows: {text:#?}"
    );
}

#[test]
fn expanded_pane_help_topics_follow_sectioned_rows() {
    use crate::domain::recommendation::{Recommendation, RequestedEffect, Severity};
    use crate::ui::help_glossary::HelpTopic;

    let mut rep = sample_pane_report();
    let now = std::time::Instant::now();
    rep.idle_state = Some(IdleCause::InputWait);
    rep.idle_state_entered_at = Some(now);
    rep.effects.push(RequestedEffect::PromptSendProposed {
        target_pane_id: rep.pane_id.clone(),
        slash_command: "/compact".into(),
        proposal_id: "proposal-1".into(),
    });
    rep.signals.cost_usd = Some(MetricValue::new(3.0, SourceKind::ProviderOfficial));
    rep.signals.input_tokens = Some(MetricValue::new(1_234_567, SourceKind::ProviderOfficial));
    rep.signals.output_tokens = Some(MetricValue::new(45_678, SourceKind::ProviderOfficial));
    rep.signals.cached_input_tokens = Some(MetricValue::new(150_000, SourceKind::ProviderOfficial));
    rep.signals.cache_creation_input_tokens =
        Some(MetricValue::new(770, SourceKind::ProviderOfficial));
    rep.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::Sandbox,
        "workspace-write",
        SourceKind::ProviderOfficial,
    ));
    rep.recommendations = vec![Recommendation {
        action: "cache: drift detected",
        reason: "cache drift detected - /compact will let cache rebuild".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: Some("/compact".into()),
        side_effects: vec![],
        is_strong: false,
        next_step: Some("snapshot before compact".into()),
    }];

    let reports = vec![rep];
    let mut state = ListState::default();
    state.select(Some(0));

    let lines = pane_list_lines_with_width(&reports[0], true, false, 120);
    let text = line_texts(&lines);

    // New 4-section order: IDENTITY → NOW → PRESSURE (metrics + tokens +
    // merged runtime) → NEXT. Each row keeps its hover HelpTopic; only the
    // section a row sits under changed.
    let row_expectations = [
        // IDENTITY header carries PanePath; path/cmd rows under it.
        (index_containing(&text, "IDENTITY"), HelpTopic::PanePath),
        (index_containing(&text, "path"), HelpTopic::PanePath),
        (index_containing(&text, "cmd"), HelpTopic::PaneCommand),
        // NOW header carries PaneState; state + status rows under it; the
        // proposal row stays a PaneRecommendation topic.
        (index_containing(&text, "NOW"), HelpTopic::PaneState),
        (index_containing(&text, "state"), HelpTopic::PaneState),
        (index_containing(&text, "status"), HelpTopic::PaneStatus),
        (
            index_containing(&text, "proposal"),
            HelpTopic::PaneRecommendation,
        ),
        // PRESSURE header carries PaneMetrics; metrics + token rows + the
        // merged runtime `modes` row all live here now.
        (index_containing(&text, "PRESSURE"), HelpTopic::PaneMetrics),
        (index_containing(&text, "metrics"), HelpTopic::PaneMetrics),
        (index_containing(&text, "tokens  :"), HelpTopic::PaneTokens),
        (index_containing(&text, "token io"), HelpTopic::PaneTokens),
        (index_containing(&text, "cache io"), HelpTopic::PaneTokens),
        (index_containing(&text, "modes"), HelpTopic::PaneRuntime),
        // NEXT header carries PaneRecommendation; rec rows under it.
        (
            index_containing(&text, "NEXT"),
            HelpTopic::PaneRecommendation,
        ),
        (
            index_containing(&text, "CONCERN"),
            HelpTopic::PaneRecommendation,
        ),
        (
            index_containing(&text, "next"),
            HelpTopic::PaneRecommendation,
        ),
    ];

    for (row, expected) in row_expectations {
        assert_eq!(
            pane_help_topic_at_row(&reports, &state, row as u16, 120),
            Some(expected),
            "row {row} should map to {expected:?}: {text:#?}"
        );
    }
}

#[test]
fn collapsed_pane_keeps_flat_layout_without_section_headers() {
    let mut rep = sample_pane_report();
    rep.current_path = "/home/chquan/Qmonster".into();
    rep.current_command = "codex".into();
    rep.signals.context_pressure = Some(MetricValue::new(0.55, SourceKind::ProviderOfficial));
    rep.signals.cost_usd = Some(MetricValue::new(3.0, SourceKind::ProviderOfficial));
    rep.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::Sandbox,
        "workspace-write",
        SourceKind::ProviderOfficial,
    ));

    let lines = pane_list_lines_with_width(&rep, false, false, 120);
    let text = line_texts(&lines);

    for header in ["IDENTITY", "NOW", "PRESSURE", "NEXT"] {
        assert!(
            !text.iter().any(|line| line.trim() == header),
            "collapsed pane must not render section header {header}: {text:#?}"
        );
    }
    assert!(
        text.iter().any(|line| line.contains("CTX")),
        "collapsed pane should show the CTX gauge bar: {text:#?}"
    );
}

#[test]
fn non_selected_pane_shows_only_gauge_bars_not_dense_fields() {
    // v3.1.2 (operator ask 2026-06-30): scanning three panes' full detail at
    // once is too much. A non-selected (collapsed) pane now shows ONLY its
    // ctx / 5h / 7d gauge bars beneath the title+status pill; the dense
    // path / cmd / status / signals / metric-badge block is reserved for the
    // SELECTED (expanded) pane.
    let mut rep = sample_pane_report();
    rep.current_path = "/home/chquan/Qmonster".into();
    rep.current_command = "codex".into();
    rep.signals.context_pressure = Some(MetricValue::new(0.62, SourceKind::ProviderOfficial));
    rep.signals.quota_5h_pressure = Some(MetricValue::new(0.40, SourceKind::ProviderOfficial));
    rep.signals.quota_weekly_pressure = Some(MetricValue::new(0.20, SourceKind::ProviderOfficial));

    let collapsed = line_texts(&pane_list_lines_with_width(&rep, false, false, 120));

    for gauge in ["CTX", "5H", "7D"] {
        assert!(
            collapsed.iter().any(|l| l.contains(gauge)),
            "compact pane must show the {gauge} gauge bar: {collapsed:#?}"
        );
    }
    for dense in ["path    :", "cmd     :", "status  :", "signals :"] {
        assert!(
            !collapsed.iter().any(|l| l.starts_with(dense)),
            "compact pane must drop dense field {dense:?}: {collapsed:#?}"
        );
    }
}

#[test]
fn pane_expansion_follows_selection_uniformly() {
    // v3.1.3 (operator ask 2026-07-01): the pane list has NO forced selection.
    // With nothing selected every pane renders compact (clean gauge-tile view);
    // selecting a pane expands exactly that one to full detail — every provider
    // behaves the same, the Qmonster monitor pane included (selecting it shows
    // its detail like before, not just the one-line compact tile).
    assert!(
        !pane_is_expanded(0, None),
        "with nothing selected, every pane must stay compact"
    );
    assert!(
        pane_is_expanded(0, Some(0)),
        "the selected pane must expand"
    );
    assert!(
        !pane_is_expanded(1, Some(0)),
        "a non-selected pane must stay compact"
    );
    assert!(
        pane_is_expanded(2, Some(2)),
        "any selected pane expands — Qmonster monitor pane included"
    );
}

#[test]
fn expanded_pane_keeps_empty_recommendations_state_under_section() {
    let rep = sample_pane_report();
    let lines = pane_list_lines_with_width(&rep, true, false, 120);
    let text = line_texts(&lines);

    let next = index_containing(&text, "NEXT");
    let empty = index_containing(&text, "status  : no active recommendations");

    assert!(
        next < empty,
        "empty recommendation state should live under NEXT: {text:#?}"
    );
}

#[test]
fn pane_card_shows_inline_proposal_line_when_pending() {
    use crate::domain::recommendation::RequestedEffect;
    let mut rep = sample_pane_report();
    rep.effects.push(RequestedEffect::PromptSendProposed {
        target_pane_id: rep.pane_id.clone(),
        slash_command: "/compact".into(),
        proposal_id: "pid-1".into(),
    });
    let lines = pane_list_lines_with_width(&rep, /* expanded */ true, false, 80);
    assert!(
        lines.iter().any(|l| line_text(l).contains("proposal:")),
        "expected proposal: line"
    );
    assert!(
        lines.iter().any(|l| line_text(l).contains("/compact")),
        "expected slash command in proposal"
    );
}

#[test]
fn pane_card_does_not_show_proposal_when_collapsed() {
    use crate::domain::recommendation::RequestedEffect;
    let mut rep = sample_pane_report();
    rep.effects.push(RequestedEffect::PromptSendProposed {
        target_pane_id: rep.pane_id.clone(),
        slash_command: "/compact".into(),
        proposal_id: "pid-1".into(),
    });
    let lines = pane_list_lines_with_width(&rep, /* expanded */ false, false, 80);
    assert!(
        !lines.iter().any(|l| line_text(l).contains("proposal:")),
        "proposal line should only appear when expanded"
    );
}

#[test]
fn collapsed_pane_with_pending_proposal_does_not_shift_hover_topics() {
    use crate::domain::recommendation::RequestedEffect;
    use crate::ui::help_glossary::HelpTopic;

    let mut first = sample_pane_report();
    first.effects.push(RequestedEffect::PromptSendProposed {
        target_pane_id: first.pane_id.clone(),
        slash_command: "/compact".into(),
        proposal_id: "pid-1".into(),
    });

    let mut second = sample_pane_report();
    second.pane_id = "%2".into();
    second.identity.identity.pane_id = "%2".into();
    second.current_command = "codex".into();

    let reports = vec![first, second];
    let mut state = ListState::default();
    state.select(Some(1));
    let wrap_width = 80;

    let first_height =
        pane_list_lines_with_width(&reports[0], false, true, wrap_width).len() as u16;

    assert_eq!(
        pane_help_topic_at_row(&reports, &state, first_height, wrap_width),
        Some(HelpTopic::PaneHeader),
        "row after first collapsed pane should start second pane header"
    );
}

#[test]
fn active_pane_shows_persistent_active_pill() {
    // S3b: a non-idle pane shows `● ACTIVE` even without a state-change flash
    // (every pane shows its state at a glance, per the mockup).
    let mut report = sample_pane_report();
    report.idle_state = None;
    let text = line_text(&pane_panel_title_line(&report, Instant::now(), None));
    assert!(
        text.contains("● ACTIVE"),
        "active pane must show a persistent ● ACTIVE pill (no flash): {text:?}"
    );
}

#[test]
fn wait_input_state_carries_at_a_glance_glyph() {
    // S3b: idle/wait states lead with a glyph in the title pill.
    let mut report = sample_pane_report();
    report.idle_state = Some(crate::domain::signal::IdleCause::InputWait);
    let text = line_text(&pane_panel_title_line(&report, Instant::now(), None));
    assert!(
        text.contains("⌛ WAIT INPUT"),
        "wait-input state must carry the ⌛ glyph: {text:?}"
    );
}

#[test]
fn pane_card_title_carries_pending_proposal_chip() {
    // v1.39 surface A: pane card title must carry a `★p` chip
    // when the pane has a pending PromptSendProposed effect, so
    // operators can spot actionable items without selecting each
    // pane. Severity-colored from the highest-severity rec.
    let mut rep = sample_pane_report();
    rep.effects.push(
        crate::domain::recommendation::RequestedEffect::PromptSendProposed {
            target_pane_id: rep.pane_id.clone(),
            slash_command: "/compact".into(),
            proposal_id: "pid-1".into(),
        },
    );
    let lines = pane_list_lines_with_width(&rep, true, false, 80);
    let title_line = &lines[0];
    let dump: String = title_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(
        dump.contains("\u{2605}p"),
        "title should carry ★p chip when proposal pending; got: {dump}"
    );
}

#[test]
fn pane_card_title_omits_chip_when_no_proposal() {
    // v1.39 surface A: no proposal => no chip. Lock the gating
    // so a stray Notify or non-prompt-send effect can never light
    // up the chip.
    let rep = sample_pane_report();
    let lines = pane_list_lines_with_width(&rep, true, false, 80);
    let title_line = &lines[0];
    let dump: String = title_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(
        !dump.contains("\u{2605}p"),
        "no proposal => no chip; got: {dump}"
    );
}

#[test]
fn format_resets_eta_renders_hours_and_minutes_under_24h() {
    // 2h 13m → "2h13m" (existing behavior, unchanged)
    let ts = future_unix_seconds(std::time::Duration::from_secs(2 * 3600 + 13 * 60));
    let out = format_resets_eta(ts).expect("eta should render");
    assert_eq!(out, "2h13m", "got: {out}");
}

#[test]
fn format_resets_eta_renders_minutes_only_under_1h() {
    // 45m → "45m"
    let ts = future_unix_seconds(std::time::Duration::from_secs(45 * 60));
    let out = format_resets_eta(ts).expect("eta should render");
    assert_eq!(out, "45m", "got: {out}");
}

#[test]
fn format_resets_eta_renders_seconds_only_under_1m() {
    // 30s → "30s"
    let ts = future_unix_seconds(std::time::Duration::from_secs(30));
    let out = format_resets_eta(ts).expect("eta should render");
    assert_eq!(out, "30s", "got: {out}");
}

#[test]
fn format_resets_eta_renders_days_for_long_etas() {
    // 4 days 6 hours from now → "4d 6h" (was "102h00m" pre-v1.38 polish).
    let ts = future_unix_seconds(std::time::Duration::from_secs(4 * 24 * 3600 + 6 * 3600));
    let out = format_resets_eta(ts).expect("eta should render");
    assert_eq!(out, "4d 6h", "got: {out}");
}

#[test]
fn format_resets_eta_renders_days_at_exactly_24h_boundary() {
    // 24h flat → "1d 0h"; verifies the >=24h branch fires at the boundary.
    let ts = future_unix_seconds(std::time::Duration::from_secs(24 * 3600));
    let out = format_resets_eta(ts).expect("eta should render");
    assert_eq!(out, "1d 0h", "got: {out}");
}

#[test]
fn format_resets_eta_caps_at_14_days() {
    // 14d + 1m is past the sentinel cap → None.
    let ts = future_unix_seconds(std::time::Duration::from_secs(14 * 24 * 3600 + 60));
    assert!(
        format_resets_eta(ts).is_none(),
        "14-day sentinel must reject"
    );
}

#[test]
fn format_resets_eta_returns_none_for_past_timestamps() {
    // A timestamp in the past → None.
    assert!(format_resets_eta(0).is_none(), "past timestamp must reject");
}

#[test]
fn path_row_appends_wt_of_suffix_when_pane_is_in_linked_worktree() {
    use crate::app::worktree_info::WorktreeRole;
    use std::path::PathBuf;

    let mut report = base_report();
    report.current_path = "/home/operator/proj-feat-x".into();
    report.worktree_role = Some(WorktreeRole::Linked {
        parent_repo_root: PathBuf::from("/home/operator/proj"),
    });

    // v3.1.2: the compact non-selected card shows only gauge bars, so assert
    // the path/worktree-suffix logic directly on path_row_value (unchanged; it
    // still feeds the path row in the expanded/selected card).
    let path_row = path_row_value(&report);

    assert!(
        path_row.contains("/proj-feat-x"),
        "primary cwd must still render, got {path_row:?}"
    );
    assert!(
        path_row.contains(" · wt of "),
        "linked worktree must append ` · wt of ` separator, got {path_row:?}"
    );
    assert!(
        path_row.contains("/proj"),
        "linked worktree must surface the parent repo root, got {path_row:?}"
    );
}

#[test]
fn path_row_renders_unchanged_for_primary_worktree() {
    use crate::app::worktree_info::WorktreeRole;

    let mut report = base_report();
    report.current_path = "/home/operator/proj".into();
    report.worktree_role = Some(WorktreeRole::Primary);

    // v3.1.2: path row lives in the expanded card now; assert path_row_value
    // (the worktree-suffix logic) directly (see sibling test).
    let path_row = path_row_value(&report);

    assert!(
        !path_row.contains(" · wt of "),
        "primary worktree must NOT add a worktree suffix, got {path_row:?}"
    );
}

#[test]
fn path_row_renders_unchanged_when_worktree_role_is_none() {
    let mut report = base_report();
    report.current_path = "/tmp/non-git-cwd".into();
    report.worktree_role = None;

    // v3.1.2: path row lives in the expanded card now; assert path_row_value
    // (the worktree-suffix logic) directly (see sibling test).
    let path_row = path_row_value(&report);

    assert!(
        !path_row.contains(" · wt of "),
        "non-git cwd must NOT add a worktree suffix, got {path_row:?}"
    );
}

#[test]
fn pane_card_renders_context_window_size_in_metrics_row() {
    // Task D-3: context window size renders next to CTX% in pane card
    // when signals.context_window_size is Some.
    let mut report = base_report();
    report.signals = SignalSet {
        context_pressure: Some(MetricValue::new(0.21, SourceKind::ProviderOfficial)),
        context_window_size: Some(MetricValue::new(
            1_000_000_u64,
            SourceKind::ProviderOfficial,
        )),
        ..SignalSet::default()
    };

    let lines = pane_list_lines_with_flash(&report, false, false, Instant::now(), None, 200);
    let rendered: String = lines.iter().map(line_text).collect::<Vec<_>>().join(" ");

    // S3: CTX renders as a gauge BAR with its value; the window size moves to
    // the bar's trailing slot ("of 1.00M") from the old `CTX 21% of 1.00M` badge.
    assert!(
        rendered.contains('█') && rendered.contains("21%"),
        "CTX must render as a gauge bar with its value, got {rendered:?}"
    );
    assert!(
        rendered.contains("of 1.00M"),
        "window size must render as '1.00M' abbreviation, got {rendered:?}"
    );
}

#[test]
fn pane_card_renders_context_without_window_size_when_absent() {
    // Task D-3: no regression when context_window_size is None
    // Should render just "CTX 21%", not "CTX 21% of ?" or similar.
    let mut report = base_report();
    report.signals = SignalSet {
        context_pressure: Some(MetricValue::new(0.21, SourceKind::ProviderOfficial)),
        context_window_size: None,
        ..SignalSet::default()
    };

    let lines = pane_list_lines_with_flash(&report, false, false, Instant::now(), None, 200);
    let rendered: String = lines.iter().map(line_text).collect::<Vec<_>>().join(" ");

    // S3: CTX gauge bar with value; no trailing window-size when absent.
    assert!(
        rendered.contains('█') && rendered.contains("21%"),
        "CTX must render as a gauge bar with its value, got {rendered:?}"
    );
    assert!(
        !rendered.contains(" of "),
        "should not contain ' of ' when window_size is None, got {rendered:?}"
    );
}

#[test]
fn pane_card_renders_agy_label_for_antigravity_provider() {
    let mut rep = base_report();
    rep.provider = Provider::Antigravity;
    rep.identity = ResolvedIdentity {
        identity: PaneIdentity {
            provider: Provider::Antigravity,
            instance: 1,
            role: Role::Research,
            pane_id: "%3".into(),
        },
        confidence: IdentityConfidence::High,
    };

    let lines = pane_list_lines(&rep, true, false);
    let rendered: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

    assert!(
        rendered.contains("agy"),
        "pane card title chip must show 'agy' for Antigravity provider; rendered:\n{rendered}"
    );
    assert!(
        !rendered.contains("Antigravity"),
        "pane card must use the short 'agy' label, not the enum variant name; rendered:\n{rendered}"
    );
}
