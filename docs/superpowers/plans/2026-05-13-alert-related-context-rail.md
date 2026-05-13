# Alert Related Context Rail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show related alert context inline while keeping every alert as its own top-level row.

**Architecture:** Keep this as a UI projection in `src/ui/alerts.rs`, after strong `FLOW` suppression and before final sorting. Add render-facing related metadata to `AlertItem`, decorate only visible recommendation items, then render `related`, `rail`, and selected `included` lines from that metadata without changing hide/copy/sort semantics.

**Tech Stack:** Rust 2021, Ratatui alert renderer, existing `AlertItem` / `AlertEntry` pipeline, `cargo test ui::alerts::tests`, `cargo clippy --all-targets --all-features -- -D warnings`.

---

## File Structure

- Modify `src/ui/alerts.rs`
  - Add `RelatedAlertContext`, `RelatedAlertStep`, `RelatedAlertEvidence`, and an internal connector helper enum near `AlertItem` / `AlertEntry`.
  - Add `related: Option<RelatedAlertContext>` to `AlertItem`.
  - Decorate visible `AlertItem`s in `project_alert_entries` after strong flows consume their source items.
  - Render related lines from `alert_item_lines`.
  - Extend filter matching to search related labels/evidence.
  - Add unit tests in the existing `#[cfg(test)] mod tests`.
- Modify `docs/ai/UI_MANUAL.md`
  - Document that weak relationships show as `related` / `rail` lines, while `FLOW` remains reserved for strong grouping.

No new module split in this plan. `src/ui/alerts.rs` is large, but alert flow/projection/render tests already live together there; splitting would add unrelated movement.

---

### Task 1: Add Failing Projection Tests

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Add test helper accessors near the start of `mod tests`**

Add these helpers after `report_with_pane`:

```rust
fn item_related(entry: &AlertEntry) -> Option<&RelatedAlertContext> {
    match entry {
        AlertEntry::Item(item) => item.related.as_ref(),
        AlertEntry::Flow(_) => None,
    }
}

fn entry_title(entry: &AlertEntry) -> String {
    entry.title()
}
```

- [ ] **Step 2: Add a failing test for same-pane same-command related alerts**

Add this test after `context_recovery_flow_requires_two_categories`:

```rust
#[test]
fn related_context_links_same_pane_same_command_without_collapsing() {
    let compact_quota = rec(
        "quota-pressure: compact soon",
        "quota pressure can recover after compact",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let compact_profile = rec(
        "profile-switch: compact profile",
        "profile switch should happen after compact",
        Severity::Concern,
        SourceKind::ProjectCanonical,
        Some("/compact"),
        None,
    );
    let report = base_report(vec![compact_quota.clone(), compact_profile.clone()]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 2, "weak related context must not collapse into FLOW");
    assert!(entries.iter().all(|entry| matches!(entry, AlertEntry::Item(_))));

    let related: Vec<&RelatedAlertContext> =
        entries.iter().filter_map(item_related).collect();
    assert_eq!(related.len(), 2, "both visible siblings should carry related context");
    assert!(related.iter().all(|ctx| ctx.total == 2));
    assert!(related.iter().all(|ctx| ctx.pane_id == "%1"));
    assert!(related
        .iter()
        .all(|ctx| ctx.connector_label == "same /compact path"));
}
```

- [ ] **Step 3: Add failing tests for pane isolation and strong flow precedence**

Add these tests after the same-command test:

```rust
#[test]
fn related_context_does_not_link_different_panes() {
    let first = rec(
        "quota-pressure: compact soon",
        "first pane compact",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let second = rec(
        "profile-switch: compact profile",
        "second pane compact",
        Severity::Concern,
        SourceKind::ProjectCanonical,
        Some("/compact"),
        None,
    );
    let reports = vec![
        report_with_pane("%1", vec![first]),
        report_with_pane("%2", vec![second]),
    ];

    let entries = collect_entries(
        &[],
        &reports,
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| item_related(entry).is_none()));
}

#[test]
fn related_context_does_not_decorate_items_consumed_by_flow() {
    let context = rec(
        "context-pressure: checkpoint",
        "context near warning threshold",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        Some("press 's' to snapshot + archive first"),
    );
    let cache = rec(
        "cache: drift detected — /compact will let cache rebuild",
        "cache hit ratio dropped from 0.72 to 0.38",
        Severity::Concern,
        SourceKind::Heuristic,
        None,
        None,
    );
    let report = base_report(vec![context, cache]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 1);
    assert!(entries[0].as_flow().is_some());
    assert!(item_related(&entries[0]).is_none());
}
```

