# Phase 7 v2 promotion: anomaly → Recommendation + Notify (v1.44.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote the v1.43.0 anomaly observation surface into the operator-facing recommendation + notify channels. When a v1 detector fires with `confidence = High`, the `AnomalySignal` becomes a `Recommendation` (rendered in the dashboard recommendation panel) and adds `RequestedEffect::Notify` to the per-tick effect set.

**Architecture:** Two pure additions plus one event-loop wiring block. Detector severity bump (one-line per detector via a new `severity_for(confidence)` helper) makes `Severity::Warning` emerge naturally at `confidence = High`. New pure function `promote_anomalies_to_recommendations(signals: &[AnomalySignal]) -> Vec<Recommendation>` filters by `severity ≥ Warning` and maps each kind to a static `(action, reason, next_step)` tuple. `event_loop` calls the promote function after `eval_anomalies`, extends `out.recommendations`, and pushes `RequestedEffect::Notify` when any promoted is Warning+ — mirroring the F-9b cost-budget pattern. Reference spec: `docs/superpowers/specs/2026-05-07-phase7-v2-promotion-design.md`.

**Tech Stack:** Rust 2021, `cargo test --lib`, `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`, `cargo fmt --all --check`.

---

## File Map

- `src/policy/rules/anomaly.rs` — new `severity_for` helper; four detectors swap hardcoded `Severity::Concern` for `severity_for(confidence)`; new `promote_anomalies_to_recommendations` pure function with kind→Recommendation matrix.
- `src/app/event_loop.rs` — one new block after `let anomalies = { ... eval_anomalies(...) };` (line ~489): call `promote_anomalies_to_recommendations`, extend `out.recommendations`, push `Notify` effect when any promoted is Warning+.
- `src/ui/settings.rs` — `Rules` tab: append `(promotes to Recommendation when confidence=High)` to each of the four anomaly rule rows.
- `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md` — Phase 7 v2 promotion paragraph / checkbox row / m-overlay subsection extension. `config/qmonster.example.toml` is unchanged (no new knobs).
- `package.json`, `VERSION.md`, `README.md`, `mission.yaml`, `mission-history.yaml`, `.mission/CURRENT_STATE.md`, `docs/ai/PROJECT_BRIEF.md`, `docs/ai/ARCHITECTURE.md` — release ledger sync to v1.44.0 (final task).

## Validation Gates (run after every task)

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

If any gate fails, fix before committing the task.

Baseline lib test count entering this plan: **1081** (post-v1.43.0 publication-verified). Each task notes its expected delta.

---

## Task 1: `severity_for` helper + four detector severity bumps

**Files:**

- Modify: `src/policy/rules/anomaly.rs` — add `severity_for` private helper; replace each detector's `severity: Severity::Concern,` with `severity: severity_for(confidence),`.

**Step 1: Write the failing tests**

Append to the `#[cfg(test)] mod tests` block in `src/policy/rules/anomaly.rs`:

```rust
    #[test]
    fn severity_for_high_returns_warning() {
        assert_eq!(severity_for(AnomalyConfidence::High), Severity::Warning);
    }

    #[test]
    fn severity_for_medium_low_returns_concern() {
        assert_eq!(severity_for(AnomalyConfidence::Medium), Severity::Concern);
        assert_eq!(severity_for(AnomalyConfidence::Low), Severity::Concern);
    }

    #[test]
    fn detect_identity_churn_emits_warning_when_high_confidence() {
        // 5 flips when threshold is 3 → 5 ≥ 1.5 * 3 = 4.5 → High → Warning
        let h = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_identity_churn_emits_concern_when_medium_confidence() {
        // exactly 3 flips → Medium (3 < 1.5*3=4.5)
        let h = churn_history(vec![
            ("claude", "/r"),
            ("codex", "/r"),
            ("claude", "/r"),
            ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    #[test]
    fn detect_error_burst_emits_warning_when_high_confidence() {
        // 16 errors / 20 = 0.80 ≥ 1.5 * 0.5 = 0.75 → High
        let mut bools = vec![true; 16];
        bools.extend(vec![false; 4]);
        let h = errors_history(bools);
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_error_burst_emits_concern_when_medium_confidence() {
        // 12 errors / 20 = 0.60 ≥ 0.5 but < 0.75 → Medium
        let mut bools = vec![true; 12];
        bools.extend(vec![false; 8]);
        let h = errors_history(bools);
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    #[test]
    fn detect_cache_discontinuity_emits_warning_when_high_confidence() {
        // 0.45-pp drop ≥ 1.5 * 0.30 = 0.45 → High
        let mut ratios = vec![Some(0.40); 10];
        ratios.extend(vec![Some(0.85); 10]);
        let h = cache_history(ratios, vec![false; 20]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_cache_discontinuity_emits_concern_when_medium_confidence() {
        // 0.34-pp drop ≥ 0.30 but < 0.45 → Medium
        let mut ratios = vec![Some(0.51); 10];
        ratios.extend(vec![Some(0.85); 10]);
        let h = cache_history(ratios, vec![false; 20]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }

    #[test]
    fn detect_cross_pane_edit_cluster_emits_warning_when_high_confidence() {
        // 6 findings with min=3 → 6 ≥ 2*3 → High
        let mut ticks: Vec<Vec<&str>> = vec![vec!["/r/foo.rs"]; 6];
        ticks.extend(vec![vec![]; 14]);
        let h = cross_pane_history(ticks);
        let sig = detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
        assert_eq!(sig.severity, Severity::Warning);
    }

    #[test]
    fn detect_cross_pane_edit_cluster_emits_concern_when_medium_confidence() {
        // 4 findings with min=3 → 4 ≥ 3 but < 6 → Medium
        let mut ticks: Vec<Vec<&str>> = vec![vec!["/r/foo.rs"]; 4];
        ticks.extend(vec![vec![]; 16]);
        let h = cross_pane_history(ticks);
        let sig = detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::Medium);
        assert_eq!(sig.severity, Severity::Concern);
    }
```

