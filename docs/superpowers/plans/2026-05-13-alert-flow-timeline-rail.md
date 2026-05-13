# Alert Flow Timeline Rail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group related context/cache/snapshot alerts into one `FLOW Context recovery` Timeline Rail row while preserving evidence, copy behavior, hide behavior, and existing non-flow alerts.

**Architecture:** Keep policy data unchanged and add a UI-only projection in `src/ui/alerts.rs`: raw `AlertItem`s are still collected first, then transformed into `AlertEntry::Item` or `AlertEntry::Flow`. Public alert helpers switch from raw items to entries so rendering, hit testing, bulk hide, filtering, pending actions, and clipboard ledger attribution all agree on the same visible list.

**Tech Stack:** Rust, ratatui `ListItem`/`Line` rendering, existing Qmonster `Recommendation`/`PaneReport` data, cargo unit tests, markdown docs.

---

## File Structure

- Modify `src/ui/alerts.rs`
  - Add private `AlertEntry`, `AlertFlow`, `AlertFlowStep`, `AlertFlowFamily`, and `ContextRecoveryCategory`.
  - Add flow classification and projection helpers.
  - Update all visible-list helpers to use `collect_entries`.
  - Render the Timeline Rail and selected included-alert evidence.
  - Preserve existing raw `collect_items` behavior for non-flow alerts and internal source data.

- Modify `src/ui/help_glossary.rs`
  - Extend existing Alert header/detail help text in Korean and English so `FLOW` rows and included evidence are explained.

- Modify `docs/ai/UI_MANUAL.md`
  - Add a short Alerts subsection describing `FLOW`, the rail, included alerts, copy, and dismiss behavior.

- Add no new runtime modules and no new persistence tables.

---

## Task 1: Add Flow Projection Tests and Model

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Write failing projection tests**

Add these helpers and tests inside the existing `#[cfg(test)] mod tests` in `src/ui/alerts.rs`.

```rust
fn rec(
    action: &'static str,
    reason: &str,
    severity: Severity,
    source_kind: SourceKind,
    suggested_command: Option<&str>,
    next_step: Option<&str>,
) -> Recommendation {
    Recommendation {
        action,
        reason: reason.into(),
        severity,
        source_kind,
        suggested_command: suggested_command.map(str::to_string),
        side_effects: vec![],
        is_strong: false,
        next_step: next_step.map(str::to_string),
        profile: None,
    }
}

fn report_with_pane(pane_id: &str, recs: Vec<Recommendation>) -> PaneReport {
    let mut report = base_report(recs);
    report.pane_id = pane_id.into();
    report.identity.identity.pane_id = pane_id.into();
    report
}

#[test]
fn context_recovery_flow_groups_context_and_cache_on_same_pane() {
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
    let report = base_report(vec![context.clone(), cache.clone()]);
    let times = HashMap::from([
        (recommendation_key("%1", &context), "14:23:08".into()),
        (recommendation_key("%1", &cache), "14:23:12".into()),
    ]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &times,
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 1, "context/cache should collapse into one flow");
    let flow = entries[0].as_flow().expect("entry must be a flow");
    assert_eq!(flow.pane_id, "%1");
    assert_eq!(flow.severity, Severity::Warning);
    assert_eq!(flow.suggested_command.as_deref(), Some("/compact"));
    assert_eq!(
        flow.command_identity.as_ref().map(|(_, action)| action.as_str()),
        Some("context-pressure: checkpoint")
    );
    assert_eq!(flow.included.len(), 2);
    assert!(flow.title().contains("Context recovery"));
    assert!(flow.summary.contains("context pressure"));
}

#[test]
fn context_recovery_flow_requires_two_categories() {
    let context = rec(
        "context-pressure: checkpoint",
        "context near warning threshold",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        Some("press 's' to snapshot + archive first"),
    );
    let report = base_report(vec![context]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0], AlertEntry::Item(_)));
}

#[test]
fn context_recovery_flow_does_not_merge_different_panes() {
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
    let reports = vec![
        report_with_pane("%1", vec![context]),
        report_with_pane("%2", vec![cache]),
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
    assert!(entries.iter().all(|entry| matches!(entry, AlertEntry::Item(_))));
}

#[test]
fn context_recovery_flow_key_uses_stable_projected_key() {
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
    let raw_context_key = recommendation_key("%1", &context);
    let raw_cache_key = recommendation_key("%1", &cache);
    let report = base_report(vec![context.clone(), cache.clone()]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    let entry_keys: Vec<&str> = entries.iter().map(|entry| entry.key()).collect();
    assert_eq!(entry_keys.len(), 1);
    assert!(entry_keys[0].starts_with("flow|context-recovery|%1|"));
    assert!(!entry_keys.contains(&raw_context_key.as_str()));
    assert!(!entry_keys.contains(&raw_cache_key.as_str()));
}

#[test]
fn context_recovery_flow_does_not_group_hot_cache_advice() {
    let context = rec(
        "context-pressure: checkpoint",
        "context near warning threshold",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        Some("press 's' to snapshot + archive first"),
    );
    let hot_cache = rec(
        "cache: avoid /compact while cache is hot",
        "cache is still hot after a recent reset",
        Severity::Concern,
        SourceKind::Heuristic,
        None,
        None,
    );
    let report = base_report(vec![context, hot_cache]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| matches!(entry, AlertEntry::Item(_))));
}

#[test]
fn context_recovery_flow_does_not_group_cold_cache_advice() {
    let context = rec(
        "context-pressure: checkpoint",
        "context near warning threshold",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        Some("press 's' to snapshot + archive first"),
    );
    let cold_cache = rec(
        "cache: /compact is safe — cache is cold",
        "cache has cooled and compact is safe",
        Severity::Concern,
        SourceKind::Heuristic,
        None,
        None,
    );
    let report = base_report(vec![context, cold_cache]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| matches!(entry, AlertEntry::Item(_))));
}

#[test]
fn context_recovery_flow_preserves_highest_included_kind_priority() {
    let mut strong_context = rec(
        "context-pressure: checkpoint",
        "context near warning threshold",
        Severity::Warning,
        SourceKind::Estimated,
        Some("/compact"),
        Some("press 's' to snapshot + archive first"),
    );
    strong_context.is_strong = true;
    let cache = rec(
        "cache: drift detected — /compact will let cache rebuild",
        "cache hit ratio dropped from 0.72 to 0.38",
        Severity::Warning,
        SourceKind::Heuristic,
        None,
        None,
    );
    let plain = rec(
        "notify-input-wait",
        "waiting for user input",
        Severity::Warning,
        SourceKind::ProjectCanonical,
        None,
        None,
    );
    let report = base_report(vec![strong_context.clone(), cache.clone(), plain.clone()]);
    let times = HashMap::from([
        (recommendation_key("%1", &strong_context), "14:23:12".into()),
        (recommendation_key("%1", &cache), "14:23:12".into()),
        (recommendation_key("%1", &plain), "14:23:12".into()),
    ]);

    let entries = collect_entries(
        &[],
        &[report],
        &HashSet::new(),
        &times,
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 2);
    assert!(entries[0].as_flow().is_some(), "flow should sort before plain recommendation");
    assert!(matches!(entries[1], AlertEntry::Item(_)));
}
```