- [ ] **Step 4: Run tests and verify they fail**

Run:

```bash
cargo test ui::alerts::tests::related_context -- --nocapture
```

Expected: compile failure naming `RelatedAlertContext` or `item.related`, because the production metadata does not exist yet.

- [ ] **Step 5: Commit is not expected**

Do not commit failing tests alone unless the implementation will be interrupted. Continue to Task 2.

---

### Task 2: Implement Related Context Projection

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Add related metadata types near `AlertItem`**

Insert after `AlertItem`:

```rust
#[derive(Debug, Clone)]
struct RelatedAlertContext {
    group_key: String,
    pane_id: String,
    connector_label: String,
    total: usize,
    steps: Vec<RelatedAlertStep>,
    evidence: Vec<RelatedAlertEvidence>,
}

#[derive(Debug, Clone)]
struct RelatedAlertStep {
    marker: &'static str,
    label: String,
    is_current: bool,
}

#[derive(Debug, Clone)]
struct RelatedAlertEvidence {
    action: String,
    source_label: String,
}

#[derive(Debug, Clone)]
struct RelatedConnector {
    rank: u8,
    key: String,
    label: String,
}
```

Add this field to `AlertItem`:

```rust
related: Option<RelatedAlertContext>,
```

Every `AlertItem { ... }` literal in `collect_items` and tests must get:

```rust
related: None,
```

- [ ] **Step 2: Replace the final visible-item assembly in `project_alert_entries`**

Replace the `let mut entries: Vec<AlertEntry> = items...` block with:

```rust
let mut visible_items: Vec<AlertItem> = items
    .into_iter()
    .enumerate()
    .filter_map(|(idx, item)| {
        if consumed.contains(&idx) {
            None
        } else {
            Some(item)
        }
    })
    .collect();
apply_related_contexts(&mut visible_items);

let mut entries: Vec<AlertEntry> = visible_items
    .into_iter()
    .map(AlertEntry::Item)
    .chain(flows)
    .collect();
sort_entries(&mut entries);
entries
```

- [ ] **Step 3: Add projection helpers after `project_alert_entries`**

Add:

```rust
fn apply_related_contexts(items: &mut [AlertItem]) {
    let mut groups: HashMap<String, (RelatedConnector, Vec<usize>)> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        let Some(pane_id) = recommendation_pane_id(item) else {
            continue;
        };
        for connector in related_connectors(item) {
            let group_key = format!("related|{pane_id}|{}", connector.key);
            groups
                .entry(group_key)
                .or_insert_with(|| (connector, Vec::new()))
                .1
                .push(idx);
        }
    }

    let mut candidates: Vec<(String, RelatedConnector, Vec<usize>)> = groups
        .into_iter()
        .filter_map(|(group_key, (connector, indexes))| {
            (indexes.len() >= 2).then_some((group_key, connector, indexes))
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.1.rank
            .cmp(&a.1.rank)
            .then_with(|| a.1.label.cmp(&b.1.label))
            .then_with(|| a.0.cmp(&b.0))
    });

    for (group_key, connector, indexes) in candidates {
        for idx in indexes.iter().copied() {
            if items[idx].related.is_none() {
                items[idx].related = Some(build_related_context(
                    &group_key,
                    &connector,
                    items,
                    &indexes,
                    idx,
                ));
            }
        }
    }
}

fn recommendation_pane_id(item: &AlertItem) -> Option<String> {
    parse_recommendation_key(&item.key).map(|(pane_id, _)| pane_id)
}

fn recommendation_action(item: &AlertItem) -> Option<String> {
    parse_recommendation_key(&item.key).map(|(_, action)| action)
}

fn related_connectors(item: &AlertItem) -> Vec<RelatedConnector> {
    if item.kind != AlertKind::Recommendation {
        return Vec::new();
    }
    let mut connectors = Vec::new();
    if let Some(cmd) = item.suggested_command.as_deref() {
        connectors.push(RelatedConnector {
            rank: 40,
            key: format!("cmd|{cmd}"),
            label: format!("same {cmd} path"),
        });
    }
    if let Some(category) = context_recovery_category(item) {
        connectors.push(RelatedConnector {
            rank: 30,
            key: format!("context-recovery|{:?}", category),
            label: "same pane recovery".into(),
        });
    }
    if item
        .details
        .iter()
        .any(|detail| detail.to_ascii_lowercase().contains("snapshot before"))
    {
        connectors.push(RelatedConnector {
            rank: 20,
            key: "recovery-path|snapshot-before".into(),
            label: "same pane recovery".into(),
        });
    }
    if let Some(action) = recommendation_action(item)
        && let Some(family) = action.split(':').next().map(str::trim).filter(|s| !s.is_empty())
    {
        connectors.push(RelatedConnector {
            rank: 10,
            key: format!("family|{family}"),
            label: "same action family".into(),
        });
    }
    connectors
}

fn build_related_context(
    group_key: &str,
    connector: &RelatedConnector,
    items: &[AlertItem],
    indexes: &[usize],
    current_idx: usize,
) -> RelatedAlertContext {
    let pane_id = recommendation_pane_id(&items[current_idx]).unwrap_or_default();
    let steps = indexes
        .iter()
        .copied()
        .take(4)
        .map(|idx| RelatedAlertStep {
            marker: if idx == current_idx { "o" } else { "|" },
            label: related_step_label(&items[idx]),
            is_current: idx == current_idx,
        })
        .collect();
    let evidence = indexes
        .iter()
        .copied()
        .map(|idx| RelatedAlertEvidence {
            action: recommendation_action(&items[idx]).unwrap_or_else(|| items[idx].headline.clone()),
            source_label: source_kind_label(items[idx].source_kind).to_string(),
        })
        .collect();
    RelatedAlertContext {
        group_key: group_key.to_string(),
        pane_id,
        connector_label: connector.label.clone(),
        total: indexes.len(),
        steps,
        evidence,
    }
}

fn related_step_label(item: &AlertItem) -> String {
    match context_recovery_category(item) {
        Some(ContextRecoveryCategory::Context) => "context pressure".into(),
        Some(ContextRecoveryCategory::Cache | ContextRecoveryCategory::CacheAnomaly) => {
            "cache drift".into()
        }
        Some(ContextRecoveryCategory::Snapshot) => "snapshot before reset".into(),
        None => recommendation_action(item)
            .and_then(|action| action.split(':').next().map(str::trim).map(str::to_string))
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| "related alert".into()),
    }
}
```

- [ ] **Step 4: Extend item filter matching to include related context**

At the end of `alert_item_matches_filter`, before `false`, add:

```rust
if let Some(related) = item.related.as_ref()
    && related_context_matches_filter(related, needle)
{
    return true;
}
```

Add this helper near `alert_item_matches_filter`:

```rust
fn related_context_matches_filter(related: &RelatedAlertContext, needle: &str) -> bool {
    related.group_key.to_ascii_lowercase().contains(needle)
        || related.pane_id.to_ascii_lowercase().contains(needle)
        || related.connector_label.to_ascii_lowercase().contains(needle)
        || related
            .steps
            .iter()
            .any(|step| step.label.to_ascii_lowercase().contains(needle))
        || related.evidence.iter().any(|evidence| {
            evidence.action.to_ascii_lowercase().contains(needle)
                || evidence.source_label.to_ascii_lowercase().contains(needle)
        })
}
```

- [ ] **Step 5: Run the projection tests**

Run:

```bash
cargo test ui::alerts::tests::related_context -- --nocapture
```

Expected: the three tests from Task 1 pass.

- [ ] **Step 6: Run focused alert tests**

Run:

```bash
cargo test ui::alerts::tests::context_recovery_flow -- --nocapture
```

Expected: existing flow tests still pass; strong flow behavior remains unchanged.

- [ ] **Step 7: Commit projection**

