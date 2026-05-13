# Panes Expanded Sectioned Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reorganize the selected expanded Panes content into `NOW`, `WHERE`, `PRESSURE`, `RUNTIME`, and `RECOMMENDATIONS` sections while preserving existing pane facts and collapsed row behavior.

**Architecture:** Keep all provider, signal, and recommendation data unchanged. Add a small render-only section helper in `src/ui/panels/mod.rs` that assembles rows with their hover-help topics, then flatten it into `Line`s for the expanded list and `ListItem`s for the pane panel. Collapsed pane rows keep the existing flat renderer path.

**Tech Stack:** Rust, ratatui `Line`/`ListItem` rendering, existing `PaneReport` data, cargo unit tests, markdown docs.

---

## File Structure

- Modify `src/ui/panels/mod.rs`
  - Add `HelpTopic` import.
  - Add private `PaneRenderLine`, `PaneTokenRows`, and `PaneSectionOptions`.
  - Add helpers that assemble sectioned pane rows with matching help topics.
  - Use the section helper for selected expanded Panes list rows.
  - Keep the collapsed Panes list on its current flat path.
  - Use the section helper for `panel_body_with_width`.
  - Update `pane_list_help_topics_with_width` so hover help follows the new section rows.

- Modify `src/ui/panels/tests.rs`
  - Add tests for expanded section order, collapsed behavior, empty recommendation state, pane panel alignment, and hover-help topic mapping.
  - Keep existing wrapping and state-flash tests intact.

- Modify `docs/ai/UI_MANUAL.md`
  - Document the new expanded Panes section order under "Panes 읽는 법".

- No new provider adapters, domain data structures, persistence tables, or overlays.

---

## Task 1: Lock Expanded Panes Layout With Failing Tests

**Files:**
- Modify: `src/ui/panels/tests.rs`

- [ ] **Step 1: Add text search helpers near the existing `line_text` helper**

Add this code immediately after `fn line_text(line: &Line<'_>) -> String`.

```rust
fn line_texts(lines: &[Line<'_>]) -> Vec<String> {
    lines.iter().map(line_text).collect()
}

fn index_containing(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?} in lines: {lines:#?}"))
}
```

- [ ] **Step 2: Add failing expanded-list section tests**

Add these tests near the existing pane card rendering tests in `src/ui/panels/tests.rs`.

```rust
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
        profile: None,
    }];

    let lines = pane_list_lines_with_width(&rep, true, false, 120);
    let text = line_texts(&lines);

    let title = index_containing(&text, "qwork:1");
    let now = index_containing(&text, "NOW");
    let proposal = index_containing(&text, "proposal");
    let where_section = index_containing(&text, "WHERE");
    let path = index_containing(&text, "path    : /home/chquan/Qmonster");
    let command = index_containing(&text, "cmd     : codex");
    let pressure = index_containing(&text, "PRESSURE");
    let metrics = index_containing(&text, "metrics");
    let runtime = index_containing(&text, "RUNTIME");
    let modes = index_containing(&text, "modes");
    let recommendations = index_containing(&text, "RECOMMENDATIONS");
    let rec = index_containing(&text, "CONCERN");

    assert!(title < now, "title should precede NOW: {text:#?}");
    assert!(now < proposal, "NOW should contain proposal rows: {text:#?}");
    assert!(proposal < where_section, "NOW should precede WHERE: {text:#?}");
    assert!(where_section < path, "WHERE should contain path rows: {text:#?}");
    assert!(path < command, "path should precede command: {text:#?}");
    assert!(command < pressure, "WHERE should precede PRESSURE: {text:#?}");
    assert!(pressure < metrics, "PRESSURE should contain metrics: {text:#?}");
    assert!(metrics < runtime, "PRESSURE should precede RUNTIME: {text:#?}");
    assert!(runtime < modes, "RUNTIME should contain runtime facts: {text:#?}");
    assert!(
        modes < recommendations,
        "RUNTIME should precede RECOMMENDATIONS: {text:#?}"
    );
    assert!(
        recommendations < rec,
        "RECOMMENDATIONS should contain recommendation rows: {text:#?}"
    );
}

#[test]
fn collapsed_pane_keeps_flat_layout_without_section_headers() {
    let mut rep = sample_pane_report();
    rep.current_path = "/home/chquan/Qmonster".into();
    rep.current_command = "codex".into();
    rep.signals.cost_usd = Some(MetricValue::new(3.0, SourceKind::ProviderOfficial));
    rep.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::Sandbox,
        "workspace-write",
        SourceKind::ProviderOfficial,
    ));

    let lines = pane_list_lines_with_width(&rep, false, false, 120);
    let text = line_texts(&lines);

    for header in ["NOW", "WHERE", "PRESSURE", "RUNTIME", "RECOMMENDATIONS"] {
        assert!(
            !text.iter().any(|line| line.trim() == header),
            "collapsed pane must not render section header {header}: {text:#?}"
        );
    }
    assert!(
        text.iter()
            .any(|line| line.starts_with("path    : /home/chquan/Qmonster")),
        "collapsed pane should keep existing path row: {text:#?}"
    );
}

#[test]
fn expanded_pane_keeps_empty_recommendations_state_under_section() {
    let rep = sample_pane_report();
    let lines = pane_list_lines_with_width(&rep, true, false, 120);
    let text = line_texts(&lines);

    let recommendations = index_containing(&text, "RECOMMENDATIONS");
    let empty = index_containing(&text, "status  : no active recommendations");

    assert!(
        recommendations < empty,
        "empty recommendation state should live under RECOMMENDATIONS: {text:#?}"
    );
}
```