- [ ] **Step 2: Run the focused test and verify failure**

Run:

```bash
cargo test -q ui::alerts::tests::context_recovery_flow_groups_context_and_cache_on_same_pane
```

Expected: FAIL because `collect_entries`, `AlertEntry`, and `as_flow` do not exist.

- [ ] **Step 3: Add the private entry and flow model**

Add this code below `AlertKind` in `src/ui/alerts.rs`.

```rust
#[derive(Debug, Clone)]
enum AlertEntry {
    Item(AlertItem),
    Flow(AlertFlow),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertFlowFamily {
    ContextRecovery,
}

#[derive(Debug, Clone)]
struct AlertFlow {
    key: String,
    family: AlertFlowFamily,
    pane_id: String,
    timestamp: String,
    timestamp_sort_key: u32,
    severity: Severity,
    source_kind: SourceKind,
    color: Color,
    is_new: bool,
    hide_deadline: Option<Instant>,
    summary: String,
    steps: Vec<AlertFlowStep>,
    included: Vec<AlertItem>,
    suggested_command: Option<String>,
    command_identity: Option<(String, String)>,
}

#[derive(Debug, Clone)]
struct AlertFlowStep {
    marker: &'static str,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ContextRecoveryCategory {
    Context,
    Cache,
    CacheAnomaly,
    Snapshot,
}
```

Add these methods below the `impl AlertKind` block.

```rust
impl AlertEntry {
    fn key(&self) -> &str {
        match self {
            AlertEntry::Item(item) => &item.key,
            AlertEntry::Flow(flow) => &flow.key,
        }
    }

    fn severity(&self) -> Severity {
        match self {
            AlertEntry::Item(item) => item.severity,
            AlertEntry::Flow(flow) => flow.severity,
        }
    }

    fn timestamp_sort_key(&self) -> u32 {
        match self {
            AlertEntry::Item(item) => item.timestamp_sort_key,
            AlertEntry::Flow(flow) => flow.timestamp_sort_key,
        }
    }

    fn kind_priority(&self) -> u8 {
        match self {
            AlertEntry::Item(item) => item.kind.priority(),
            AlertEntry::Flow(_) => AlertKind::Recommendation.priority(),
        }
    }

    fn is_new(&self) -> bool {
        match self {
            AlertEntry::Item(item) => item.is_new,
            AlertEntry::Flow(flow) => flow.is_new,
        }
    }

    fn hide_deadline(&self) -> Option<Instant> {
        match self {
            AlertEntry::Item(item) => item.hide_deadline,
            AlertEntry::Flow(flow) => flow.hide_deadline,
        }
    }

    fn suggested_command(&self) -> Option<&str> {
        match self {
            AlertEntry::Item(item) => item.suggested_command.as_deref(),
            AlertEntry::Flow(flow) => flow.suggested_command.as_deref(),
        }
    }

    fn source_kind(&self) -> SourceKind {
        match self {
            AlertEntry::Item(item) => item.source_kind,
            AlertEntry::Flow(flow) => flow.source_kind,
        }
    }

    fn title(&self) -> String {
        match self {
            AlertEntry::Item(item) => item.title.clone(),
            AlertEntry::Flow(flow) => flow.title(),
        }
    }

    #[cfg(test)]
    fn as_flow(&self) -> Option<&AlertFlow> {
        match self {
            AlertEntry::Flow(flow) => Some(flow),
            AlertEntry::Item(_) => None,
        }
    }
}

impl AlertFlow {
    fn title(&self) -> String {
        let family = match self.family {
            AlertFlowFamily::ContextRecovery => "Context recovery",
        };
        format!("FLOW {family} · {} alerts · active", self.included.len())
    }
}
```