```bash
git add src/ui/alerts.rs
git commit -m "feat(alerts): project related alert context"
```

---

### Task 3: Render Related Lines And Selected Evidence

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Add related rendering helpers near `alert_item_lines`**

Add:

```rust
fn related_summary_line(related: &RelatedAlertContext) -> String {
    aligned_detail(
        "related",
        &format!(
            "{} alerts on {} · {}",
            related.total, related.pane_id, related.connector_label
        ),
    )
}

fn related_rail_line(related: &RelatedAlertContext) -> String {
    let body = related
        .steps
        .iter()
        .map(|step| {
            let marker = if step.is_current { "o" } else { step.marker };
            format!("{marker} {}", step.label)
        })
        .collect::<Vec<_>>()
        .join(" ");
    aligned_detail("rail", &body)
}

fn related_evidence_line(evidence: &RelatedAlertEvidence) -> String {
    aligned_detail(
        "included",
        &format!("{} [{}]", evidence.action, evidence.source_label),
    )
}
```

- [ ] **Step 2: Insert related rendering in `alert_item_lines`**

After rendering `item.details` and before the selected copy block, insert:

```rust
if let Some(related) = item.related.as_ref() {
    for detail in [
        related_summary_line(related),
        related_rail_line(related),
    ] {
        lines.extend(
            wrap_with_prefix(&detail, width, &continuation, &continuation)
                .into_iter()
                .map(|line| Line::styled(line, Style::default().fg(theme::text_dim()))),
        );
    }
    if is_selected {
        for evidence in &related.evidence {
            lines.extend(
                wrap_with_prefix(
                    &related_evidence_line(evidence),
                    width,
                    &continuation,
                    &continuation,
                )
                .into_iter()
                .map(|line| Line::styled(line, Style::default().fg(theme::text_dim()))),
            );
        }
    }
}
```

Leave the existing selected copy block after this new block, so `copy` stays close to the bottom and `is_alert_copy_row` remains correct.

- [ ] **Step 3: Add renderer tests**

Add this test near `renders_context_recovery_flow_with_timeline_rail`:

```rust
#[test]
fn renders_related_context_rail_without_collapsing_alerts() {
    let compact_quota = rec(
        "quota-pressure: compact soon",
        "quota pressure can recover after compact",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let compact_profile = rec(
        "profile-switch: compact profile",
        "profile switch should happen after compact",
        Severity::Concern,
        SourceKind::ProjectCanonical,
        Some("/compact"),
        None,
    );
    let report = base_report(vec![compact_quota, compact_profile]);
    let mut state = ListState::default();
    state.select(Some(0));
    let area = Rect::new(0, 0, 100, 18);
    let mut buf = Buffer::empty(area);
    let view = AlertView {
        notices: &[],
        reports: &[report],
        fresh_alerts: &HashSet::new(),
        alert_times: &HashMap::new(),
        hidden_until: &HashMap::new(),
        now: Instant::now(),
        target_label: "all",
        focused: true,
        filter: None,
    };

    render_alerts(area, &mut buf, &mut state, view);

    let dump = buffer_to_string(&buf);
    assert!(!dump.contains("FLOW Context recovery"), "{dump}");
    assert!(dump.contains("related : 2 alerts on %1 · same /compact path"), "{dump}");
    assert!(dump.contains("rail    : o quota-pressure | profile-switch"), "{dump}");
    assert!(dump.contains("included: quota-pressure: compact soon [Estimate]"), "{dump}");
    assert!(
        dump.contains("included: profile-switch: compact profile [Qmonster]"),
        "{dump}"
    );
}

#[test]
fn unselected_related_context_omits_evidence_but_keeps_rail() {
    let compact_quota = rec(
        "quota-pressure: compact soon",
        "quota pressure can recover after compact",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let compact_profile = rec(
        "profile-switch: compact profile",
        "profile switch should happen after compact",
        Severity::Concern,
        SourceKind::ProjectCanonical,
        Some("/compact"),
        None,
    );
    let notice = SystemNotice {
        title: "startup".into(),
        body: "ready".into(),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
    };
    let report = base_report(vec![compact_quota, compact_profile]);
    let mut state = ListState::default();
    state.select(Some(2));
    let area = Rect::new(0, 0, 100, 18);
    let mut buf = Buffer::empty(area);
    let view = AlertView {
        notices: &[notice],
        reports: &[report],
        fresh_alerts: &HashSet::new(),
        alert_times: &HashMap::new(),
        hidden_until: &HashMap::new(),
        now: Instant::now(),
        target_label: "all",
        focused: true,
        filter: None,
    };

    render_alerts(area, &mut buf, &mut state, view);

    let dump = buffer_to_string(&buf);
    assert!(dump.contains("related : 2 alerts on %1 · same /compact path"), "{dump}");
    assert!(dump.contains("rail    : o quota-pressure | profile-switch"), "{dump}");
    assert!(!dump.contains("included: quota-pressure"), "{dump}");
}
```