- [ ] **Step 3: Run the new expanded-list tests and verify they fail**

Run:

```bash
cargo test expanded_pane_uses_sectioned_single_column_order -- --nocapture
cargo test expanded_pane_keeps_empty_recommendations_state_under_section -- --nocapture
```

Expected:

- `expanded_pane_uses_sectioned_single_column_order` fails because `NOW` is missing.
- `expanded_pane_keeps_empty_recommendations_state_under_section` fails because `RECOMMENDATIONS` is missing.

---

## Task 2: Implement Sectioned Expanded List Rendering

**Files:**
- Modify: `src/ui/panels/mod.rs`
- Test: `src/ui/panels/tests.rs`

- [ ] **Step 1: Add the `HelpTopic` import**

In `src/ui/panels/mod.rs`, add this import with the other `crate::ui` imports.

```rust
use crate::ui::help_glossary::HelpTopic;
```

- [ ] **Step 2: Add section render helper types and functions**

Add this code after `fn push_topic_count<T>`.

```rust
#[derive(Clone)]
struct PaneRenderLine {
    line: Line<'static>,
    topic: Option<HelpTopic>,
}

#[derive(Clone, Copy)]
enum PaneTokenRows {
    ExpandedList,
    PanelBody,
}

#[derive(Clone, Copy)]
struct PaneSectionOptions {
    recommendation_limit: usize,
    include_proposal: bool,
    token_rows: PaneTokenRows,
    show_empty_recommendations: bool,
}

impl PaneSectionOptions {
    fn expanded_list() -> Self {
        Self {
            recommendation_limit: 3,
            include_proposal: true,
            token_rows: PaneTokenRows::ExpandedList,
            show_empty_recommendations: true,
        }
    }

    fn panel_body() -> Self {
        Self {
            recommendation_limit: 6,
            include_proposal: false,
            token_rows: PaneTokenRows::PanelBody,
            show_empty_recommendations: true,
        }
    }
}

fn pane_topic_lines(topic: HelpTopic, lines: Vec<Line<'static>>) -> Vec<PaneRenderLine> {
    lines
        .into_iter()
        .map(|line| PaneRenderLine {
            line,
            topic: Some(topic),
        })
        .collect()
}

fn pane_topic_line(topic: HelpTopic, line: Line<'static>) -> PaneRenderLine {
    PaneRenderLine {
        line,
        topic: Some(topic),
    }
}

fn pane_section_header(title: &'static str) -> Line<'static> {
    Line::styled(
        title,
        Style::default()
            .fg(theme::text_dim())
            .add_modifier(Modifier::BOLD),
    )
}

fn push_pane_section(
    out: &mut Vec<PaneRenderLine>,
    title: &'static str,
    header_topic: HelpTopic,
    rows: Vec<PaneRenderLine>,
) {
    if rows.is_empty() {
        return;
    }
    out.push(PaneRenderLine {
        line: pane_section_header(title),
        topic: Some(header_topic),
    });
    out.extend(rows);
}

fn pane_sectioned_rows(
    report: &PaneReport,
    now: Instant,
    flash: Option<&PaneStateFlash>,
    wrap_width: u16,
    options: PaneSectionOptions,
) -> Vec<PaneRenderLine> {
    let mut out = Vec::new();

    let mut now_rows = Vec::new();
    now_rows.extend(pane_topic_lines(
        HelpTopic::PaneState,
        render_pane_state_row_with_flash(report, now, flash, wrap_width),
    ));
    now_rows.extend(pane_topic_lines(
        HelpTopic::PaneSignals,
        blocking_signal_lines(&report.signals, wrap_width),
    ));
    now_rows.extend(pane_topic_lines(
        HelpTopic::PaneSignals,
        signal_badge_lines(
            "signals",
            secondary_signal_chips(&report.signals),
            wrap_width,
        ),
    ));
    if options.include_proposal
        && let Some((_target, slash)) =
            crate::app::prompt_send_actions::first_prompt_send_proposal(report)
    {
        now_rows.extend(pane_topic_lines(
            HelpTopic::PaneRecommendation,
            wrap_aligned_field(
                "proposal",
                &format!("{slash}  \u{2192} press p to accept \u{00b7} d to reject"),
                wrap_width,
            ),
        ));
    }
    push_pane_section(&mut out, "NOW", HelpTopic::PaneState, now_rows);

    let mut where_rows = Vec::new();
    where_rows.extend(pane_topic_lines(
        HelpTopic::PanePath,
        wrap_aligned_field("path", &display_path(&report.current_path), wrap_width),
    ));
    where_rows.extend(pane_topic_lines(
        HelpTopic::PaneCommand,
        wrap_aligned_field("cmd", &display_command(&report.current_command), wrap_width),
    ));
    where_rows.extend(pane_topic_lines(
        HelpTopic::PaneStatus,
        wrap_aligned_field("status", &state_summary_line(report), wrap_width),
    ));
    push_pane_section(&mut out, "WHERE", HelpTopic::PanePath, where_rows);

    let mut pressure_rows = Vec::new();
    pressure_rows.extend(pane_topic_lines(
        HelpTopic::PaneMetrics,
        metric_badge_lines(
            &report.signals,
            report.identity.identity.provider,
            wrap_width,
        ),
    ));
    if token_rows_supported(report.identity.identity.provider) {
        match options.token_rows {
            PaneTokenRows::ExpandedList => {
                pressure_rows.push(pane_topic_line(
                    HelpTopic::PaneTokens,
                    token_sparkline_status_line(&report.recent_token_samples, &report.signals),
                ));
                if let Some(line) = token_io_line(report) {
                    pressure_rows.push(pane_topic_line(HelpTopic::PaneTokens, Line::from(line)));
                }
                if let Some(line) = cache_token_io_line(report) {
                    pressure_rows.push(pane_topic_line(HelpTopic::PaneTokens, Line::from(line)));
                }
            }
            PaneTokenRows::PanelBody => {
                if let Some(line) = token_breakdown_line(report) {
                    pressure_rows.push(pane_topic_line(HelpTopic::PaneTokens, Line::from(line)));
                }
            }
        }
    }
    push_pane_section(
        &mut out,
        "PRESSURE",
        HelpTopic::PaneMetrics,
        pressure_rows,
    );

    push_pane_section(
        &mut out,
        "RUNTIME",
        HelpTopic::PaneRuntime,
        pane_topic_lines(
            HelpTopic::PaneRuntime,
            runtime_badge_lines_wrapped(&report.signals, wrap_width),
        ),
    );

    let mut recommendation_rows = Vec::new();
    for rec in report
        .recommendations
        .iter()
        .take(options.recommendation_limit)
    {
        recommendation_rows.extend(pane_topic_lines(
            HelpTopic::PaneRecommendation,
            wrap_aligned_field(severity_label(rec.severity), &rec.reason, wrap_width),
        ));
        for detail in crate::ui::alerts::recommendation_detail_lines(rec) {
            let formatted = expanded_detail_field(&detail);
            recommendation_rows.extend(pane_topic_lines(
                HelpTopic::PaneRecommendation,
                reflow_already_aligned(&formatted, wrap_width),
            ));
        }
        for line in format_profile_lines(rec) {
            let formatted = expanded_detail_field(&line);
            recommendation_rows.extend(pane_topic_lines(
                HelpTopic::PaneProfile,
                reflow_already_aligned(&formatted, wrap_width),
            ));
        }
    }
    if report.recommendations.is_empty() && options.show_empty_recommendations {
        recommendation_rows.extend(pane_topic_lines(
            HelpTopic::PaneRecommendation,
            wrap_aligned_field("status", "no active recommendations", wrap_width),
        ));
    }
    push_pane_section(
        &mut out,
        "RECOMMENDATIONS",
        HelpTopic::PaneRecommendation,
        recommendation_rows,
    );

    out
}
```