- [ ] **Step 4: Add projection helpers**

Add these functions above `collect_items`.

```rust
fn collect_entries(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<AlertEntry> {
    let items = collect_items(
        notices,
        reports,
        fresh_alerts,
        alert_times,
        hidden_until,
        now,
    );
    project_alert_entries(items, fresh_alerts, hidden_until, now)
}

fn project_alert_entries(
    items: Vec<AlertItem>,
    fresh_alerts: &HashSet<String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<AlertEntry> {
    let mut consumed = HashSet::new();
    let mut flows = Vec::new();

    let mut by_pane: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        if let Some((pane_id, _)) = parse_recommendation_key(&item.key)
            && context_recovery_category(item).is_some()
        {
            by_pane.entry(pane_id).or_default().push(idx);
        }
    }

    for (pane_id, indexes) in by_pane {
        let categories: HashSet<ContextRecoveryCategory> = indexes
            .iter()
            .filter_map(|idx| context_recovery_category(&items[*idx]))
            .collect();
        if categories.len() < 2 {
            continue;
        }
        let included: Vec<AlertItem> = indexes.iter().map(|idx| items[*idx].clone()).collect();
        let key = context_recovery_flow_key(&pane_id, &included);
        match visible_hide_deadline(hidden_until, &key, now) {
            Some(hide_deadline) => {
                for idx in indexes {
                    consumed.insert(idx);
                }
                flows.push(AlertEntry::Flow(build_context_recovery_flow(
                    key,
                    pane_id,
                    included,
                    hide_deadline,
                    fresh_alerts,
                )));
            }
            None => {
                for idx in indexes {
                    consumed.insert(idx);
                }
            }
        }
    }

    let mut entries: Vec<AlertEntry> = items
        .into_iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            if consumed.contains(&idx) {
                None
            } else {
                Some(AlertEntry::Item(item))
            }
        })
        .chain(flows)
        .collect();
    sort_entries(&mut entries);
    entries
}

fn sort_entries(entries: &mut [AlertEntry]) {
    entries.sort_by(|a, b| {
        b.severity()
            .cmp(&a.severity())
            .then_with(|| b.is_new().cmp(&a.is_new()))
            .then_with(|| b.timestamp_sort_key().cmp(&a.timestamp_sort_key()))
            .then_with(|| b.kind_priority().cmp(&a.kind_priority()))
            .then_with(|| a.key().cmp(b.key()))
    });
}
```

- [ ] **Step 5: Add classifier and builder helpers**

Add these functions below `alert_item_matches_filter`.

```rust
fn context_recovery_category(item: &AlertItem) -> Option<ContextRecoveryCategory> {
    let (_, action) = parse_recommendation_key(&item.key)?;
    if action.starts_with("context-pressure:") {
        return Some(ContextRecoveryCategory::Context);
    }
    if action == "anomaly: cache discontinuity detected" {
        return Some(ContextRecoveryCategory::CacheAnomaly);
    }
    if action.starts_with("snapshot before ") {
        return Some(ContextRecoveryCategory::Snapshot);
    }
    if action.starts_with("cache:")
        && (action.contains("/compact") || item.headline.contains("/compact"))
    {
        return Some(ContextRecoveryCategory::Cache);
    }
    None
}

fn context_recovery_flow_key(pane_id: &str, included: &[AlertItem]) -> String {
    let mut keys: Vec<&str> = included.iter().map(|item| item.key.as_str()).collect();
    keys.sort_unstable();
    format!("flow|context-recovery|{pane_id}|{}", keys.join("\u{1f}"))
}

fn build_context_recovery_flow(
    key: String,
    pane_id: String,
    included: Vec<AlertItem>,
    hide_deadline: Option<Instant>,
    fresh_alerts: &HashSet<String>,
) -> AlertFlow {
    let severity = included
        .iter()
        .map(|item| item.severity)
        .max()
        .unwrap_or(Severity::Concern);
    let source_kind = included
        .iter()
        .map(|item| item.source_kind)
        .max_by_key(|source| source_authority_rank(*source))
        .unwrap_or(SourceKind::Heuristic);
    let timestamp = included
        .iter()
        .max_by_key(|item| item.timestamp_sort_key)
        .map(|item| item.timestamp.clone())
        .unwrap_or_else(|| "--:--:--".into());
    let timestamp_sort_key = sortable_timestamp(&timestamp);
    let is_new = fresh_alerts.contains(&key) || included.iter().any(|item| item.is_new);
    let (suggested_command, command_identity) = flow_command_and_identity(&included);
    let steps = context_recovery_steps(&included, suggested_command.as_deref());

    AlertFlow {
        key,
        family: AlertFlowFamily::ContextRecovery,
        pane_id,
        timestamp,
        timestamp_sort_key,
        severity,
        source_kind,
        color: theme::severity_color(severity),
        is_new,
        hide_deadline,
        summary: context_recovery_summary(&included, suggested_command.as_deref()),
        steps,
        included,
        suggested_command,
        command_identity,
    }
}

fn source_authority_rank(source: SourceKind) -> u8 {
    match source {
        SourceKind::ProviderOfficial => 4,
        SourceKind::ProjectCanonical => 3,
        SourceKind::Heuristic => 2,
        SourceKind::Estimated => 1,
    }
}

fn flow_command_and_identity(included: &[AlertItem]) -> (Option<String>, Option<(String, String)>) {
    let preferred = included
        .iter()
        .find(|item| item.suggested_command.as_deref() == Some("/compact"))
        .or_else(|| included.iter().find(|item| item.suggested_command.is_some()));
    let Some(item) = preferred else {
        return (None, None);
    };
    (
        item.suggested_command.clone(),
        parse_recommendation_key(&item.key),
    )
}

fn context_recovery_summary(included: &[AlertItem], command: Option<&str>) -> String {
    let has_context = included
        .iter()
        .any(|item| context_recovery_category(item) == Some(ContextRecoveryCategory::Context));
    let has_cache = included.iter().any(|item| {
        matches!(
            context_recovery_category(item),
            Some(ContextRecoveryCategory::Cache | ContextRecoveryCategory::CacheAnomaly)
        )
    });
    match (has_context, has_cache, command) {
        (true, true, Some("/compact")) => {
            "context pressure led to cache drift; snapshot before compact".into()
        }
        (true, true, _) => "context pressure and cache drift share one recovery path".into(),
        (true, false, _) => "context pressure has a related recovery step".into(),
        (false, true, _) => "cache drift has a related recovery step".into(),
        (false, false, _) => "related recovery alerts share one response path".into(),
    }
}

fn context_recovery_steps(included: &[AlertItem], command: Option<&str>) -> Vec<AlertFlowStep> {
    let mut steps = Vec::new();
    if included
        .iter()
        .any(|item| context_recovery_category(item) == Some(ContextRecoveryCategory::Context))
    {
        steps.push(AlertFlowStep {
            marker: "o",
            text: "context pressure detected".into(),
        });
    }
    if included.iter().any(|item| {
        matches!(
            context_recovery_category(item),
            Some(ContextRecoveryCategory::Cache | ContextRecoveryCategory::CacheAnomaly)
        )
    }) {
        steps.push(AlertFlowStep {
            marker: "|",
            text: "cache drift/discontinuity detected".into(),
        });
    }
    if included
        .iter()
        .any(|item| context_recovery_category(item) == Some(ContextRecoveryCategory::Snapshot))
        || included.iter().any(|item| {
            item.details
                .iter()
                .any(|detail| detail.to_ascii_lowercase().contains("snapshot"))
        })
    {
        steps.push(AlertFlowStep {
            marker: "o",
            text: "snapshot before reset".into(),
        });
    }
    if let Some(cmd) = command {
        steps.push(AlertFlowStep {
            marker: "|",
            text: format!("then run {cmd}"),
        });
    }
    steps
}
```