- [ ] **Step 4: Run renderer tests**

Run:

```bash
cargo test ui::alerts::tests::renders_related_context_rail_without_collapsing_alerts ui::alerts::tests::unselected_related_context_omits_evidence_but_keeps_rail -- --nocapture
```

Expected: both tests pass. If Cargo rejects multiple exact test names in this form, run:

```bash
cargo test ui::alerts::tests::renders_related_context_rail_without_collapsing_alerts -- --nocapture
cargo test ui::alerts::tests::unselected_related_context_omits_evidence_but_keeps_rail -- --nocapture
```

- [ ] **Step 5: Run all alert UI tests**

Run:

```bash
cargo test ui::alerts::tests -- --nocapture
```

Expected: all alert tests pass.

- [ ] **Step 6: Commit renderer**

```bash
git add src/ui/alerts.rs
git commit -m "feat(alerts): render related context rail"
```

---

### Task 4: Lock Lifecycle Semantics

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Add tests for sort, hide, bulk count, and item-local copy**

Add these tests near the existing flow lifecycle tests:

```rust
#[test]
fn related_context_does_not_change_sort_order_or_bulk_count() {
    let warning = rec(
        "quota-pressure: compact soon",
        "warning compact",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let concern = rec(
        "profile-switch: compact profile",
        "concern compact",
        Severity::Concern,
        SourceKind::ProjectCanonical,
        Some("/compact"),
        None,
    );
    let reports = vec![base_report(vec![concern.clone(), warning.clone()])];

    let entries = collect_entries(
        &[],
        &reports,
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );
    let warning_keys = actionable_alert_keys_for_severity(
        &[],
        &reports,
        &HashMap::new(),
        Instant::now(),
        Severity::Warning,
    );
    let concern_keys = actionable_alert_keys_for_severity(
        &[],
        &reports,
        &HashMap::new(),
        Instant::now(),
        Severity::Concern,
    );

    assert_eq!(entries.len(), 2);
    assert!(entry_title(&entries[0]).contains("Recommendation"));
    assert_eq!(entries[0].severity(), Severity::Warning);
    assert_eq!(entries[1].severity(), Severity::Concern);
    assert_eq!(warning_keys.len(), 1);
    assert_eq!(concern_keys.len(), 1);
}

#[test]
fn hiding_one_related_item_does_not_hide_sibling() {
    let warning = rec(
        "quota-pressure: compact soon",
        "warning compact",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let concern = rec(
        "profile-switch: compact profile",
        "concern compact",
        Severity::Concern,
        SourceKind::ProjectCanonical,
        Some("/compact"),
        None,
    );
    let hidden_key = recommendation_key("%1", &warning);
    let hidden = HashMap::from([(hidden_key, Instant::now() - Duration::from_secs(1))]);
    let report = base_report(vec![warning, concern]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &hidden,
        Instant::now(),
    );

    assert_eq!(entries.len(), 1);
    assert!(entry_title(&entries[0]).contains("Recommendation"));
    assert!(item_related(&entries[0]).is_none(), "single remaining sibling should not claim a group");
}

#[test]
fn related_copy_stays_item_local() {
    let compact = rec(
        "maintenance: compact first",
        "compact first",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let snapshot = rec(
        "maintenance: snapshot first",
        "snapshot first",
        Severity::Warning,
        SourceKind::ProjectCanonical,
        Some("qmonster snapshot"),
        None,
    );
    let report = base_report(vec![compact.clone(), snapshot.clone()]);
    let mut state = ListState::default();
    state.select(Some(0));

    let cmd = selected_alert_suggested_command(
        &state,
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(cmd.as_deref(), Some("/compact"));
}
```