The fixture helpers (`churn_history`, `errors_history`, `cache_history`, `cross_pane_history`) already exist in the test module from Phase 7 v1 Tasks 5-8.

**Step 2: Run tests to verify they fail**

```bash
cargo test --lib severity_for
cargo test --lib emits_warning_when_high_confidence
cargo test --lib emits_concern_when_medium_confidence
```

Expected: 10 FAIL — `severity_for` tests fail with `cannot find function 'severity_for'`; the eight detector tests fail asserting `Severity::Warning` against the current `Severity::Concern` literal that all four detectors still hardcode.

**Step 3: Add the `severity_for` helper**

In `src/policy/rules/anomaly.rs`, add the helper just below the other module-level imports (near the top, after the existing `use` block; if a `severity_for`-style helper area doesn't exist yet, insert before `detect_identity_churn`):

```rust
/// Phase 7 v2 (v1.44.0): map detector confidence to severity.
/// `High` confidence promotes to `Warning` so the
/// `promote_anomalies_to_recommendations` filter (gate
/// `severity >= Warning`) emits a `Recommendation` and triggers
/// `RequestedEffect::Notify`. Medium and Low stay at `Concern` —
/// observation only, no Recommendation, no Notify.
fn severity_for(confidence: AnomalyConfidence) -> Severity {
    match confidence {
        AnomalyConfidence::High => Severity::Warning,
        _ => Severity::Concern,
    }
}
```

**Step 4: Replace each detector's hardcoded `Severity::Concern`**

Find the four `severity: Severity::Concern,` lines (expected at approximately lines 111, 175, 255, 382 — verify with `grep -n "severity: Severity::Concern" src/policy/rules/anomaly.rs`). For each one, replace with:

```rust
        severity: severity_for(confidence),
```

The `confidence` local is already in scope at each call site (Phase 7 v1 computes it just above the `Some(AnomalySignal { ... })` literal in every detector body).

There are exactly four occurrences. Replace ALL of them. The `replace_all` flag on Edit is appropriate here, but verify by re-running grep that exactly four lines changed and no false positives remain.

**Step 5: Run tests to verify they pass**

```bash
cargo test --lib severity_for
cargo test --lib emits_warning_when_high_confidence
cargo test --lib emits_concern_when_medium_confidence
```

Expected: 10 PASS.

**Step 6: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 1091 lib tests pass (1081 + 10).

**Step 7: Commit**

```bash
git add src/policy/rules/anomaly.rs
git commit -m "phase 7 v2: severity_for helper + 4 detector severity bumps (Warning at High)"
```

---

## Task 2: `promote_anomalies_to_recommendations` pure function

**Files:**

- Modify: `src/policy/rules/anomaly.rs` — add `pub fn promote_anomalies_to_recommendations` after the four detectors / `eval_anomalies` orchestrator.

**Step 1: Write failing tests**

Append to the `#[cfg(test)] mod tests` block:

```rust
    fn make_signal(kind: AnomalyKind, confidence: AnomalyConfidence) -> AnomalySignal {
        let severity = severity_for(confidence);
        let (metric_name, before, after) = match kind {
            AnomalyKind::IdentityChurn => ("provider_path", "claude:/r".to_string(), "codex:/r".to_string()),
            AnomalyKind::ErrorBurst => ("error_rate", "0.10".to_string(), "0.80".to_string()),
            AnomalyKind::CacheDiscontinuity => ("cache_hit_ratio", "0.85".to_string(), "0.40".to_string()),
            AnomalyKind::CrossPaneEditCluster => (
                "concurrent_edit_path",
                "/r/foo.rs".to_string(),
                "6".to_string(),
            ),
        };
        AnomalySignal {
            kind,
            confidence,
            severity,
            evidence: vec![AnomalyEvidence {
                metric_name,
                before,
                after,
                sample_count: 6,
                source_kind: SourceKind::Estimated,
            }],
            window_polls: 20,
            detected_at: 1_700_000_000,
        }
    }

    #[test]
    fn promote_returns_empty_for_concern_only() {
        let signals = vec![
            make_signal(AnomalyKind::IdentityChurn, AnomalyConfidence::Medium),
            make_signal(AnomalyKind::ErrorBurst, AnomalyConfidence::Low),
        ];
        let promoted = promote_anomalies_to_recommendations(&signals);
        assert!(promoted.is_empty());
    }

    #[test]
    fn promote_emits_recommendation_for_warning_signal() {
        let signals = vec![make_signal(
            AnomalyKind::IdentityChurn,
            AnomalyConfidence::High,
        )];
        let promoted = promote_anomalies_to_recommendations(&signals);
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].action, "anomaly: identity churn detected");
        assert_eq!(promoted[0].severity, Severity::Warning);
    }

    #[test]
    fn promote_maps_each_kind_to_distinct_action() {
        let signals = vec![
            make_signal(AnomalyKind::IdentityChurn, AnomalyConfidence::High),
            make_signal(AnomalyKind::ErrorBurst, AnomalyConfidence::High),
            make_signal(AnomalyKind::CacheDiscontinuity, AnomalyConfidence::High),
            make_signal(AnomalyKind::CrossPaneEditCluster, AnomalyConfidence::High),
        ];
        let promoted = promote_anomalies_to_recommendations(&signals);
        assert_eq!(promoted.len(), 4);
        let actions: Vec<&str> = promoted.iter().map(|r| r.action).collect();
        assert!(actions.contains(&"anomaly: identity churn detected"));
        assert!(actions.contains(&"anomaly: error burst detected"));
        assert!(actions.contains(&"anomaly: cache discontinuity detected"));
        assert!(actions.contains(&"anomaly: cross-pane edit cluster detected"));
        // All four must be distinct
        let mut sorted = actions.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "all 4 actions must be distinct");
    }

    #[test]
    fn promote_recommendation_severity_matches_signal() {
        let signals = vec![make_signal(
            AnomalyKind::ErrorBurst,
            AnomalyConfidence::High,
        )];
        let promoted = promote_anomalies_to_recommendations(&signals);
        assert_eq!(promoted[0].severity, signals[0].severity);
    }

    #[test]
    fn promote_recommendation_source_kind_matches_evidence() {
        let signals = vec![make_signal(
            AnomalyKind::CacheDiscontinuity,
            AnomalyConfidence::High,
        )];
        let promoted = promote_anomalies_to_recommendations(&signals);
        assert_eq!(promoted[0].source_kind, signals[0].evidence[0].source_kind);
    }
```

**Step 2: Run tests to verify they fail**

```bash
cargo test --lib promote_
```

Expected: 5 FAIL with `cannot find function 'promote_anomalies_to_recommendations'`.

**Step 3: Implement `promote_anomalies_to_recommendations`**

In `src/policy/rules/anomaly.rs`, add the function below `eval_anomalies` (or below the four detectors and above the `#[cfg(test)] mod tests` block):

```rust
/// Phase 7 v2 (v1.44.0): promote Warning+ `AnomalySignal`s into
/// `Recommendation`s so they reach the dashboard recommendation
/// panel and (via the caller's `RequestedEffect::Notify` push)
/// the desktop notification path. Concern-severity signals are
/// observation-only and skipped. Pure over its inputs.
///
/// Each kind maps to a static `(action, reason format, next_step)`
/// triple; severity and source_kind copy from the input signal.
/// `is_strong = false`, `suggested_command = None`,
/// `profile = None`, `side_effects = vec![]`.
pub fn promote_anomalies_to_recommendations(
    signals: &[AnomalySignal],
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    for sig in signals {
        if sig.severity < Severity::Warning {
            continue;
        }
        let evidence = sig.evidence.first();
        let source_kind = evidence
            .map(|e| e.source_kind)
            .unwrap_or(SourceKind::Estimated);
        let (action, reason, next_step) = match sig.kind {
            AnomalyKind::IdentityChurn => {
                let (sample_count, before, after) = evidence
                    .map(|e| (e.sample_count, e.before.clone(), e.after.clone()))
                    .unwrap_or((0, String::new(), String::new()));
                (
                    "anomaly: identity churn detected",
                    format!(
                        "pane identity flipped {sample_count} times in {} polls — verify operator intent or pane reuse; latest flip: {before} → {after}",
                        sig.window_polls
                    ),
                    "pause provider switching; review pane assignments".to_string(),
                )
            }
            AnomalyKind::ErrorBurst => {
                let (newer, older) = evidence
                    .map(|e| (e.after.clone(), e.before.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: error burst detected",
                    format!(
                        "error rate {newer} over the most recent half of the {}-poll window (baseline {older}); confidence {}",
                        sig.window_polls,
                        sig.confidence.label()
                    ),
                    "check the recent provider output; consider profile_switch if errors persist".to_string(),
                )
            }
            AnomalyKind::CacheDiscontinuity => {
                let (before, after) = evidence
                    .map(|e| (e.before.clone(), e.after.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: cache discontinuity detected",
                    format!(
                        "cache hit ratio dropped from {before} to {after} (or F-7b cache-drift fired ≥ 2× in {} polls)",
                        sig.window_polls
                    ),
                    "press 's' to snapshot before reset; consider /compact when conversation is large".to_string(),
                )
            }
            AnomalyKind::CrossPaneEditCluster => {
                let (count, path) = evidence
                    .map(|e| (e.after.clone(), e.before.clone()))
                    .unwrap_or((String::new(), String::new()));
                (
                    "anomaly: cross-pane edit cluster detected",
                    format!(
                        "{count} concurrent file-edit findings on `{path}` in {} polls — multiple panes editing the same file",
                        sig.window_polls
                    ),
                    "coordinate edits between panes; the existing F-8 ConcurrentFileEdit findings panel lists which panes".to_string(),
                )
            }
        };
        out.push(Recommendation {
            action,
            reason,
            severity: sig.severity,
            source_kind,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: Some(next_step),
            profile: None,
        });
    }
    out
}
```

**Step 4: Run tests to verify they pass**

```bash
cargo test --lib promote_
```

Expected: 5 PASS.

**Step 5: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 1096 lib tests pass (1091 + 5).

**Step 6: Commit**

```bash
git add src/policy/rules/anomaly.rs
git commit -m "phase 7 v2: promote_anomalies_to_recommendations + kind matrix + 5 tests"
```

---

## Task 3: event_loop wiring + 2 integration tests

**Files:**

- Modify: `src/app/event_loop.rs` — insert promote+Notify block immediately after `let anomalies = { ... eval_anomalies(...) };` (around line 489).
- Modify: `tests/event_loop_integration.rs` (or wherever Phase 7 v1's two anomaly integration tests landed) — add 2 new tests.

**Step 1: Find the call-site context**

```bash
grep -n "eval_anomalies\|let anomalies" src/app/event_loop.rs | head -5
```

Phase 7 v1 wires `let anomalies = { ... eval_anomalies(...) };` near line 463-489. The promote block goes immediately after that closing brace, BEFORE `reports.push(PaneReport { ... });`.

The F-9b cost-budget Notify pattern lives at `src/app/event_loop.rs:347-357`:

```rust
let mut should_notify = false;
for alert in alerts {
    let rec = crate::policy::rules::cost_budget::recommendation_for_budget_alert(&alert);
    should_notify |= rec.severity >= Severity::Warning;
    out.recommendations.push(rec);
}
if should_notify && !out.effects.contains(&RequestedEffect::Notify) {
    out.effects.push(RequestedEffect::Notify);
}
```

Mirror that exactly.

**Step 2: Write failing integration tests**

Locate the existing event-loop integration tests Phase 7 v1 added (search `event_loop_anomalies_fire_when_enabled` or similar in `tests/event_loop_integration.rs`). Append:

```rust
#[test]
fn event_loop_promotes_warning_anomalies_to_recommendations_and_notify() {
    // High-confidence churn (5 flips when min=3 → confidence=High → severity=Warning).
    // Pre-populate ctx.anomaly_history with 5 distinct identity snapshots, then run
    // run_once_with_target with [anomaly] enabled = true. Assert:
    //   - PaneReport.recommendations contains the promoted "anomaly: identity churn detected"
    //   - PaneReport.effects contains RequestedEffect::Notify
    //   - PaneReport.anomalies still contains the underlying AnomalySignal (m overlay row)

    // Build the fixture context using the same pattern as
    // event_loop_anomalies_fire_when_enabled_and_history_full (Phase 7 v1).
    let mut config = QmonsterConfig::default();
    config.anomaly.enabled = true;
    config.anomaly.window_polls = 5;
    config.anomaly.identity_churn_min_flips = 2;
    let mut ctx = build_test_context(config);

    // Pre-populate identity_snapshots with 5 entries that produce ≥ 4 flips
    // (4 ≥ 1.5 * 2 = 3 → High confidence).
    let pane_id = "%1";
    {
        let history = ctx.anomaly_history.entry(pane_id.to_string()).or_default();
        // Push newest first, alternating provider+path (4 flips between adjacent pairs).
        history.identity_snapshots.push_front(("Codex".into(), "/r".into()));
        history.identity_snapshots.push_front(("Claude".into(), "/r".into()));
        history.identity_snapshots.push_front(("Codex".into(), "/r".into()));
        history.identity_snapshots.push_front(("Claude".into(), "/r".into()));
        history.identity_snapshots.push_front(("Codex".into(), "/r".into()));
    }

    let target = build_test_target(pane_id);
    let reports = run_once_with_target(&mut ctx, std::time::Instant::now(), Some(&target))
        .expect("run_once_with_target");

    let report = reports.iter().find(|r| r.pane_id == pane_id).expect("pane report");
    assert!(
        report
            .recommendations
            .iter()
            .any(|r| r.action == "anomaly: identity churn detected"),
        "promoted Recommendation missing: {:?}",
        report.recommendations.iter().map(|r| r.action).collect::<Vec<_>>()
    );
    assert!(
        report.effects.contains(&RequestedEffect::Notify),
        "Notify effect missing: {:?}",
        report.effects
    );
    assert!(
        !report.anomalies.is_empty(),
        "underlying AnomalySignal still present in PaneReport.anomalies"
    );
}

#[test]
fn event_loop_no_promotion_when_anomaly_disabled() {
    // Same fixture but [anomaly] enabled = false (default).
    // Assert: no promoted recommendation, no Notify, anomalies vec empty.

    let config = QmonsterConfig::default();  // anomaly.enabled = false by default
    let mut ctx = build_test_context(config);

    // Pre-populate (would be enough to fire if the gate were on).
    let pane_id = "%1";
    {
        let history = ctx.anomaly_history.entry(pane_id.to_string()).or_default();
        history.identity_snapshots.push_front(("Codex".into(), "/r".into()));
        history.identity_snapshots.push_front(("Claude".into(), "/r".into()));
        history.identity_snapshots.push_front(("Codex".into(), "/r".into()));
        history.identity_snapshots.push_front(("Claude".into(), "/r".into()));
        history.identity_snapshots.push_front(("Codex".into(), "/r".into()));
    }

    let target = build_test_target(pane_id);
    let reports = run_once_with_target(&mut ctx, std::time::Instant::now(), Some(&target))
        .expect("run_once_with_target");

    let report = reports.iter().find(|r| r.pane_id == pane_id).expect("pane report");
    assert!(
        !report
            .recommendations
            .iter()
            .any(|r| r.action.starts_with("anomaly:")),
        "no promoted Recommendation when disabled"
    );
    // Notify effect may or may not be present from other rules (e.g., cost_budget);
    // assert specifically no anomaly-driven Notify by checking no anomaly Recommendation
    // is the Notify trigger.
    assert!(
        report.anomalies.is_empty(),
        "anomalies vec should be empty when [anomaly] enabled = false"
    );
}
```

The helper functions `build_test_context` and `build_test_target` are conceptual. Read the actual Phase 7 v1 test file to find the canonical fixture pattern (which used a Codex pane with `ctx.codex_rate_limits` injection). Adapt the fixture to match.

**Step 3: Run tests to verify they fail**

```bash
cargo test --test event_loop_integration event_loop_promotes_warning
cargo test --test event_loop_integration event_loop_no_promotion
```

Expected: FAIL — promoted Recommendation missing because the wiring doesn't exist yet.

**Step 4: Insert the promote+Notify block**

In `src/app/event_loop.rs`, immediately after the closing `};` of `let anomalies = { ... eval_anomalies(...) };` (around line 489), insert:

```rust
        // Phase 7 v2 (v1.44.0): promote Warning+ anomalies to Recommendations
        // and trigger Notify, mirroring F-9b cost-budget pattern.
        // Edge-triggered dedup is already enforced by eval_anomalies, so each
        // anomaly emits its Recommendation once per active window naturally.
        let mut should_notify_anomaly = false;
        for promoted in
            crate::policy::rules::anomaly::promote_anomalies_to_recommendations(&anomalies)
        {
            should_notify_anomaly |= promoted.severity >= Severity::Warning;
            out.recommendations.push(promoted);
        }
        if should_notify_anomaly && !out.effects.contains(&RequestedEffect::Notify) {
            out.effects.push(RequestedEffect::Notify);
        }
```

`Severity` and `RequestedEffect` are already imported at the top of `event_loop.rs` (Phase 7 v1 / cost_budget paths use them).

**Step 5: Run tests to verify they pass**

```bash
cargo test --test event_loop_integration event_loop_promotes_warning
cargo test --test event_loop_integration event_loop_no_promotion
cargo test --lib
```

Expected: both new integration tests PASS; all 1096 lib tests still green.

**Step 6: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --all-targets
```

Expected: lib unchanged at 1096; integration test count grows by 2 (was 54 → 56).

**Step 7: Commit**

```bash
git add src/app/event_loop.rs tests/event_loop_integration.rs
git commit -m "phase 7 v2: wire promote_anomalies into event_loop + 2 integration tests"
```

---

## Task 4: Settings overlay rule-row annotations

**Files:**

- Modify: `src/ui/settings.rs` — append `(promotes to Recommendation when confidence=High)` to each of the four anomaly rule rows.

**Step 1: Locate the rows**

```bash
grep -n "anomaly: IdentityChurn\|anomaly: ErrorBurst\|anomaly: CacheDiscontinuity\|anomaly: CrossPaneEditCluster" src/ui/settings.rs | head -8
```

Phase 7 v1 Task 12 placed these rows around line 1895-1918 in `build_rule_body_lines`.

**Step 2: Write a failing test**

Append to the settings-overlay test module (or extend the existing `rules_tab_shows_four_anomaly_rows` test):

```rust
#[test]
fn rules_tab_anomaly_rows_show_promotion_annotation() {
    let config = QmonsterConfig::default();
    let lines = build_rule_body_lines(&config);
    let rendered = rendered_text(&lines);
    // All four anomaly rule rows should mention promotion semantics.
    let expected_phrase = "promotes to Recommendation when confidence=High";
    let count = rendered.matches(expected_phrase).count();
    assert_eq!(
        count, 4,
        "expected 4 anomaly rules with promotion annotation; got {count}: {rendered}"
    );
}
```

**Step 3: Run test to verify fail**

```bash
cargo test --lib rules_tab_anomaly_rows_show_promotion_annotation
```

Expected: FAIL — annotation is missing on all four rows.

**Step 4: Append the annotation to each row**

In `src/ui/settings.rs::build_rule_body_lines`, find each anomaly rule row (the four `rule_row(...)` calls Phase 7 v1 Task 12 added). For each, append the new phrase to the condition string. Example (the actual format string structure should be preserved):

```rust
    rule_row(
        "anomaly: IdentityChurn",
        format!(
            "fires when (provider, path) flips ≥ {} times in {} polls; promotes to Recommendation when confidence=High",
            config.anomaly.identity_churn_min_flips, config.anomaly.window_polls,
        ),
    ),
    rule_row(
        "anomaly: ErrorBurst",
        format!(
            "fires when error rate ≥ {:.0}% over {} polls; promotes to Recommendation when confidence=High",
            config.anomaly.error_burst_threshold * 100.0,
            config.anomaly.window_polls,
        ),
    ),
    rule_row(
        "anomaly: CacheDiscontinuity",
        format!(
            "fires when cache_hit_ratio drops ≥ {:.0}pp OR F-7b fires ≥ 2× in {} polls; promotes to Recommendation when confidence=High",
            config.anomaly.cache_discontinuity_drop * 100.0,
            config.anomaly.window_polls,
        ),
    ),
    rule_row(
        "anomaly: CrossPaneEditCluster",
        format!(
            "fires when ≥ {} ConcurrentFileEdit findings target the same path in {} polls; promotes to Recommendation when confidence=High",
            config.anomaly.cross_pane_cluster_min_findings, config.anomaly.window_polls,
        ),
    ),
```

If the existing strings differ slightly, preserve them and append `; promotes to Recommendation when confidence=High` to the end of each.

**Step 5: Run tests to verify pass**

```bash
cargo test --lib rules_tab_anomaly_rows_show_promotion_annotation
cargo test --lib settings
```

Expected: PASS.

**Step 6: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 1097 lib tests pass (1096 + 1).

**Step 7: Commit**

```bash
git add src/ui/settings.rs
git commit -m "phase 7 v2: Settings Rules tab — promotion annotation on 4 anomaly rows"
```

---

## Task 5: Docs sync

**Files:**

- Modify: `docs/ai/UI_MANUAL.md` — extend the v1.43.0 anomaly subsection with a "Promotion to Recommendation + Notify" paragraph.
- Modify: `docs/ai/ARCHITECTURE.md` — append a v1.44.0 paragraph below the v1.43.0 entry.
- Modify: `docs/ai/VALIDATION.md` — extend the Phase 7 v1 checklist row to mention promotion.

No `config/qmonster.example.toml` change — there are no new operator knobs.

**Step 1: `docs/ai/UI_MANUAL.md` — promotion paragraph**

Find the Phase 7 v1 anomaly subsection (the one Phase 7 v1 Task 13 added). Append a paragraph at the end of that subsection:

```markdown
### Phase 7 v2: anomaly promotion (v1.44.0)

When a v1 detector fires with `confidence = High`, the resulting
`AnomalySignal` is promoted into the dashboard recommendation
panel as a `Recommendation` and triggers a desktop notification
via `RequestedEffect::Notify` (the same path F-9b cost-budget
alerts use). The m overlay ANOMALIES row remains the underlying
observation surface — promoted signals show up in both places at
once.

Promotion criteria: detector severity is `Warning` (which happens
exactly when confidence is `High`); detector severity is `Concern`
when confidence is `Medium` or `Low`, and Concern signals are
observation-only — they appear in the m overlay row but do NOT
emit a Recommendation or fire Notify.

Operator opts in with the same `[anomaly] enabled = true` from
v1.43.0 — there are no new knobs.
```

**Step 2: `docs/ai/ARCHITECTURE.md` — v1.44.0 paragraph**

Find the v1.43.0 (Phase 7 v1) entry. Append below it:

```markdown
v1.44.0 ships **Phase 7 v2 promotion**. New
`policy::rules::anomaly::promote_anomalies_to_recommendations`
pure function maps Warning+ `AnomalySignal`s to `Recommendation`s
via a static `(action, reason, next_step)` matrix per
`AnomalyKind`. New `severity_for(confidence)` helper makes
detector severity a function of confidence
(`High → Warning`, otherwise `Concern`); the four v1 detectors
swap their hardcoded `Severity::Concern` for
`severity_for(confidence)` (one-line change each). The event
loop calls `promote_anomalies_to_recommendations` after
`eval_anomalies`, extends `out.recommendations`, and pushes
`RequestedEffect::Notify` when any promoted is Warning+ —
mirroring the F-9b cost-budget pattern at
`src/app/event_loop.rs:347-357`. No `AuditEventKind` or
`RequestedEffect` variant changes; no new config knobs; no
schema changes. Edge-triggered dedup from v1 carries through
naturally — promoted Recommendations and Notify fire once per
active window.
```

**Step 3: `docs/ai/VALIDATION.md` — extend the checklist row**

Find the Phase 7 v1 checklist row (added by v1's Task 13). Replace its body with a longer version:

```markdown
- [x] Phase 7 v1 anomaly observation (v1.43.0) + Phase 7 v2 promotion (v1.44.0):
      `[anomaly] enabled = false` by default; `enabled = true` opens the
      m overlay ANOMALIES row per pane via
      `policy::rules::anomaly::eval_anomalies` over a per-pane rolling
      `AnomalyHistory`; edge-triggered dedup keeps one emission per
      active window. v1.44.0 adds Recommendation + Notify promotion
      via `promote_anomalies_to_recommendations` for signals at
      confidence=High (Severity::Warning). No new knobs; no schema
      changes; no AuditEventKind or RequestedEffect additions.
```

**Step 4: Run validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green (docs changes don't move test counts).

**Step 5: Commit**

```bash
git add docs/ai/UI_MANUAL.md docs/ai/ARCHITECTURE.md docs/ai/VALIDATION.md
git commit -m "phase 7 v2: docs (UI_MANUAL promotion subsection, ARCHITECTURE v1.44.0 paragraph, VALIDATION row)"
```

---

## Task 6: v1.44.0 release ledger sync

**Files:**

- Modify: `package.json`, `VERSION.md`, `README.md`, `mission.yaml`, `mission-history.yaml`, `.mission/CURRENT_STATE.md`, `docs/ai/PROJECT_BRIEF.md` (Date + Phase line), `docs/ai/ARCHITECTURE.md` (Date line).

Mirror the v1.43.0 ledger-sync pattern (commit `25b7df4`). NO source-code changes.

**Step 1: Read the v1.43.0 ledger sync diff for reference**

```bash
git show 25b7df4 -- package.json VERSION.md README.md docs/ai/PROJECT_BRIEF.md docs/ai/ARCHITECTURE.md mission.yaml mission-history.yaml .mission/CURRENT_STATE.md | head -300
```

Mirror the structural pattern. Write fresh prose for Phase 7 v2 promotion.

**Step 2: `package.json`** — `1.43.0` → `1.44.0`.

**Step 3: `VERSION.md`** — bump to `1.44.0`. One-liner:
`v1.44.0 — Phase 7 v2 promotion: anomaly → Recommendation + Notify (Warning at confidence=High).`

**Step 4: `README.md`** — add v1.44.0 row.

**Step 5: `mission.yaml`**:

- Title: `"Qmonster v1.44.0 - Phase 7 v2 promotion: anomaly → Recommendation + Notify (severity_for + promote_anomalies_to_recommendations + event_loop wiring)."`
- Phase line: append `+ Phase 7 v2 promotion`.
- `execution_hints.topology`: replace v1.43.0 baseline summary with v1.44.0 baseline (Phase 7 v2 promotion now shipped).
- `execution_hints.suggested_approach`: shift from "Phase 7 v1 plan" to "v1.44.0 Phase 7 v2 promotion shipped; Phase 7 v2 detectors (CostSlope/TokenSlope/MemoryGrowth/SubagentSideEffect) is the next sub-cycle (v1.45.0)".
- `budget_hint.priority`: change to `"post-v1.44.0 Phase 7 v2 detectors plan + impl"`.
- `budget_hint.scope`: write fresh v1.44.0 scope.
- `budget_hint.followups`: drop "Phase 7 v2 promotion plan pending"; promote "Phase 7 v2 detectors (CostSlope/TokenSlope/MemoryGrowth/SubagentSideEffect)" to top followup.

**Step 6: `mission-history.yaml`** — append v1.44.0 entry. Use SHAs from `git log v1.43.0..HEAD --oneline`.

**Step 7: `.mission/CURRENT_STATE.md`** — full refresh:

- `_Last updated: 2026-05-07 (Claude, v1.44.0 release ledger sync)_`
- Mission title v1.44.0; version surfaces line v1.44.0
- Replace v1.43.0 Feature State with v1.44.0 Feature State (Phase 7 v2 promotion: severity_for, promote_anomalies_to_recommendations, event_loop wiring, Settings annotation, behavior on/off)
- Active Follow-Ups: drop "Phase 7 v2 promotion plan", promote "Phase 7 v2 detectors (4 new kinds)" to top
- Validation Baseline: lib ~1097, integration ~56

**Step 8: `docs/ai/PROJECT_BRIEF.md`** — Date and Phase lines:

- Append a 2026-05-07 v1.44.0 segment to Date line
- Phase line: append `+ Phase 7 v2 promotion` after `Phase 7 v1`

**Step 9: `docs/ai/ARCHITECTURE.md`** — Date line: append v1.44.0 segment.

**Step 10: Run final validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --all-targets
```

Expected: all green. Capture exact lib + integration counts to use in `.mission/CURRENT_STATE.md` Validation Baseline.

**Step 11: Commit and tag (annotated for consistency with v1.40 / v1.41 / v1.42 / v1.43)**

```bash
git add package.json VERSION.md README.md mission.yaml mission-history.yaml .mission/CURRENT_STATE.md docs/ai/PROJECT_BRIEF.md docs/ai/ARCHITECTURE.md
git commit -m "v1.44.0 release ledger sync"
git tag -a v1.44.0 HEAD -m "v1.44.0 — Phase 7 v2 promotion: anomaly → Recommendation + Notify"
```

DO NOT push. The user will push manually after final review.

**Step 12: Verify final state**

```bash
git log --oneline -10
git tag --list | tail -5
git for-each-ref --format='%(refname:short) %(objecttype)' refs/tags/v1.4*
```

Expected: 6 task commits + 1 release ledger sync, the v1.44.0 annotated tag pointing at the ledger sync commit.

---

## Self-review

- **Spec coverage:**
  - § 1 Goal — Tasks 1-3 deliver the promotion infrastructure end-to-end.
  - § 2 Non-goals — explicitly preserved (no new detectors, no SQLite, no `n` overlay, no AuditEventKind variants, no RequestedEffect variants, no operator-tunable thresholds).
  - § 3 Architecture — Tasks 1 (severity_for + bumps) + 2 (promote function) + 3 (event_loop wiring) deliver the three pieces.
  - § 4 Detector severity bump — Task 1.
  - § 5 Detector → Recommendation mapping — Task 2.
  - § 6 Data flow — Task 3 wiring verifies it end-to-end via integration tests.
  - § 7 Error handling — Task 2's "Concern-only input → empty Vec" + Task 3's "disabled → no promotion" tests.
  - § 8 UI / docs surfaces — Task 4 (Settings) + Task 5 (UI_MANUAL/ARCHITECTURE/VALIDATION).
  - § 9 Test plan — every prescribed test is present across Tasks 1-3.
  - § 10 Validation gates — Task 6 enforces them at release time.
  - § 11 v3 / future follow-ups — explicitly NOT implemented (deferred).

- **Placeholder scan:** No "TBD" / "TODO" / "implement later" / "fill in details" / "Similar to Task N" patterns.

- **Type consistency:**
  - `severity_for` signature `fn severity_for(c: AnomalyConfidence) -> Severity` is consistent across Task 1 (definition + helper tests + detector callers) and not referenced elsewhere.
  - `promote_anomalies_to_recommendations` signature `pub fn promote_anomalies_to_recommendations(signals: &[AnomalySignal]) -> Vec<Recommendation>` is consistent between Task 2 (definition) and Task 3 (call site).
  - Action strings in Task 2 implementation match Task 4's annotation expectations (no separate action-string scope) and Task 3's integration test assertion (`"anomaly: identity churn detected"`).

- **Plan-vs-spec deltas:** None. The plan implements every § 4-8 requirement and matches the § 9 test count exactly (~13 unit + 2 integration).

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-07-phase7-v2-promotion.md`.

Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, two-stage review per task, fast iteration. Required sub-skill: `superpowers:subagent-driven-development`.
2. **Inline Execution** — execute tasks in this session via `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