- [ ] **Step 6: Keep live fingerprints raw until helper wiring**

Do not switch `alert_fingerprints` in Task 1. The live queue still
renders raw `AlertItem`s until Task 2, so fingerprints must remain keyed
to the raw visible rows for now. The projected flow key is covered by
the `context_recovery_flow_key_uses_stable_projected_key` test and the
actual live fingerprint switch happens atomically in Task 2.

- [ ] **Step 7: Run projection tests and commit**

Run:

```bash
cargo test -q context_recovery_flow_groups_context_and_cache_on_same_pane
cargo test -q context_recovery_flow_requires_two_categories
cargo test -q context_recovery_flow_does_not_merge_different_panes
cargo test -q context_recovery_flow_key_uses_stable_projected_key
cargo test -q context_recovery_flow_does_not_group_hot_cache_advice
cargo test -q context_recovery_flow_does_not_group_cold_cache_advice
cargo test -q context_recovery_flow_preserves_highest_included_kind_priority
```

Expected: PASS.

Commit:

```bash
git add src/ui/alerts.rs
git commit -m "feat(alerts): add context recovery flow projection"
```

---

## Task 2: Wire Entry-Based Public Helpers

**Files:**
- Modify: `src/ui/alerts.rs`

Execution note: this task must be implemented together with Task 3.
`alert_fingerprints`, visible-list helpers, hit testing, and
`render_alerts` must all switch to `AlertEntry` in the same commit.
Switching helpers before render (or render before helpers) breaks
freshness, timestamps, selection, and hide behavior.

- [ ] **Step 1: Write failing helper tests**

Add these tests to `src/ui/alerts.rs`.

```rust
#[test]
fn selected_flow_copies_compact_and_returns_source_recommendation_identity() {
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
    let mut state = ListState::default();
    state.select(Some(0));

    let cmd = selected_alert_suggested_command(
        &state,
        &[],
        &[report.clone()],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );
    let identity = selected_alert_recommendation_identity(
        &state,
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(cmd.as_deref(), Some("/compact"));
    assert_eq!(
        identity,
        Some(("%1".into(), "context-pressure: checkpoint".into()))
    );
}

#[test]
fn flow_visible_keys_and_bulk_hide_count_once() {
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
    let now = Instant::now();

    let keys = visible_alert_keys(&[], &[report.clone()], &HashMap::new(), now);
    let warning = actionable_alert_keys_for_severity(
        &[],
        &[report],
        &HashMap::new(),
        now,
        Severity::Warning,
    );

    assert_eq!(keys.len(), 1);
    assert!(keys[0].starts_with("flow|context-recovery|%1|"));
    assert_eq!(warning, keys);
}

#[test]
fn alert_filter_matches_included_alert_and_keeps_flow_visible() {
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
    assert!(alert_entry_matches_filter(&entries[0], "0.72"));
}

#[test]
fn alert_items_with_command_reports_flow_command_and_pane() {
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

    let entries = alert_items_with_command(
        &[],
        &[report],
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    );

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "/compact");
    assert_eq!(entries[0].pane_id.as_deref(), Some("%1"));
    assert!(entries[0].title.contains("FLOW Context recovery"));
}
```