- [ ] **Step 2: Add a filter test**

Add this test near `alert_filter_matches_included_alert_and_keeps_flow_visible`:

```rust
#[test]
fn alert_filter_matches_related_evidence_and_keeps_item_visible() {
    let compact_quota = rec(
        "quota-pressure: compact soon",
        "quota pressure can recover after compact",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        None,
    );
    let compact_profile = rec(
        "profile-switch: compact profile",
        "profile switch should happen after compact",
        Severity::Concern,
        SourceKind::ProjectCanonical,
        Some("/compact"),
        None,
    );
    let report = base_report(vec![compact_quota, compact_profile]);
    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .any(|entry| alert_entry_matches_filter(entry, "profile-switch")));
    assert!(entries
        .iter()
        .any(|entry| alert_entry_matches_filter(entry, "same /compact path")));
}
```

- [ ] **Step 3: Run lifecycle tests**

Run:

```bash
cargo test ui::alerts::tests::related_context -- --nocapture
```

Expected: all `related_context...` tests pass.

- [ ] **Step 4: Run full alert tests**

Run:

```bash
cargo test ui::alerts::tests -- --nocapture
```

Expected: all alert tests pass.

- [ ] **Step 5: Commit lifecycle tests**

If Step 1 or Step 2 required implementation adjustment, commit it:

```bash
git add src/ui/alerts.rs
git commit -m "test(alerts): lock related context lifecycle"
```

If no production change was needed and the tests were already committed in Task 3, skip this commit and note it in the final report.

---

### Task 5: Document And Final Verification

**Files:**
- Modify: `docs/ai/UI_MANUAL.md`
- Modify: `src/ui/alerts.rs` only if clippy/fmt requires it.

- [ ] **Step 1: Update UI manual**

In `docs/ai/UI_MANUAL.md`, under `### Alert Flow 읽는 법`, add this paragraph after the `FLOW` bullets:

```markdown
- `related` / `rail` 행은 FLOW로 접을 만큼 강한 관계는 아니지만 같은 pane,
  command, action family, recovery signal을 공유하는 alert들이 있을 때
  표시됩니다. 이 경우 alert들은 계속 개별 top-level 항목으로 남고,
  hide/copy/severity 정렬도 각 alert 기준으로 유지됩니다. 선택된 alert는
  `included` 행으로 관련 sibling action/source를 짧게 보여줍니다.
```

- [ ] **Step 2: Run formatting**

Run:

```bash
cargo fmt -- --check
```

Expected: no output and exit 0.

- [ ] **Step 3: Run whitespace check**

Run:

```bash
git diff --check
```

Expected: no output and exit 0.

- [ ] **Step 4: Run focused alert tests**

Run:

```bash
cargo test ui::alerts::tests -- --nocapture
```

Expected: all alert tests pass.

- [ ] **Step 5: Run full clippy gate**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: no warnings/errors and exit 0.

- [ ] **Step 6: Run full test suite**

Run:

```bash
cargo test
```

Expected: full suite passes.

- [ ] **Step 7: Commit docs/final polish**

```bash
git add docs/ai/UI_MANUAL.md src/ui/alerts.rs
git commit -m "docs: explain alert related context rail"
```

If `src/ui/alerts.rs` has no changes in this task, commit only the doc:

```bash
git add docs/ai/UI_MANUAL.md
git commit -m "docs: explain alert related context rail"
```

---

## Self-Review Checklist

- Spec goal covered: related context shows continuity without collapsing rows.
- Non-goals covered: no FLOW broadening, no policy changes, no persistence, no nested navigation.
- Rendering covered: `related`, `rail`, selected `included` evidence.
- Lifecycle covered: sort, hide, bulk count, copy behavior remain item-local.
- Tests covered: same-pane relation, pane isolation, strong-flow precedence, renderer output, filter, hide, bulk/copy.
- Verification covered: fmt, whitespace, alert tests, clippy, full tests.