- [ ] **Step 3: Replace `pane_list_lines_with_flash` with a section-aware version**

Replace the existing `fn pane_list_lines_with_flash` body with this body.

```rust
fn pane_list_lines_with_flash(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
    now: Instant,
    flash: Option<&PaneStateFlash>,
    wrap_width: u16,
) -> Vec<Line<'static>> {
    let flash = matching_state_flash(report, now, flash);
    let mut lines = vec![pane_panel_title_line(report, now, flash)];

    if expanded {
        lines.extend(
            pane_sectioned_rows(
                report,
                now,
                flash,
                wrap_width,
                PaneSectionOptions::expanded_list(),
            )
            .into_iter()
            .map(|row| row.line),
        );
    } else {
        for row in render_pane_state_row_with_flash(report, now, flash, wrap_width) {
            lines.push(row);
        }
        lines.extend(wrap_aligned_field(
            "path",
            &display_path(&report.current_path),
            wrap_width,
        ));
        lines.extend(wrap_aligned_field(
            "cmd",
            &display_command(&report.current_command),
            wrap_width,
        ));
        lines.extend(wrap_aligned_field(
            "status",
            &state_summary_line(report),
            wrap_width,
        ));
        lines.extend(blocking_signal_lines(&report.signals, wrap_width));
        lines.extend(signal_badge_lines(
            "signals",
            secondary_signal_chips(&report.signals),
            wrap_width,
        ));
        for row in metric_badge_lines(
            &report.signals,
            report.identity.identity.provider,
            wrap_width,
        ) {
            lines.push(row);
        }
        for row in runtime_badge_lines_wrapped(&report.signals, wrap_width) {
            lines.push(row);
        }
    }

    if with_separator {
        lines.push(Line::styled(
            "────────────────────────────────────────",
            Style::default().fg(theme::text_dim()),
        ));
    }

    lines
}
```