- [ ] **Step 2: Run helper tests and verify failure**

Run:

```bash
cargo test -q selected_flow_copies_compact_and_returns_source_recommendation_identity
cargo test -q flow_visible_keys_and_bulk_hide_count_once
cargo test -q alert_filter_matches_included_alert_and_keeps_flow_visible
cargo test -q alert_items_with_command_reports_flow_command_and_pane
```

Expected: FAIL because public helpers still read raw `AlertItem`s.

- [ ] **Step 3: Replace helper internals with entries**

Update these functions to call `collect_entries` instead of `collect_items`:

```rust
pub fn alert_count(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> usize {
    collect_entries(notices, reports, &HashSet::new(), &HashMap::new(), hidden_until, now).len()
}

pub fn visible_alert_keys(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<String> {
    collect_entries(notices, reports, &HashSet::new(), &HashMap::new(), hidden_until, now)
        .into_iter()
        .map(|entry| entry.key().to_string())
        .collect()
}

pub fn selected_alert_suggested_command(
    state: &ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Option<String> {
    let idx = state.selected()?;
    collect_entries(notices, reports, fresh_alerts, alert_times, hidden_until, now)
        .get(idx)
        .and_then(|entry| entry.suggested_command().map(str::to_string))
}

pub fn selected_alert_recommendation_identity(
    state: &ListState,
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Option<(String, String)> {
    let idx = state.selected()?;
    let entries = collect_entries(notices, reports, fresh_alerts, alert_times, hidden_until, now);
    match entries.get(idx)? {
        AlertEntry::Item(item) => parse_recommendation_key(&item.key),
        AlertEntry::Flow(flow) => flow.command_identity.clone(),
    }
}
```

Update `selected_alert_suggested_command_meta`, `actionable_alert_keys_for_severity`,
`pending_auto_hide_count`, `copy_alert_count`, `alert_fingerprints`, and
`alert_items_with_command` with the same entry access pattern. This is
the point where live freshness/timestamp/hide keys switch to projected
flow keys, because the visible-list helpers are switching at the same
time.

Use this body for `alert_fingerprints`:

```rust
pub fn alert_fingerprints(notices: &[SystemNotice], reports: &[PaneReport]) -> HashSet<String> {
    collect_entries(
        notices,
        reports,
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        Instant::now(),
    )
    .into_iter()
    .map(|entry| entry.key().to_string())
    .collect()
}
```

Use this body for `alert_items_with_command`:

```rust
pub fn alert_items_with_command(
    notices: &[SystemNotice],
    reports: &[PaneReport],
    fresh_alerts: &HashSet<String>,
    alert_times: &HashMap<String, String>,
    hidden_until: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<CommandAlertEntry> {
    collect_entries(notices, reports, fresh_alerts, alert_times, hidden_until, now)
        .into_iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            let command = entry.suggested_command()?.to_string();
            let pane_id = match &entry {
                AlertEntry::Item(item) => item
                    .key
                    .strip_prefix("rec|")
                    .and_then(|tail| tail.split('|').next())
                    .map(str::to_string)
                    .or_else(|| {
                        item.key
                            .strip_prefix("finding|")
                            .and_then(|tail| tail.split('|').next())
                            .map(str::to_string)
                    }),
                AlertEntry::Flow(flow) => Some(flow.pane_id.clone()),
            };
            Some(CommandAlertEntry {
                alert_idx: idx,
                command,
                title: entry.title(),
                severity: entry.severity(),
                source: entry.source_kind(),
                pane_id,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Add entry filter helper**

Add this below `alert_item_matches_filter`.

```rust
fn alert_entry_matches_filter(entry: &AlertEntry, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    match entry {
        AlertEntry::Item(item) => alert_item_matches_filter(item, needle),
        AlertEntry::Flow(flow) => {
            flow.title().to_ascii_lowercase().contains(needle)
                || flow.summary.to_ascii_lowercase().contains(needle)
                || flow
                    .steps
                    .iter()
                    .any(|step| step.text.to_ascii_lowercase().contains(needle))
                || flow
                    .included
                    .iter()
                    .any(|item| alert_item_matches_filter(item, needle))
                || flow
                    .suggested_command
                    .as_deref()
                    .is_some_and(|cmd| cmd.to_ascii_lowercase().contains(needle))
        }
    }
}
```

- [ ] **Step 5: Run helper tests and commit**

Run:

```bash
cargo test -q selected_flow_copies_compact_and_returns_source_recommendation_identity
cargo test -q flow_visible_keys_and_bulk_hide_count_once
cargo test -q alert_filter_matches_included_alert_and_keeps_flow_visible
cargo test -q alert_items_with_command_reports_flow_command_and_pane
cargo test -q context_recovery_flow_key_uses_stable_projected_key
```

Expected: PASS.

Commit:

```bash
git add src/ui/alerts.rs
git commit -m "feat(alerts): wire flow entries into alert helpers"
```

---

## Task 3: Render Timeline Rail Rows

**Files:**
- Modify: `src/ui/alerts.rs`

- [ ] **Step 1: Write failing render and hit-test tests**

Add these tests to `src/ui/alerts.rs`.

```rust
#[test]
fn renders_context_recovery_flow_with_timeline_rail() {
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
    let mut state = ListState::default();
    state.select(Some(0));
    let area = Rect::new(0, 0, 96, 18);
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
    assert!(dump.contains("FLOW Context recovery"), "{dump}");
    assert!(dump.contains("summary : context pressure led to cache drift"), "{dump}");
    assert!(dump.contains("o context pressure detected"), "{dump}");
    assert!(dump.contains("| cache drift/discontinuity detected"), "{dump}");
    assert!(dump.contains("| then run /compact"), "{dump}");
    assert!(dump.contains("included: context-pressure: checkpoint"), "{dump}");
    assert!(dump.contains("copy    : `/compact`"), "{dump}");
}

#[test]
fn unselected_flow_omits_included_and_copy_detail() {
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
    let notice = SystemNotice {
        title: "startup".into(),
        body: "ready".into(),
        severity: Severity::Good,
        source_kind: SourceKind::ProjectCanonical,
    };
    let report = base_report(vec![context, cache]);
    let mut state = ListState::default();
    state.select(Some(1));
    let area = Rect::new(0, 0, 96, 18);
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
    assert!(dump.contains("FLOW Context recovery"), "{dump}");
    assert!(!dump.contains("included: context-pressure"), "{dump}");
    assert!(!dump.contains("press y to copy"), "{dump}");
}
```

- [ ] **Step 2: Run render tests and verify failure**

Run:

```bash
cargo test -q renders_context_recovery_flow_with_timeline_rail
cargo test -q unselected_flow_omits_included_and_copy_detail
```

Expected: FAIL because render paths still use `alert_item_lines`.

- [ ] **Step 3: Switch `render_alerts` to entries**

In `render_alerts`, replace the `items` local with `entries`, apply filtering via `alert_entry_matches_filter`, and build `ListItem`s with `alert_entry_list_item`.

Use this shape:

```rust
let mut entries = collect_entries(
    view.notices,
    view.reports,
    view.fresh_alerts,
    view.alert_times,
    view.hidden_until,
    view.now,
);
let needle = view.filter.map(|s| s.trim().to_ascii_lowercase());
if let Some(n) = needle.as_deref()
    && !n.is_empty()
{
    entries.retain(|entry| alert_entry_matches_filter(entry, n));
}
let new_count = entries.iter().filter(|entry| entry.is_new()).count();
let pending_hide_count = entries
    .iter()
    .filter(|entry| entry.hide_deadline().is_some())
    .count();
```

Update the list mapping:

```rust
let alert_items: Vec<ListItem<'static>> = entries
    .iter()
    .enumerate()
    .map(|(idx, entry)| alert_entry_list_item(entry, item_width, view.now, Some(idx) == selected))
    .collect();
```

- [ ] **Step 4: Add entry rendering helpers**

Add these near the current `alert_list_item` and `alert_item_lines` functions.

```rust
fn alert_entry_list_item(
    entry: &AlertEntry,
    width: usize,
    now: Instant,
    is_selected: bool,
) -> ListItem<'static> {
    match entry {
        AlertEntry::Item(item) => alert_list_item(item, width, now, is_selected),
        AlertEntry::Flow(flow) => ListItem::new(alert_flow_lines(flow, width, now, is_selected))
            .style(alert_style(flow.color, flow.is_new)),
    }
}

fn alert_entry_lines(
    entry: &AlertEntry,
    width: usize,
    now: Instant,
    is_selected: bool,
) -> Vec<Line<'static>> {
    match entry {
        AlertEntry::Item(item) => alert_item_lines(item, width, now, is_selected),
        AlertEntry::Flow(flow) => alert_flow_lines(flow, width, now, is_selected),
    }
}

fn alert_flow_lines(
    flow: &AlertFlow,
    width: usize,
    now: Instant,
    is_selected: bool,
) -> Vec<Line<'static>> {
    let prefix = format!("[{}] ", flow.timestamp);
    let continuation = continuation_prefix(&prefix);
    let mut lines = vec![flow_title_line(flow, &prefix)];
    lines.extend(
        wrap_with_prefix(
            &flow_dismiss_line_text(flow, now),
            width,
            &continuation,
            &continuation,
        )
        .into_iter()
        .map(Line::from),
    );
    lines.extend(
        wrap_with_prefix(
            &aligned_detail("summary", &flow.summary),
            width,
            &continuation,
            &continuation,
        )
        .into_iter()
        .map(Line::from),
    );
    for step in &flow.steps {
        let text = format!("  {} {}", step.marker, step.text);
        lines.extend(
            wrap_with_prefix(&text, width, &continuation, &continuation)
                .into_iter()
                .map(Line::from),
        );
    }
    if is_selected {
        for (idx, item) in flow.included.iter().enumerate() {
            let value = format!(
                "{} · {} · [{}]",
                item_action_label(item),
                flow.pane_id,
                source_kind_label(item.source_kind)
            );
            let label = if idx == 0 { "included" } else { "" };
            lines.extend(
                wrap_with_prefix(
                    &aligned_detail(label, &value),
                    width,
                    &continuation,
                    &continuation,
                )
                .into_iter()
                .map(Line::from),
            );
        }
        if let Some(cmd) = flow.suggested_command.as_deref() {
            for wrapped in wrap_with_prefix(
                &aligned_detail("copy", &format!("`{cmd}`  -> press y to copy")),
                width,
                &continuation,
                &continuation,
            ) {
                lines.push(Line::styled(wrapped, Style::default().fg(theme::text_dim())));
            }
        }
    }
    lines.push(Line::styled(
        format!(
            "{}{}",
            continuation,
            "─".repeat(width.saturating_sub(continuation.chars().count()).max(8))
        ),
        Style::default().fg(theme::text_dim()),
    ));
    lines
}