- [ ] **Step 4: Replace `pane_list_help_topics_with_width` with matching section topics**

Replace the existing `fn pane_list_help_topics_with_width` with this version.

```rust
fn pane_list_help_topics_with_width(
    report: &PaneReport,
    expanded: bool,
    with_separator: bool,
    wrap_width: u16,
) -> Vec<Option<HelpTopic>> {
    let now = Instant::now();
    let mut topics = vec![Some(HelpTopic::PaneHeader)];

    if expanded {
        topics.extend(
            pane_sectioned_rows(
                report,
                now,
                None,
                wrap_width,
                PaneSectionOptions::expanded_list(),
            )
            .into_iter()
            .map(|row| row.topic),
        );
    } else {
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneState),
            render_pane_state_row_with_flash(report, now, None, wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PanePath),
            wrap_aligned_field("path", &display_path(&report.current_path), wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneCommand),
            wrap_aligned_field("cmd", &display_command(&report.current_command), wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneStatus),
            wrap_aligned_field("status", &state_summary_line(report), wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneSignals),
            blocking_signal_lines(&report.signals, wrap_width).len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneSignals),
            signal_badge_lines(
                "signals",
                secondary_signal_chips(&report.signals),
                wrap_width,
            )
            .len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneMetrics),
            metric_badge_lines(
                &report.signals,
                report.identity.identity.provider,
                wrap_width,
            )
            .len(),
        );
        push_topic_count(
            &mut topics,
            Some(HelpTopic::PaneRuntime),
            runtime_badge_lines_wrapped(&report.signals, wrap_width).len(),
        );
    }

    if with_separator {
        topics.push(None);
    }
    topics
}
```

- [ ] **Step 5: Run the expanded-list tests and verify they pass**

Run:

```bash
cargo test expanded_pane_uses_sectioned_single_column_order -- --nocapture
cargo test collapsed_pane_keeps_flat_layout_without_section_headers -- --nocapture
cargo test expanded_pane_keeps_empty_recommendations_state_under_section -- --nocapture
```

Expected:

- All three tests pass.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
git add src/ui/panels/mod.rs src/ui/panels/tests.rs
git commit -m "feat: section expanded pane rows"
```

Expected:

- Commit succeeds with only `src/ui/panels/mod.rs` and `src/ui/panels/tests.rs` staged.

---

## Task 3: Align Pane Panel Body With The Section Order

**Files:**
- Modify: `src/ui/panels/mod.rs`
- Modify: `src/ui/panels/tests.rs`

- [ ] **Step 1: Add a pane panel section test**

Add this test near `pane_cards_render_current_command_row`.

```rust
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
            profile: None,
        })
        .collect();

    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 24));
    render_pane_panel(Rect::new(0, 0, 120, 24), &mut buf, &rep);
    let rendered: String = (0..24)
        .map(|y| {
            (0..120)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    let where_section = rendered
        .find("WHERE")
        .unwrap_or_else(|| panic!("WHERE missing in panel: {rendered}"));
    let pressure = rendered
        .find("PRESSURE")
        .unwrap_or_else(|| panic!("PRESSURE missing in panel: {rendered}"));
    let runtime = rendered
        .find("RUNTIME")
        .unwrap_or_else(|| panic!("RUNTIME missing in panel: {rendered}"));
    let recommendations = rendered
        .find("RECOMMENDATIONS")
        .unwrap_or_else(|| panic!("RECOMMENDATIONS missing in panel: {rendered}"));

    assert!(
        where_section < pressure,
        "WHERE should precede PRESSURE in panel: {rendered}"
    );
    assert!(
        pressure < runtime,
        "PRESSURE should precede RUNTIME in panel: {rendered}"
    );
    assert!(
        runtime < recommendations,
        "RUNTIME should precede RECOMMENDATIONS in panel: {rendered}"
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
```

- [ ] **Step 2: Run the pane panel test and verify it fails**

Run:

```bash
cargo test pane_panel_uses_sectioned_order_and_keeps_top_six_recommendations -- --nocapture
```

Expected:

- Test fails because the pane panel body does not yet render section headers.

- [ ] **Step 3: Replace `panel_body_with_width`**

Replace the existing `fn panel_body_with_width` body with this body.

```rust
fn panel_body_with_width(report: &PaneReport, wrap_width: u16) -> Vec<ListItem<'static>> {
    pane_sectioned_rows(
        report,
        Instant::now(),
        None,
        wrap_width,
        PaneSectionOptions::panel_body(),
    )
    .into_iter()
    .map(|row| ListItem::new(row.line))
    .collect()
}
```

- [ ] **Step 4: Run pane panel and command-row tests**

Run:

```bash
cargo test pane_panel_uses_sectioned_order_and_keeps_top_six_recommendations -- --nocapture
cargo test pane_cards_render_current_command_row -- --nocapture
```

Expected:

- Both tests pass.
- `pane_cards_render_current_command_row` still finds the command in the selected pane panel.

- [ ] **Step 5: Commit Task 3**

Run:

```bash
git add src/ui/panels/mod.rs src/ui/panels/tests.rs
git commit -m "feat: section pane panel body"
```

Expected:

- Commit succeeds with only `src/ui/panels/mod.rs` and `src/ui/panels/tests.rs` staged.

---

## Task 4: Update Hover Help Mapping And User Documentation

**Files:**
- Modify: `src/ui/panels/tests.rs`
- Modify: `docs/ai/UI_MANUAL.md`

- [ ] **Step 1: Replace the fixed-index hover help test**

Replace `fn pane_help_topic_at_row_maps_core_rows()` in `src/ui/panels/tests.rs` with this version.

```rust
#[test]
fn pane_help_topic_at_row_maps_sectioned_rows() {
    use crate::domain::recommendation::{Recommendation, Severity};
    use crate::ui::help_glossary::HelpTopic;

    let mut rep = sample_pane_report();
    rep.current_path = "/repo".into();
    rep.current_command = "claude --dangerously-skip-permissions".into();
    rep.signals.cost_usd = Some(MetricValue::new(3.0, SourceKind::ProviderOfficial));
    rep.signals.runtime_facts.push(RuntimeFact::new(
        RuntimeFactKind::Sandbox,
        "workspace-write",
        SourceKind::ProviderOfficial,
    ));
    rep.recommendations = vec![Recommendation {
        action: "recommendation-test",
        reason: "recommendation detail".into(),
        severity: Severity::Concern,
        source_kind: SourceKind::Heuristic,
        suggested_command: None,
        side_effects: vec![],
        is_strong: false,
        next_step: None,
        profile: None,
    }];
    let reports = vec![rep];
    let mut state = ListState::default();
    state.select(Some(0));

    let lines = pane_list_lines_with_width(&reports[0], true, false, 100);
    let text = line_texts(&lines);

    let where_section = index_containing(&text, "WHERE") as u16;
    let path = index_containing(&text, "path    : /repo") as u16;
    let command = index_containing(&text, "cmd     : claude") as u16;
    let pressure = index_containing(&text, "PRESSURE") as u16;
    let metrics = index_containing(&text, "metrics") as u16;
    let runtime = index_containing(&text, "RUNTIME") as u16;
    let modes = index_containing(&text, "modes") as u16;
    let recommendations = index_containing(&text, "RECOMMENDATIONS") as u16;
    let recommendation = index_containing(&text, "CONCERN") as u16;

    assert_eq!(
        pane_help_topic_at_row(&reports, &state, where_section, 100),
        Some(HelpTopic::PanePath)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, path, 100),
        Some(HelpTopic::PanePath)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, command, 100),
        Some(HelpTopic::PaneCommand)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, pressure, 100),
        Some(HelpTopic::PaneMetrics)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, metrics, 100),
        Some(HelpTopic::PaneMetrics)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, runtime, 100),
        Some(HelpTopic::PaneRuntime)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, modes, 100),
        Some(HelpTopic::PaneRuntime)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, recommendations, 100),
        Some(HelpTopic::PaneRecommendation)
    );
    assert_eq!(
        pane_help_topic_at_row(&reports, &state, recommendation, 100),
        Some(HelpTopic::PaneRecommendation)
    );
}
```

- [ ] **Step 2: Run the hover help test**

Run:

```bash
cargo test pane_help_topic_at_row_maps_sectioned_rows -- --nocapture
```

Expected:

- Test passes after Task 2's topic mapping replacement.

- [ ] **Step 3: Update `docs/ai/UI_MANUAL.md`**

Under `## 3. Panes 읽는 법`, add this bullet after the pane title example block.