fn flow_title_line(flow: &AlertFlow, prefix: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_string())];
    if flow.is_new {
        spans.push(Span::styled(
            "NEW ",
            Style::default()
                .fg(theme::text_primary())
                .bg(theme::badge_bg())
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!(" {} ", severity_badge_text(flow.severity)),
        theme::severity_badge_style(flow.severity).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        flow.title(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    if flow.suggested_command.is_some() {
        spans.push(Span::styled(
            "  \u{2605}y",
            Style::default()
                .fg(theme::severity_color(flow.severity))
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn flow_dismiss_line_text(flow: &AlertFlow, now: Instant) -> String {
    match flow.hide_deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(now);
            let secs = remaining.as_secs().max(1);
            aligned_detail(
                "dismiss",
                &format!("[x] auto-hide flow in {secs}s · click undo · Enter/Space undo"),
            )
        }
        None => aligned_detail("dismiss", "[ ] click hide flow · Enter/Space hide"),
    }
}

fn item_action_label(item: &AlertItem) -> String {
    parse_recommendation_key(&item.key)
        .map(|(_, action)| action)
        .unwrap_or_else(|| item.title.clone())
}
```

- [ ] **Step 5: Update hit testing to use entry line heights**

Replace local `items` collections in `alert_hit_at_row`, `alert_help_topic_at_row`,
`is_alert_summary_row`, and `is_alert_copy_row` call paths with entries.

Use this in `alert_hit_at_row`:

```rust
let entries = collect_entries(
    view.notices,
    view.reports,
    view.fresh_alerts,
    view.alert_times,
    view.hidden_until,
    view.now,
);
let mut remaining = row;
let selected = state.selected();
for (idx, entry) in entries.iter().enumerate().skip(state.offset()) {
    let is_selected = Some(idx) == selected;
    let dismiss_height = entry_dismiss_line_count(entry, width, view.now) as u16;
    let height = alert_entry_lines(entry, width, view.now, is_selected).len() as u16;
    if remaining < height {
        return Some(AlertMouseHit {
            index: idx,
            dismiss: remaining > 0 && remaining < 1 + dismiss_height,
        });
    }
    remaining = remaining.saturating_sub(height);
}
None
```

Add this helper:

```rust
fn entry_dismiss_line_count(entry: &AlertEntry, width: usize, now: Instant) -> usize {
    match entry {
        AlertEntry::Item(item) => dismiss_line_count(item, width, now),
        AlertEntry::Flow(flow) => {
            let prefix = format!("[{}] ", flow.timestamp);
            let continuation = continuation_prefix(&prefix);
            wrap_with_prefix(
                &flow_dismiss_line_text(flow, now),
                width,
                &continuation,
                &continuation,
            )
            .len()
        }
    }
}
```

- [ ] **Step 6: Run render tests and commit**

Run:

```bash
cargo test -q renders_context_recovery_flow_with_timeline_rail
cargo test -q unselected_flow_omits_included_and_copy_detail
```

Expected: PASS.

Commit:

```bash
git add src/ui/alerts.rs
git commit -m "feat(alerts): render context recovery timeline rail"
```

---

## Task 4: Help Text and UI Manual

**Files:**
- Modify: `src/ui/help_glossary.rs`
- Modify: `docs/ai/UI_MANUAL.md`

- [ ] **Step 1: Write failing help test**

Add this test to `src/ui/help_glossary.rs`.

```rust
#[test]
fn alert_help_mentions_flow_rows_in_both_languages() {
    let ko = help_lines(HelpTopic::AlertHeader, HelpLanguage::Ko).join("\n");
    let en = help_lines(HelpTopic::AlertHeader, HelpLanguage::En).join("\n");

    assert!(ko.contains("FLOW"));
    assert!(ko.contains("묶"));
    assert!(en.contains("FLOW"));
    assert!(en.contains("groups related alerts"));
}
```

- [ ] **Step 2: Run help test and verify failure**

Run:

```bash
cargo test -q ui::help_glossary::tests::alert_help_mentions_flow_rows_in_both_languages
```

Expected: FAIL because current help text does not mention `FLOW`.

- [ ] **Step 3: Update Korean and English help text**

Modify the `HelpTopic::AlertHeader` and `HelpTopic::AlertDetail` arrays.

Korean `AlertHeader`:

```rust
HelpTopic::AlertHeader => &[
    "헤더: 알림 발생 시각, NEW 여부, 심각도, 제목을 한 줄에 모읍니다.",
    "제목은 Recommendation/Checkpoint/FLOW와 관련 pane ID 또는 흐름 이름을 보여줍니다.",
    "FLOW는 같은 대응 경로를 공유하는 관련 alert를 하나로 묶은 행입니다.",
    "★y 칩은 바로 실행 가능한 추천 명령이 있음을 뜻합니다.",
],
```

Korean `AlertDetail`:

```rust
HelpTopic::AlertDetail => &[
    "details: summary 아래의 next/run/anchor/others 또는 FLOW rail 같은 추가 정보입니다.",
    "FLOW rail은 신호 -> 후속 근거 -> 권장 다음 조치를 순서대로 보여줍니다.",
    "included 행은 FLOW가 묶은 원본 alert 근거입니다.",
    "profile, side_effects, lever가 있으면 설정 변경의 근거와 트레이드오프를 함께 보여줍니다.",
],
```

English `AlertHeader`:

```rust
HelpTopic::AlertHeader => &[
    "header: timestamp, NEW badge, severity, and alert title in one row.",
    "The title names the alert type, including Recommendation, Checkpoint, or FLOW.",
    "FLOW groups related alerts that share one response path.",
    "The ★y chip means the alert has a copyable command suggestion.",
],
```

English `AlertDetail`:

```rust
HelpTopic::AlertDetail => &[
    "details: extra next/run/anchor/others rows or FLOW rail rows below the summary.",
    "A FLOW rail shows signal -> follow-on evidence -> recommended next step.",
    "included rows list the original alerts used as evidence.",
    "profile, side_effects, and lever rows explain config-change tradeoffs when present.",
],
```

- [ ] **Step 4: Update UI manual**

In `docs/ai/UI_MANUAL.md`, add this subsection under `## 2. Alerts 읽는 법`, after the paragraph that lists alert title kinds.

```markdown
### Alert Flow 읽는 법

- `FLOW` 행은 서로 관련된 recommendation alert들이 하나의 대응 흐름을
  만들 때 표시됩니다. 첫 구현 범위는 context/cache/snapshot/`/compact`
  계열의 **Context recovery** 흐름입니다.
- 기본 목록에서는 원본 alert들이 중복 top-level 행으로 흩어지지 않고,
  `FLOW Context recovery · N alerts · active` 한 행으로 대표됩니다.
- `summary` 아래 rail은 `o` / `|` 접두사로 원인 신호, 후속 근거,
  선행 조치, 실행 명령을 순서대로 보여줍니다.
- 선택된 FLOW 행은 `included` 행으로 묶인 원본 alert action과 source를
  보여줍니다. 이는 flow가 어떤 근거로 만들어졌는지 확인하기 위한
  evidence입니다.
- FLOW에 실행 가능한 command가 있으면 기존 alert와 같이 제목에 `★y`가
  붙고, Alerts focus에서 `y`로 현재 대표 command를 복사합니다. Context
  recovery에서는 `/compact`가 있으면 그것을 우선 복사합니다.
- Enter/Space hide는 FLOW key를 대상으로 합니다. 같은 구성의 flow가
  숨겨진 동안 included 원본 alert들은 top-level로 다시 흩어지지 않습니다.
```

- [ ] **Step 5: Run help test and commit**

Run:

```bash
cargo test -q ui::help_glossary::tests::alert_help_mentions_flow_rows_in_both_languages
```

Expected: PASS.

Commit:

```bash
git add src/ui/help_glossary.rs docs/ai/UI_MANUAL.md
git commit -m "docs(alerts): document alert flow timeline rail"
```

---

## Task 5: End-to-End Alert Helper Regression Sweep

**Files:**
- Modify only if a listed test reveals a compile or behavior mismatch:
  - `src/ui/alerts.rs`
  - `src/ui/help_glossary.rs`
  - `docs/ai/UI_MANUAL.md`

- [ ] **Step 1: Run all alert and help unit tests**

Run:

```bash
cargo test -q ui::alerts::tests
cargo test -q ui::help_glossary::tests
```

Expected: PASS.

- [ ] **Step 2: Run clipboard ledger tests**

Run:

```bash
cargo test -q app::clipboard_actions::tests
```

Expected: PASS. This confirms `selected_alert_recommendation_identity` still feeds the copied-outcome ledger path.

- [ ] **Step 3: Run pending actions tests**

Run:

```bash
cargo test -q ui::pending_actions::tests
```

Expected: PASS. This confirms the `a` overlay still sees copyable alert entries after flow projection.

- [ ] **Step 4: Run dashboard state tests**

Run:

```bash
cargo test -q app::dashboard_state::tests
```

Expected: PASS. This confirms alert selection, hide, bulk hide, and freshness state still work with flow keys.

- [ ] **Step 5: Run formatting and full validation**

Run:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --all-targets
```

Expected:

- `cargo fmt --all --check`: exits 0.
- `cargo clippy ...`: exits 0 with no warnings promoted to errors.
- `cargo test --all-targets`: exits 0 with all tests passing.

- [ ] **Step 6: Commit final validation fixes**

If Step 5 required code changes, commit them:

```bash
git add src/ui/alerts.rs src/ui/help_glossary.rs docs/ai/UI_MANUAL.md
git commit -m "fix(alerts): stabilize alert flow regressions"
```

If Step 5 required no code changes, do not create an empty commit.

---

## Self-Review

### Spec Coverage

- Flow family and same-pane context/cache/snapshot grouping: Task 1.
- Flow key projection: Task 1.
- Live freshness/fingerprint behavior: Task 2.
- Severity/source/timestamp derivation: Task 1.
- Command copy and lifecycle attribution: Task 2.
- Visible list helpers, hide keys, bulk hide, filter, pending actions: Task 2.
- Timeline Rail rendering, selected included evidence, `★y` copy line: Task 3.
- Hover/help and UI manual documentation: Task 4.
- Regression validation across Alerts, clipboard, pending actions, dashboard state, clippy, and full tests: Task 5.

### Placeholder Scan

The plan contains no intentionally vague implementation slots. Every task names the exact file, test, command, and expected result.

### Type Consistency

The plan consistently uses:

- `AlertEntry::Item(AlertItem)` and `AlertEntry::Flow(AlertFlow)`.
- `AlertFlow.command_identity: Option<(String, String)>`.
- `collect_entries(...) -> Vec<AlertEntry>`.
- `alert_entry_matches_filter(&AlertEntry, &str) -> bool`.
- Existing `CommandAlertEntry` without a shape change.