```markdown
- 선택된 pane가 펼쳐진 상태에서는 같은 정보를 한 줄 목록으로 섞지 않고
  `NOW`, `WHERE`, `PRESSURE`, `RUNTIME`, `RECOMMENDATIONS` 구역으로
  나눠 보여줍니다. `NOW`는 현재 state/blocked/signals/proposal,
  `WHERE`는 path/cmd/status, `PRESSURE`는 metrics/tokens/cache,
  `RUNTIME`은 provider runtime facts, `RECOMMENDATIONS`는 추천과
  detail/profile 정보를 담습니다. 접힌 pane 행은 기존처럼 flat row로
  유지됩니다.
```

- [ ] **Step 4: Run docs and panel checks**

Run:

```bash
cargo test pane_help_topic_at_row_maps_sectioned_rows -- --nocapture
git diff --check
```

Expected:

- Cargo test passes.
- `git diff --check` exits 0.

- [ ] **Step 5: Commit Task 4**

Run:

```bash
git add src/ui/panels/tests.rs docs/ai/UI_MANUAL.md
git commit -m "docs: describe sectioned pane layout"
```

Expected:

- Commit succeeds with only `src/ui/panels/tests.rs` and `docs/ai/UI_MANUAL.md` staged.

---

## Task 5: Final Verification

**Files:**
- Verify: `src/ui/panels/mod.rs`
- Verify: `src/ui/panels/tests.rs`
- Verify: `docs/ai/UI_MANUAL.md`

- [ ] **Step 1: Run focused panel tests**

Run:

```bash
cargo test ui::panels::tests -- --nocapture
```

Expected:

- All `ui::panels::tests` pass.

- [ ] **Step 2: Run full test suite**

Run:

```bash
cargo test
```

Expected:

- Full test suite passes.

- [ ] **Step 3: Check formatting and whitespace**

Run:

```bash
cargo fmt -- --check
git diff --check
```

Expected:

- `cargo fmt -- --check` exits 0.
- `git diff --check` exits 0.

- [ ] **Step 4: Review final diff**

Run:

```bash
git status --short --branch
git log --oneline --decorate -4
```

Expected:

- Branch shows the Task 2, Task 3, and Task 4 commits.
- Working tree is clean.

---

## Spec Coverage Checklist

- Selected expanded pane sections: Task 1, Task 2.
- Existing expanded facts remain visible: Task 1 asserts path, command, metrics, runtime, proposal, recommendations; Task 5 runs all panel tests.
- Collapsed Panes list unchanged: Task 1 collapsed test and Task 2 collapsed renderer path.
- Pane panel uses same section order: Task 3.
- Hover help maps to sectioned rows: Task 4.
- User manual describes new order: Task 4.
- Narrow width behavior remains covered by existing wrapping tests plus Task 5 focused panel tests.
