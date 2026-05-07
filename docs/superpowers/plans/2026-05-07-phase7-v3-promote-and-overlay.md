# Phase 7 v3 (a+b) — Per-kind Promotion Gate + `n` Overlay (v1.46.0) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Phase 7 v3 (a+b): replace hardcoded `severity ≥ Warning` promotion gate with per-`AnomalyKind` `min_confidence` thresholds from `[anomaly.promote]` config, and add a session-scoped `AnomalyEventsRing` + new `n` overlay that shows recent `AnomalySignal`s with their gate decision.

**Architecture:** Two parallel data flows from the post-`eval_anomalies` signal vec inside `event_loop`: existing recommendation pipeline gets a new gate predicate (`signal.confidence >= gates.promote_min_confidence(sig.kind)`), new in-memory ring buffer (capacity 100) captures every signal with its `promoted` bool. Both happen BEFORE `deliver_effects` and the audit_event loop — same sequencing pattern as v1.44.0. New `n` overlay renders the ring chronologically (newest first) with `pane_id` column.

**Tech Stack:** Rust 2021, ratatui, crossterm, serde (toml), `VecDeque` for the ring buffer.

**Reference spec:** `docs/superpowers/specs/2026-05-07-phase7-v3-promote-and-overlay-design.md`

---

## File Structure (locked-in decomposition)

**New files:**

- `src/app/anomaly_events_ring.rs` — `AnomalyEventsRing` (bounded VecDeque, push/iter/len/is_empty, capacity 100).
- `src/app/anomaly_overlay.rs` — `handle_anomaly_overlay_key` + `handle_anomaly_overlay_mouse`.
- `src/ui/anomaly_overlay.rs` — `AnomalyOverlay` state struct + `render`.

**Modified files:**

- `src/domain/anomaly.rs` — add `AnomalyEvent` struct.
- `src/app/config.rs` — add `AnomalyPromoteConfig` sub-struct + `AnomalyConfig.promote` field.
- `src/policy/gates.rs` — 8 `anomaly_promote_*` fields + `promote_min_confidence(kind)` helper + `from_inputs` wiring + defaults.
- `src/policy/rules/anomaly.rs` — change `promote_anomalies_to_recommendations` signature + gate predicate.
- `src/app/bootstrap.rs` — add `anomaly_events_ring: AnomalyEventsRing` field to `Context<P, N>`.
- `src/app/event_loop.rs` — update `promote_anomalies_to_recommendations` call site + push to `ctx.anomaly_events_ring`.
- `src/app/mod.rs` — register `anomaly_events_ring`, `anomaly_overlay` modules.
- `src/ui/mod.rs` — register `anomaly_overlay`.
- `src/app/dashboard_render.rs` — new fields on the render-context struct; new render branch when `anomaly_overlay.is_open()`.
- `src/app/tui_loop.rs` — construct local `AnomalyOverlay`; route `n` key + scroll keys + mouse.
- `src/ui/settings.rs` — Parameters tab `[anomaly.promote]` section + Rules tab annotation update.
- `tests/event_loop_integration.rs` — new integration test.
- Docs + version surfaces (Task 13).
- Mission ledger + tag (Task 14).

**State location convention:**

- `Context<P, N>` (in `src/app/bootstrap.rs`) holds policy-engine state — including the new `anomaly_events_ring` (since it is populated inside `event_loop`).
- `tui_loop.rs::run` body holds UI overlay state as `let mut` locals — including the new `anomaly_overlay` (mirroring how `metrics_overlay` lives at `tui_loop.rs:77`). There is no `AppState` struct in this codebase.

---

## Task 1: Add `AnomalyEvent` struct

**Files:**

- Modify: `src/domain/anomaly.rs` (add struct + smoke test)

- [ ] **Step 1: Add the `AnomalyEvent` struct + import**

Edit `src/domain/anomaly.rs`. Add `use crate::domain::recommendation::Severity;` near the top (after existing `use` lines). After the existing `AnomalyEvidence` struct definition, add:

```rust
/// Phase 7 v3 (v1.46.0): record pushed into the in-memory
/// `AnomalyEventsRing` for every visible `AnomalySignal`. Carries the
/// promotion-gate result so the `n` overlay can show which signals
/// became Recommendations and which stayed at passive observation.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyEvent {
    pub timestamp: i64,
    pub pane_id: String,
    pub kind: AnomalyKind,
    pub confidence: AnomalyConfidence,
    pub severity: Severity,
    pub promoted: bool,
    pub reason: String,
}
```

- [ ] **Step 2: Add a smoke test inside the existing `#[cfg(test)] mod tests` block**

Append to the test module:

```rust
#[test]
fn anomaly_event_construction_smoke() {
    let event = AnomalyEvent {
        timestamp: 1_700_000_000,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: "cost_usd: 0.10 → 25.40".to_string(),
    };
    assert_eq!(event.timestamp, 1_700_000_000);
    assert_eq!(event.kind, AnomalyKind::CostSlope);
    assert!(event.promoted);
}
```

- [ ] **Step 3: Build + run the test**

Run: `cargo test --lib -- --exact domain::anomaly::tests::anomaly_event_construction_smoke`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 4: Run fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
Expected: no output (clean).

- [ ] **Step 5: Commit**

```bash
git add src/domain/anomaly.rs
git commit -m "phase 7 v3 (a+b): AnomalyEvent record + smoke test"
```

---

## Task 2: Add `AnomalyPromoteConfig` sub-struct

**Files:**

- Modify: `src/app/config.rs` (add sub-struct + `AnomalyConfig.promote` field + tests)

- [ ] **Step 1: Add `AnomalyPromoteConfig` definition**

Edit `src/app/config.rs`. After the existing `AnomalyConfig` struct definition (right before `impl Default for AnomalyConfig`), insert:

```rust
/// Phase 7 v3 (v1.46.0): per-`AnomalyKind` minimum-confidence
/// threshold for promoting an `AnomalySignal` to a `Recommendation`.
/// Distinct from the global `AnomalyConfig.min_confidence` (visibility
/// filter) — `[anomaly.promote]` gates promotion only.
///
/// Defaults preserve v1.45.0 behavior for the 7 kinds that today
/// promote only at High confidence; `subagent_side_effect = "medium"`
/// activates the previously-unreachable v1.45.0 promote-matrix branch
/// for `SubagentSideEffect`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnomalyPromoteConfig {
    pub identity_churn: String,
    pub error_burst: String,
    pub cache_discontinuity: String,
    pub cross_pane_edit_cluster: String,
    pub cost_slope: String,
    pub token_slope: String,
    pub memory_growth: String,
    pub subagent_side_effect: String,
}

impl Default for AnomalyPromoteConfig {
    fn default() -> Self {
        Self {
            identity_churn: "high".to_string(),
            error_burst: "high".to_string(),
            cache_discontinuity: "high".to_string(),
            cross_pane_edit_cluster: "high".to_string(),
            cost_slope: "high".to_string(),
            token_slope: "high".to_string(),
            memory_growth: "high".to_string(),
            subagent_side_effect: "medium".to_string(),
        }
    }
}
```

- [ ] **Step 2: Add `promote` field to `AnomalyConfig`**

In the `pub struct AnomalyConfig { ... }` block, after `pub memory_growth_mb: f64,` add:

```rust
    /// Phase 7 v3 (v1.46.0): per-kind promotion-gate thresholds.
    pub promote: AnomalyPromoteConfig,
```

In the `impl Default for AnomalyConfig` body, after `memory_growth_mb: 1024.0,` add:

```rust
            promote: AnomalyPromoteConfig::default(),
```

- [ ] **Step 3: Add tests at the bottom of the existing `#[cfg(test)] mod tests` block in this file**

```rust
#[test]
fn anomaly_promote_config_defaults_match_spec() {
    let p = AnomalyPromoteConfig::default();
    assert_eq!(p.identity_churn, "high");
    assert_eq!(p.error_burst, "high");
    assert_eq!(p.cache_discontinuity, "high");
    assert_eq!(p.cross_pane_edit_cluster, "high");
    assert_eq!(p.cost_slope, "high");
    assert_eq!(p.token_slope, "high");
    assert_eq!(p.memory_growth, "high");
    assert_eq!(p.subagent_side_effect, "medium");
}

#[test]
fn anomaly_config_includes_promote_default() {
    let c = AnomalyConfig::default();
    assert_eq!(c.promote.identity_churn, "high");
    assert_eq!(c.promote.subagent_side_effect, "medium");
}

#[test]
fn anomaly_config_omitting_promote_section_uses_defaults() {
    // Pre-v1.46.0 toml file (no [anomaly.promote] section) loads
    // cleanly with v1.46.0 defaults applied.
    let toml = r#"
        [anomaly]
        enabled = true
        window_polls = 20
        min_confidence = "medium"
    "#;
    let cfg: Config = toml::from_str(toml).expect("parse");
    assert_eq!(cfg.anomaly.promote.cost_slope, "high");
    assert_eq!(cfg.anomaly.promote.subagent_side_effect, "medium");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib -- --exact app::config::tests::anomaly_promote_config_defaults_match_spec app::config::tests::anomaly_config_includes_promote_default app::config::tests::anomaly_config_omitting_promote_section_uses_defaults`
Expected: 3 passed.

- [ ] **Step 5: Run full lib suite + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --lib`
Expected: clean; lib test count up by 3 from baseline.

- [ ] **Step 6: Commit**

```bash
git add src/app/config.rs
git commit -m "phase 7 v3 (a+b): AnomalyPromoteConfig +8 fields + serde defaults"
```

---

## Task 3: Extend `PolicyGates` with `anomaly_promote_*` fields + `promote_min_confidence` helper

**Files:**

- Modify: `src/policy/gates.rs` (add 8 fields, helper, defaults, `from_inputs` wiring, tests)

- [ ] **Step 1: Add 8 `anomaly_promote_*` fields to `PolicyGates`**

Edit `src/policy/gates.rs`. In the `pub struct PolicyGates { ... }` block, immediately after the existing `anomaly_*` fields (after `anomaly_memory_growth_mb: f64,` or whatever the last anomaly field is), add:

```rust
    pub anomaly_promote_identity_churn: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_error_burst: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cache_discontinuity: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cross_pane_edit_cluster: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_cost_slope: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_token_slope: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_memory_growth: crate::domain::anomaly::AnomalyConfidence,
    pub anomaly_promote_subagent_side_effect: crate::domain::anomaly::AnomalyConfidence,
```

- [ ] **Step 2: Update both `Default` blocks (`PolicyGates::default()` x 2 — at line ~173 inline default and the explicit `Default` impl near line ~386)**

In each `Default` block, after the existing anomaly defaults, add:

```rust
            anomaly_promote_identity_churn: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_error_burst: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cache_discontinuity: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cross_pane_edit_cluster: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_cost_slope: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_token_slope: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_memory_growth: crate::domain::anomaly::AnomalyConfidence::High,
            anomaly_promote_subagent_side_effect: crate::domain::anomaly::AnomalyConfidence::Medium,
```

- [ ] **Step 3: Wire from config in `from_inputs` (line ~221)**

Locate the existing block in `from_inputs` that populates `anomaly_*` from `&inputs.config.anomaly`. After the existing population lines (after `anomaly_memory_growth_mb: anomaly.memory_growth_mb,` or the last anomaly field), add:

```rust
            anomaly_promote_identity_churn: parse_min_confidence(&anomaly.promote.identity_churn),
            anomaly_promote_error_burst: parse_min_confidence(&anomaly.promote.error_burst),
            anomaly_promote_cache_discontinuity: parse_min_confidence(&anomaly.promote.cache_discontinuity),
            anomaly_promote_cross_pane_edit_cluster: parse_min_confidence(&anomaly.promote.cross_pane_edit_cluster),
            anomaly_promote_cost_slope: parse_min_confidence(&anomaly.promote.cost_slope),
            anomaly_promote_token_slope: parse_min_confidence(&anomaly.promote.token_slope),
            anomaly_promote_memory_growth: parse_min_confidence(&anomaly.promote.memory_growth),
            anomaly_promote_subagent_side_effect: parse_min_confidence(&anomaly.promote.subagent_side_effect),
```

- [ ] **Step 4: Add the `promote_min_confidence` helper method**

After the `impl PolicyGates` block (or inside it; pick the existing pattern in this file), add:

```rust
impl PolicyGates {
    /// Phase 7 v3 (v1.46.0): per-kind promotion threshold lookup.
    pub fn promote_min_confidence(
        &self,
        kind: crate::domain::anomaly::AnomalyKind,
    ) -> crate::domain::anomaly::AnomalyConfidence {
        use crate::domain::anomaly::AnomalyKind;
        match kind {
            AnomalyKind::IdentityChurn => self.anomaly_promote_identity_churn,
            AnomalyKind::ErrorBurst => self.anomaly_promote_error_burst,
            AnomalyKind::CacheDiscontinuity => self.anomaly_promote_cache_discontinuity,
            AnomalyKind::CrossPaneEditCluster => self.anomaly_promote_cross_pane_edit_cluster,
            AnomalyKind::CostSlope => self.anomaly_promote_cost_slope,
            AnomalyKind::TokenSlope => self.anomaly_promote_token_slope,
            AnomalyKind::MemoryGrowth => self.anomaly_promote_memory_growth,
            AnomalyKind::SubagentSideEffect => self.anomaly_promote_subagent_side_effect,
        }
    }
}
```

(If the file already has an `impl PolicyGates` block, place this method inside it instead of creating a new one.)

- [ ] **Step 5: Add tests at the bottom of the existing `#[cfg(test)] mod tests` block**

```rust
#[test]
fn promote_min_confidence_returns_configured_field_per_kind() {
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
    let gates = PolicyGates {
        anomaly_promote_identity_churn: AnomalyConfidence::Low,
        anomaly_promote_error_burst: AnomalyConfidence::Medium,
        anomaly_promote_cache_discontinuity: AnomalyConfidence::High,
        anomaly_promote_cross_pane_edit_cluster: AnomalyConfidence::Low,
        anomaly_promote_cost_slope: AnomalyConfidence::Medium,
        anomaly_promote_token_slope: AnomalyConfidence::High,
        anomaly_promote_memory_growth: AnomalyConfidence::Low,
        anomaly_promote_subagent_side_effect: AnomalyConfidence::Medium,
        ..PolicyGates::default()
    };
    let cases = [
        (AnomalyKind::IdentityChurn, AnomalyConfidence::Low),
        (AnomalyKind::ErrorBurst, AnomalyConfidence::Medium),
        (AnomalyKind::CacheDiscontinuity, AnomalyConfidence::High),
        (AnomalyKind::CrossPaneEditCluster, AnomalyConfidence::Low),
        (AnomalyKind::CostSlope, AnomalyConfidence::Medium),
        (AnomalyKind::TokenSlope, AnomalyConfidence::High),
        (AnomalyKind::MemoryGrowth, AnomalyConfidence::Low),
        (AnomalyKind::SubagentSideEffect, AnomalyConfidence::Medium),
    ];
    for (kind, expected) in cases {
        assert_eq!(gates.promote_min_confidence(kind), expected, "kind {kind:?}");
    }
}

#[test]
fn policy_gates_default_promote_thresholds_match_spec() {
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
    let gates = PolicyGates::default();
    assert_eq!(
        gates.promote_min_confidence(AnomalyKind::IdentityChurn),
        AnomalyConfidence::High
    );
    assert_eq!(
        gates.promote_min_confidence(AnomalyKind::CostSlope),
        AnomalyConfidence::High
    );
    assert_eq!(
        gates.promote_min_confidence(AnomalyKind::SubagentSideEffect),
        AnomalyConfidence::Medium
    );
}

#[test]
fn from_inputs_parses_promote_config() {
    use crate::domain::anomaly::AnomalyConfidence;
    let mut config = crate::app::config::Config::default();
    config.anomaly.promote.cost_slope = "low".to_string();
    config.anomaly.promote.subagent_side_effect = "high".to_string();
    let inputs = PolicyGateInputs {
        config: &config,
        // ... existing inputs.config-only construction; copy from
        // existing tests in this file that use PolicyGateInputs
    };
    let gates = PolicyGates::from_inputs(inputs);
    assert_eq!(gates.anomaly_promote_cost_slope, AnomalyConfidence::Low);
    assert_eq!(
        gates.anomaly_promote_subagent_side_effect,
        AnomalyConfidence::High
    );
}
```

(Note: the third test's `PolicyGateInputs` construction must mirror the existing `from_inputs` tests in this file — copy the construction pattern from the nearest existing test that uses `PolicyGateInputs::default()` or similar. If no such helper exists, use the same field-by-field construction as the other `from_inputs_*` tests in this same module.)

- [ ] **Step 6: Run targeted tests**

Run: `cargo test --lib gates::tests::promote_min_confidence_returns_configured_field_per_kind gates::tests::policy_gates_default_promote_thresholds_match_spec gates::tests::from_inputs_parses_promote_config`
Expected: 3 passed.

- [ ] **Step 7: Run full lib suite + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --lib`
Expected: clean; lib test count up by 3 since Task 2.

- [ ] **Step 8: Commit**

```bash
git add src/policy/gates.rs
git commit -m "phase 7 v3 (a+b): PolicyGates +8 anomaly_promote_* + promote_min_confidence helper"
```

---

## Task 4: Add `AnomalyEventsRing` module

**Files:**

- Create: `src/app/anomaly_events_ring.rs`
- Modify: `src/app/mod.rs` (register module)

- [ ] **Step 1: Create the module file with full implementation + tests**

Create `src/app/anomaly_events_ring.rs`:

```rust
//! Phase 7 v3 (v1.46.0): bounded in-memory ring buffer of recent
//! `AnomalyEvent` records. Pushed by `event_loop` after
//! `eval_anomalies` and the per-kind promotion gate run; rendered
//! by the `n` overlay (`src/ui/anomaly_overlay.rs`).
//!
//! Capacity is a `const` (100). Operator-configurable size is
//! deferred. Buffer resets on every session start (no persistence;
//! that is the (c) sub-feature of Phase 7 v3, deferred to v1.47+).

use crate::domain::anomaly::AnomalyEvent;
use std::collections::VecDeque;

#[derive(Debug, Default, Clone)]
pub struct AnomalyEventsRing {
    events: VecDeque<AnomalyEvent>,
}

impl AnomalyEventsRing {
    pub const CAPACITY: usize = 100;

    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(Self::CAPACITY),
        }
    }

    pub fn push(&mut self, event: AnomalyEvent) {
        if self.events.len() >= Self::CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn iter(&self) -> impl Iterator<Item = &AnomalyEvent> + '_ {
        self.events.iter()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
    use crate::domain::recommendation::Severity;

    fn fixture_event(ts: i64) -> AnomalyEvent {
        AnomalyEvent {
            timestamp: ts,
            pane_id: "%1".to_string(),
            kind: AnomalyKind::CostSlope,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            promoted: true,
            reason: format!("cost_usd at ts={ts}"),
        }
    }

    #[test]
    fn ring_starts_empty() {
        let ring = AnomalyEventsRing::new();
        assert!(ring.is_empty());
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn ring_capacity_constant_is_100() {
        assert_eq!(AnomalyEventsRing::CAPACITY, 100);
    }

    #[test]
    fn ring_push_below_capacity_grows_len() {
        let mut ring = AnomalyEventsRing::new();
        for ts in 0..50 {
            ring.push(fixture_event(ts));
        }
        assert_eq!(ring.len(), 50);
        assert!(!ring.is_empty());
    }

    #[test]
    fn ring_push_at_capacity_drops_oldest() {
        let mut ring = AnomalyEventsRing::new();
        for ts in 0..(AnomalyEventsRing::CAPACITY as i64 + 1) {
            ring.push(fixture_event(ts));
        }
        assert_eq!(ring.len(), AnomalyEventsRing::CAPACITY);
        // oldest (ts=0) is evicted; first remaining is ts=1
        let first = ring.iter().next().expect("non-empty");
        assert_eq!(first.timestamp, 1);
    }

    #[test]
    fn ring_iter_yields_insertion_order_oldest_first() {
        let mut ring = AnomalyEventsRing::new();
        ring.push(fixture_event(10));
        ring.push(fixture_event(20));
        ring.push(fixture_event(30));
        let timestamps: Vec<i64> = ring.iter().map(|e| e.timestamp).collect();
        assert_eq!(timestamps, vec![10, 20, 30]);
    }
}
```

- [ ] **Step 2: Register module in `src/app/mod.rs`**

Edit `src/app/mod.rs`. Add a new line in alphabetical position with the existing `pub mod` lines:

```rust
pub mod anomaly_events_ring;
```

- [ ] **Step 3: Build + run targeted tests**

Run: `cargo test --lib app::anomaly_events_ring::tests`
Expected: 5 passed.

- [ ] **Step 4: Run fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/app/anomaly_events_ring.rs src/app/mod.rs
git commit -m "phase 7 v3 (a+b): AnomalyEventsRing (capacity 100, in-memory) + 5 tests"
```

---

## Task 5: Replace promote-gate predicate (severity-based → per-kind confidence)

**Files:**

- Modify: `src/policy/rules/anomaly.rs` (signature change + gate predicate)
- Modify: `src/app/event_loop.rs` (call-site signature update)
- Modify: any other callers of `promote_anomalies_to_recommendations` (search-and-replace)

- [ ] **Step 1: Find all callers**

Run: `grep -rn "promote_anomalies_to_recommendations" src/ tests/`

Note every call site. Expected: declaration in `src/policy/rules/anomaly.rs`, one call in `src/app/event_loop.rs`, possibly tests inside `src/policy/rules/anomaly.rs` and `tests/`.

- [ ] **Step 2: Update the function signature in `src/policy/rules/anomaly.rs`**

Change:

```rust
pub fn promote_anomalies_to_recommendations(signals: &[AnomalySignal]) -> Vec<Recommendation> {
    let mut out = Vec::new();
    for sig in signals {
        if sig.severity < Severity::Warning {
            continue;
        }
        // ... promote matrix (unchanged) ...
    }
    out
}
```

to:

```rust
pub fn promote_anomalies_to_recommendations(
    signals: &[AnomalySignal],
    gates: &crate::policy::gates::PolicyGates,
) -> Vec<Recommendation> {
    let mut out = Vec::new();
    for sig in signals {
        if sig.confidence < gates.promote_min_confidence(sig.kind) {
            continue;
        }
        // ... promote matrix (unchanged) ...
    }
    out
}
```

The promote matrix (the `match sig.kind { ... }` block that builds `(action, reason, next_step)`) is unchanged. Only the signature and the `if continue` predicate change.

- [ ] **Step 3: Update the call site in `src/app/event_loop.rs`**

Locate the existing line that looks roughly like:

```rust
let recs = promote_anomalies_to_recommendations(&signals);
```

Change to:

```rust
let recs = promote_anomalies_to_recommendations(&signals, gates);
```

(The variable name for the gates reference may differ — use whatever local `&PolicyGates` is already in scope at that call site. It is in scope because `eval_anomalies` is called immediately above with `gates`.)

- [ ] **Step 4: Update any test call sites in `src/policy/rules/anomaly.rs`**

For every existing test that calls `promote_anomalies_to_recommendations(...)` with a single argument, add a second argument. Use the existing `fixture_gates()` helper:

```rust
let recs = promote_anomalies_to_recommendations(&signals, &fixture_gates());
```

- [ ] **Step 5: Run lib tests to confirm existing behavior preserved**

Run: `cargo test --lib`
Expected: all existing tests still pass; no count regression.

- [ ] **Step 6: Add new gate-predicate tests inside the existing `#[cfg(test)] mod tests` block in `src/policy/rules/anomaly.rs`**

```rust
#[test]
fn promote_gate_blocks_below_per_kind_threshold() {
    use crate::domain::anomaly::{
        AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
    };
    use crate::domain::source::SourceKind;
    let gates = fixture_gates(); // defaults: cost_slope = High
    let signal = AnomalySignal {
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::Medium,
        severity: severity_for(AnomalyConfidence::Medium),
        window_polls: 20,
        evidence: vec![AnomalyEvidence {
            metric_name: "cost_usd",
            before: "0.10".to_string(),
            after: "25.40".to_string(),
            sample_count: 20,
            source_kind: SourceKind::Provider,
        }],
    };
    let recs = promote_anomalies_to_recommendations(&[signal], &gates);
    assert!(recs.is_empty(), "Medium CostSlope must NOT promote at default High threshold");
}

#[test]
fn promote_gate_admits_at_per_kind_threshold() {
    use crate::domain::anomaly::{
        AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
    };
    use crate::domain::source::SourceKind;
    let gates = PolicyGates {
        anomaly_promote_cost_slope: AnomalyConfidence::Medium,
        ..fixture_gates()
    };
    let signal = AnomalySignal {
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::Medium,
        severity: severity_for(AnomalyConfidence::Medium),
        window_polls: 20,
        evidence: vec![AnomalyEvidence {
            metric_name: "cost_usd",
            before: "0.10".to_string(),
            after: "25.40".to_string(),
            sample_count: 20,
            source_kind: SourceKind::Provider,
        }],
    };
    let recs = promote_anomalies_to_recommendations(&[signal], &gates);
    assert_eq!(recs.len(), 1, "Medium CostSlope must promote at Medium threshold");
}

#[test]
fn promote_gate_subagent_side_effect_default_medium_admits() {
    use crate::domain::anomaly::{
        AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
    };
    use crate::domain::source::SourceKind;
    let gates = fixture_gates(); // default subagent_side_effect = Medium
    let signal = AnomalySignal {
        kind: AnomalyKind::SubagentSideEffect,
        confidence: AnomalyConfidence::Medium,
        severity: severity_for(AnomalyConfidence::Medium),
        window_polls: 20,
        evidence: vec![AnomalyEvidence {
            metric_name: "subagent_hint",
            before: "0".to_string(),
            after: "3".to_string(),
            sample_count: 20,
            source_kind: SourceKind::Estimated,
        }],
    };
    let recs = promote_anomalies_to_recommendations(&[signal], &gates);
    assert_eq!(
        recs.len(),
        1,
        "Medium SubagentSideEffect must promote at default Medium threshold (v1.45.0 dead-code fix)"
    );
}

#[test]
fn promote_gate_subagent_side_effect_high_setting_blocks_medium() {
    use crate::domain::anomaly::{
        AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
    };
    use crate::domain::source::SourceKind;
    let gates = PolicyGates {
        anomaly_promote_subagent_side_effect: AnomalyConfidence::High,
        ..fixture_gates()
    };
    let signal = AnomalySignal {
        kind: AnomalyKind::SubagentSideEffect,
        confidence: AnomalyConfidence::Medium,
        severity: severity_for(AnomalyConfidence::Medium),
        window_polls: 20,
        evidence: vec![AnomalyEvidence {
            metric_name: "subagent_hint",
            before: "0".to_string(),
            after: "3".to_string(),
            sample_count: 20,
            source_kind: SourceKind::Estimated,
        }],
    };
    let recs = promote_anomalies_to_recommendations(&[signal], &gates);
    assert!(
        recs.is_empty(),
        "Operator override to High blocks Medium SubagentSideEffect"
    );
}

#[test]
fn promote_gate_high_threshold_admits_high() {
    use crate::domain::anomaly::{
        AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
    };
    use crate::domain::source::SourceKind;
    let gates = fixture_gates(); // default cost_slope = High
    let signal = AnomalySignal {
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: severity_for(AnomalyConfidence::High),
        window_polls: 20,
        evidence: vec![AnomalyEvidence {
            metric_name: "cost_usd",
            before: "0.10".to_string(),
            after: "25.40".to_string(),
            sample_count: 20,
            source_kind: SourceKind::Provider,
        }],
    };
    let recs = promote_anomalies_to_recommendations(&[signal], &gates);
    assert_eq!(recs.len(), 1, "High CostSlope must promote at default High threshold");
}

#[test]
fn promote_gate_low_threshold_admits_all_levels() {
    use crate::domain::anomaly::{
        AnomalyConfidence, AnomalyEvidence, AnomalyKind, AnomalySignal,
    };
    use crate::domain::source::SourceKind;
    let gates = PolicyGates {
        anomaly_promote_cost_slope: AnomalyConfidence::Low,
        ..fixture_gates()
    };
    let make = |conf: AnomalyConfidence| AnomalySignal {
        kind: AnomalyKind::CostSlope,
        confidence: conf,
        severity: severity_for(conf),
        window_polls: 20,
        evidence: vec![AnomalyEvidence {
            metric_name: "cost_usd",
            before: "0.10".to_string(),
            after: "25.40".to_string(),
            sample_count: 20,
            source_kind: SourceKind::Provider,
        }],
    };
    let signals = vec![
        make(AnomalyConfidence::Low),
        make(AnomalyConfidence::Medium),
        make(AnomalyConfidence::High),
    ];
    let recs = promote_anomalies_to_recommendations(&signals, &gates);
    assert_eq!(recs.len(), 3, "Low threshold admits all three confidence levels");
}
```

- [ ] **Step 7: Run new tests**

Run: `cargo test --lib promote_gate_`
Expected: 6 passed.

- [ ] **Step 8: Run full lib + integration suite + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets`
Expected: clean; lib test count up by 6 since Task 4; integration unchanged.

- [ ] **Step 9: Commit**

```bash
git add src/policy/rules/anomaly.rs src/app/event_loop.rs
git commit -m "phase 7 v3 (a+b): per-kind promote gate (signature + predicate) + 6 tests"
```

---

## Task 6: Add `anomaly_events_ring` field to `Context` + push from `run_once_with_target`

**Files:**

- Modify: `src/app/bootstrap.rs` (add field + initialize in constructor)
- Modify: `src/app/event_loop.rs` (push site after the gated promote call)
- Possibly: existing tests that construct `Context` directly (search for `Context {` literal init).

- [ ] **Step 1: Add the `anomaly_events_ring` field to `Context<P, N>`**

Edit `src/app/bootstrap.rs`. In the `pub struct Context<P: PaneSource, N: NotifyBackend>` definition (line ~26), add a new field — pick a logical position alongside other in-memory caches (e.g., near `tail_history`):

```rust
    /// Phase 7 v3 (v1.46.0): in-memory ring buffer of recent
    /// `AnomalyEvent`s, populated by `run_once_with_target` after
    /// the per-kind promotion gate runs. Rendered by the `n` overlay.
    /// Capacity 100 (see `AnomalyEventsRing::CAPACITY`); session-only.
    pub anomaly_events_ring: crate::app::anomaly_events_ring::AnomalyEventsRing,
```

- [ ] **Step 2: Initialize the field in `Context` constructors**

Run: `grep -n "Context {" src/app/bootstrap.rs src/app/event_loop.rs tests/ src/ -r --include="*.rs"`

For every literal `Context { ... }` initializer, add the new field with `AnomalyEventsRing::new()`:

```rust
            anomaly_events_ring: crate::app::anomaly_events_ring::AnomalyEventsRing::new(),
```

If any test fixture uses `..Default::default()` or `..Context::default()` spread, no change is needed at that site (the Default impl, if any, handles it). Otherwise add the explicit field.

- [ ] **Step 3: Add the push block in `run_once_with_target`**

Edit `src/app/event_loop.rs`. The existing v1.44.0 promote block lives around line 520-529:

```rust
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

deliver_effects(...);
```

Task 5 already changed the `promote_anomalies_to_recommendations` call to pass `&gates`. Add the ring buffer push immediately AFTER the existing for-loop (post-promote, pre-`deliver_effects`):

```rust
        // Phase 7 v3 (v1.46.0): push every visible signal into the
        // AnomalyEventsRing with its gate result, for the `n` overlay.
        // `anomalies` holds the post-eval_anomalies signal vec (visible
        // signals, post-visibility-filter); `gates` is the same
        // PolicyGates value used by the promote call above.
        let now_secs = (current_unix_ms() / 1000) as i64;
        for sig in &anomalies {
            let promoted_bool =
                sig.confidence >= gates.promote_min_confidence(sig.kind);
            let reason = sig
                .evidence
                .first()
                .map(|e| format!("{}: {} → {}", e.metric_name, e.before, e.after))
                .unwrap_or_default();
            ctx.anomaly_events_ring.push(crate::domain::anomaly::AnomalyEvent {
                timestamp: now_secs,
                pane_id: pane.pane_id.clone(),
                kind: sig.kind,
                confidence: sig.confidence,
                severity: sig.severity,
                promoted: promoted_bool,
                reason,
            });
        }
```

(The local `anomalies` variable is the existing v1.44.0 name for the `eval_anomalies` result vec at this point — confirm by reading lines 505-525. `current_unix_ms` is already imported in this file. `pane.pane_id` is the current pane id loop variable.)

- [ ] **Step 4: Run lib + integration tests + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets`
Expected: clean; existing test counts preserved.

- [ ] **Step 5: Commit**

```bash
git add src/app/bootstrap.rs src/app/event_loop.rs
git commit -m "phase 7 v3 (a+b): Context.anomaly_events_ring + push site in run_once_with_target"
```

---

## Task 7: Add `AnomalyOverlay` state struct + key handler

**Files:**

- Create: `src/ui/anomaly_overlay.rs` (state struct + render placeholder)
- Create: `src/app/anomaly_overlay.rs` (key handler)
- Modify: `src/ui/mod.rs` (register module)
- Modify: `src/app/mod.rs` (register module)

- [ ] **Step 1: Create `src/ui/anomaly_overlay.rs` with state struct**

```rust
//! Phase 7 v3 (v1.46.0): `n` overlay — recent anomaly events
//! visualization. Renders `AnomalyEventsRing` chronologically
//! (newest first) with `pane_id` column. Read-only; no actions.

use ratatui::layout::Rect;
use ratatui::Frame;

#[derive(Debug, Default, Clone)]
pub struct AnomalyOverlay {
    open: bool,
    scroll: usize,
}

impl AnomalyOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.scroll = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn scroll_down(&mut self, max: usize) {
        if self.scroll < max {
            self.scroll += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Render is added in Task 9; for Task 7 we leave a stub that
    /// the wiring task can call without panicking.
    pub fn render(
        &self,
        _frame: &mut Frame,
        _area: Rect,
        _ring: &crate::app::anomaly_events_ring::AnomalyEventsRing,
    ) {
        // implemented in Task 9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_starts_closed() {
        let o = AnomalyOverlay::new();
        assert!(!o.is_open());
        assert_eq!(o.scroll(), 0);
    }

    #[test]
    fn open_resets_scroll_to_zero() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(o.is_open());
        o.scroll_down(50);
        assert!(o.scroll() > 0);
        o.close();
        o.open();
        assert_eq!(o.scroll(), 0);
    }

    #[test]
    fn toggle_flips_state() {
        let mut o = AnomalyOverlay::new();
        o.toggle();
        assert!(o.is_open());
        o.toggle();
        assert!(!o.is_open());
    }

    #[test]
    fn scroll_down_clamps_at_max() {
        let mut o = AnomalyOverlay::new();
        o.open();
        for _ in 0..10 {
            o.scroll_down(3);
        }
        assert_eq!(o.scroll(), 3);
    }

    #[test]
    fn scroll_up_saturates_at_zero() {
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_up();
        assert_eq!(o.scroll(), 0);
    }
}
```

- [ ] **Step 2: Register `src/ui/anomaly_overlay.rs` in `src/ui/mod.rs`**

Add a `pub mod anomaly_overlay;` line in alphabetical position with existing `pub mod` lines.

- [ ] **Step 3: Create `src/app/anomaly_overlay.rs` with the key handler**

```rust
//! Phase 7 v3 (v1.46.0): keyboard handler for the `n` (anomaly
//! events) overlay. Mirrors the `m` overlay handler's contract
//! (returns `true` when the key is consumed).

use crate::ui::anomaly_overlay::AnomalyOverlay;
use crossterm::event::KeyCode;

pub fn handle_anomaly_overlay_key(
    overlay: &mut AnomalyOverlay,
    ring_len: usize,
    key: KeyCode,
) -> bool {
    match key {
        KeyCode::Char('n') => {
            overlay.toggle();
            true
        }
        KeyCode::Esc | KeyCode::Char('q') if overlay.is_open() => {
            overlay.close();
            true
        }
        KeyCode::Down | KeyCode::Char('j') if overlay.is_open() => {
            overlay.scroll_down(ring_len.saturating_sub(1));
            true
        }
        KeyCode::Up | KeyCode::Char('k') if overlay.is_open() => {
            overlay.scroll_up();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_toggles_open_close() {
        let mut o = AnomalyOverlay::new();
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Char('n')));
        assert!(o.is_open());
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Char('n')));
        assert!(!o.is_open());
    }

    #[test]
    fn esc_closes_when_open_no_op_when_closed() {
        let mut o = AnomalyOverlay::new();
        assert!(!handle_anomaly_overlay_key(&mut o, 0, KeyCode::Esc));
        assert!(!o.is_open());
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Esc));
        assert!(!o.is_open());
    }

    #[test]
    fn q_closes_when_open() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('q')));
        assert!(!o.is_open());
    }

    #[test]
    fn down_and_j_scroll_within_bounds() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Down));
        assert_eq!(o.scroll(), 1);
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Char('j')));
        assert_eq!(o.scroll(), 2);
        // ring_len=3 means max scroll index = 2
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Down));
        assert_eq!(o.scroll(), 2);
    }

    #[test]
    fn up_and_k_scroll_saturate_at_zero() {
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_down(5);
        o.scroll_down(5);
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Up));
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('k')));
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('k')));
        assert_eq!(o.scroll(), 0);
    }

    #[test]
    fn keys_ignored_when_closed() {
        let mut o = AnomalyOverlay::new();
        assert!(!handle_anomaly_overlay_key(&mut o, 5, KeyCode::Down));
        assert!(!handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('q')));
    }
}
```

- [ ] **Step 4: Register `src/app/anomaly_overlay.rs` in `src/app/mod.rs`**

Add `pub mod anomaly_overlay;` in alphabetical position.

- [ ] **Step 5: Run targeted tests + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --lib ui::anomaly_overlay app::anomaly_overlay`
Expected: 11 passed (5 state + 6 key handler).

- [ ] **Step 6: Commit**

```bash
git add src/ui/anomaly_overlay.rs src/ui/mod.rs src/app/anomaly_overlay.rs src/app/mod.rs
git commit -m "phase 7 v3 (a+b): AnomalyOverlay state + key handler + 11 tests"
```

---

## Task 8: Add `handle_anomaly_overlay_mouse` (mouse handler)

**Files:**

- Modify: `src/app/anomaly_overlay.rs` (add mouse handler + tests)

- [ ] **Step 1: Read the existing pattern**

Read `src/app/metrics_overlay.rs:42-...` (the `handle_metrics_overlay_mouse` function) to see exactly which `MouseEvent` shape and `Rect` viewport pattern is used. Mirror that pattern.

Run: `grep -n "fn handle_metrics_overlay_mouse" src/app/metrics_overlay.rs`

Then read the function body. The new handler must mirror:

- Scroll-wheel up/down adjusts the overlay's scroll.
- Click outside the overlay rect closes the overlay.
- Click inside is a no-op (no row interactions in v1).
- Returns `bool` (consumed?) matching the `metrics_overlay` convention.

- [ ] **Step 2: Add the mouse handler at the bottom of `src/app/anomaly_overlay.rs`**

Mirror `handle_metrics_overlay_mouse` body exactly, replacing `MetricsOverlay` → `AnomalyOverlay`. The general shape (will need exact details from reading `metrics_overlay.rs`):

```rust
use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

pub fn handle_anomaly_overlay_mouse(
    overlay: &mut AnomalyOverlay,
    viewport: Rect,
    ring_len: usize,
    event: MouseEvent,
) -> bool {
    if !overlay.is_open() {
        return false;
    }
    match event.kind {
        MouseEventKind::ScrollDown => {
            overlay.scroll_down(ring_len.saturating_sub(1));
            true
        }
        MouseEventKind::ScrollUp => {
            overlay.scroll_up();
            true
        }
        MouseEventKind::Down(_) => {
            // click outside overlay rect closes; inside is no-op
            let inside_x = event.column >= viewport.x
                && event.column < viewport.x + viewport.width;
            let inside_y = event.row >= viewport.y
                && event.row < viewport.y + viewport.height;
            if !(inside_x && inside_y) {
                overlay.close();
                return true;
            }
            false
        }
        _ => false,
    }
}
```

(If `handle_metrics_overlay_mouse` differs in nuance — e.g., it reads `viewport` differently, or handles drag — match its pattern, including that nuance.)

- [ ] **Step 3: Add mouse handler tests in the same `#[cfg(test)] mod tests` block**

```rust
#[test]
fn mouse_wheel_down_scrolls() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut o = AnomalyOverlay::new();
    o.open();
    let viewport = Rect::new(0, 0, 80, 24);
    let event = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 10,
        row: 5,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
    assert_eq!(o.scroll(), 1);
}

#[test]
fn mouse_wheel_up_scrolls() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut o = AnomalyOverlay::new();
    o.open();
    o.scroll_down(5);
    o.scroll_down(5);
    let viewport = Rect::new(0, 0, 80, 24);
    let event = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 5,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
    assert_eq!(o.scroll(), 1);
}

#[test]
fn mouse_click_outside_viewport_closes() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut o = AnomalyOverlay::new();
    o.open();
    let viewport = Rect::new(10, 5, 20, 10);
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
    assert!(!o.is_open());
}

#[test]
fn mouse_click_inside_viewport_is_noop() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut o = AnomalyOverlay::new();
    o.open();
    let viewport = Rect::new(10, 5, 20, 10);
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 15,
        row: 8,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(!handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
    assert!(o.is_open());
}

#[test]
fn mouse_no_op_when_closed() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut o = AnomalyOverlay::new();
    let viewport = Rect::new(0, 0, 80, 24);
    let event = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 10,
        row: 5,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(!handle_anomaly_overlay_mouse(&mut o, viewport, 5, event));
}
```

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --lib app::anomaly_overlay`
Expected: 11 passed (6 key + 5 mouse).

- [ ] **Step 5: Commit**

```bash
git add src/app/anomaly_overlay.rs
git commit -m "phase 7 v3 (a+b): AnomalyOverlay mouse handler + 5 tests"
```

---

## Task 9: Implement `AnomalyOverlay::render`

**Files:**

- Modify: `src/ui/anomaly_overlay.rs` (replace render stub with real implementation + render tests)

- [ ] **Step 1: Read existing `MetricsOverlay::render` for the rendering pattern**

Run: `grep -n "fn render\|impl MetricsOverlay" src/ui/metrics.rs | head -10`

Read the render method around the lines reported. Mirror the pattern: centered popup `Rect`, `Block` with title border, inner `List` or `Paragraph` widget, scroll offset applied to visible rows.

- [ ] **Step 2: Replace the stub render with a real implementation**

In `src/ui/anomaly_overlay.rs`, replace the existing stub `render` method with:

```rust
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

impl AnomalyOverlay {
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        ring: &crate::app::anomaly_events_ring::AnomalyEventsRing,
    ) {
        let popup_area = centered_rect(80, 70, area);
        frame.render_widget(Clear, popup_area);

        let title = format!(
            " ANOMALY EVENTS (last {} — newest first) [n to close] ",
            ring.len()
        );
        let block = Block::default().borders(Borders::ALL).title(title);

        if ring.is_empty() {
            let body = Paragraph::new(vec![
                Line::from("No anomaly events recorded this session."),
                Line::from(""),
                Line::from(
                    "Anomalies are recorded once detection is enabled in Settings: [anomaly] enabled = true.",
                ),
            ])
            .block(block)
            .wrap(Wrap { trim: false });
            frame.render_widget(body, popup_area);
            return;
        }

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let header = format!(
            "{:<8}  {:<6}  {:<22}  {:<6}  {:<8}  {}",
            "Time", "Pane", "Kind", "Conf", "Promoted", "Reason"
        );

        // newest-first iteration; apply scroll offset; clip to inner.height-1 (room for header)
        let visible_rows = inner.height.saturating_sub(1) as usize;
        let items: Vec<ListItem> = ring
            .iter()
            .rev()
            .skip(self.scroll)
            .take(visible_rows)
            .map(|e| ListItem::new(format_event_row(e, inner.width as usize)))
            .collect();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        frame.render_widget(
            Paragraph::new(header).style(Style::default().add_modifier(Modifier::BOLD)),
            layout[0],
        );
        frame.render_widget(List::new(items), layout[1]);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn format_event_row(e: &crate::domain::anomaly::AnomalyEvent, width: usize) -> String {
    let time = format_hhmmss(e.timestamp);
    let pane = truncate(&e.pane_id, 6);
    let kind = e.kind.label();
    let conf = e.confidence.label();
    let promoted = if e.promoted { "yes" } else { "no" };
    let row_prefix = format!(
        "{:<8}  {:<6}  {:<22}  {:<6}  {:<8}  ",
        time, pane, kind, conf, promoted
    );
    let reason_budget = width.saturating_sub(row_prefix.chars().count());
    let reason = truncate(&e.reason, reason_budget);
    format!("{row_prefix}{reason}")
}

fn format_hhmmss(ts: i64) -> String {
    // Local time HH:MM:SS. Use chrono if it is already a project
    // dependency; otherwise format manually from `ts`.
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "??:??:??".to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
```

(Note: confirm `chrono` is already a project dependency by running `grep "^chrono" Cargo.toml`. If not present, replace `format_hhmmss` with a manual implementation that doesn't introduce a new dep. The codebase already formats timestamps elsewhere — search `grep -rn "%H:%M:%S\|format_timestamp\|format_hhmmss" src/`. Mirror whatever exists; do NOT add a new crate dependency in this task.)

- [ ] **Step 3: Add render tests**

```rust
#[test]
fn render_empty_state_includes_help_hint() {
    use crate::app::anomaly_events_ring::AnomalyEventsRing;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let overlay = AnomalyOverlay { open: true, scroll: 0 };
    let ring = AnomalyEventsRing::new();
    terminal.draw(|f| overlay.render(f, f.size(), &ring)).unwrap();
    let buf = terminal.backend().buffer();
    let rendered = buf_to_string(buf);
    assert!(
        rendered.contains("No anomaly events recorded this session."),
        "empty state body missing: {rendered}"
    );
    assert!(
        rendered.contains("[anomaly] enabled = true"),
        "help hint missing: {rendered}"
    );
}

#[test]
fn render_populated_shows_columns_and_at_least_one_row() {
    use crate::app::anomaly_events_ring::AnomalyEventsRing;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let overlay = AnomalyOverlay { open: true, scroll: 0 };
    let mut ring = AnomalyEventsRing::new();
    ring.push(AnomalyEvent {
        timestamp: 1_700_000_000,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: "cost_usd: 0.10 → 25.40".to_string(),
    });
    terminal.draw(|f| overlay.render(f, f.size(), &ring)).unwrap();
    let rendered = buf_to_string(terminal.backend().buffer());
    assert!(rendered.contains("Time"), "header 'Time' missing: {rendered}");
    assert!(rendered.contains("Kind"), "header 'Kind' missing");
    assert!(rendered.contains("Promoted"), "header 'Promoted' missing");
    assert!(rendered.contains("CostSlope"), "kind value missing");
    assert!(rendered.contains("yes"), "promoted=yes missing");
}

#[test]
fn render_truncates_long_reason() {
    use crate::app::anomaly_events_ring::AnomalyEventsRing;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvent, AnomalyKind};
    use crate::domain::recommendation::Severity;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(60, 12); // narrow on purpose
    let mut terminal = Terminal::new(backend).unwrap();
    let overlay = AnomalyOverlay { open: true, scroll: 0 };
    let mut ring = AnomalyEventsRing::new();
    let long_reason = "A".repeat(200);
    ring.push(AnomalyEvent {
        timestamp: 1_700_000_000,
        pane_id: "%1".to_string(),
        kind: AnomalyKind::CostSlope,
        confidence: AnomalyConfidence::High,
        severity: Severity::Warning,
        promoted: true,
        reason: long_reason.clone(),
    });
    terminal.draw(|f| overlay.render(f, f.size(), &ring)).unwrap();
    let rendered = buf_to_string(terminal.backend().buffer());
    // No row should literally contain the full 200-char reason
    assert!(
        !rendered.contains(&long_reason),
        "reason was not truncated to fit"
    );
    // But a truncation marker should appear somewhere on a CostSlope row
    assert!(rendered.contains("…"), "truncation ellipsis missing");
}

#[cfg(test)]
fn buf_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf.get(x, y).symbol());
        }
        out.push('\n');
    }
    out
}
```

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --lib ui::anomaly_overlay`
Expected: 8 passed (5 state from Task 7 + 3 render).

- [ ] **Step 5: Commit**

```bash
git add src/ui/anomaly_overlay.rs
git commit -m "phase 7 v3 (a+b): AnomalyOverlay render (newest-first list, truncated reason) + 3 tests"
```

---

## Task 10: Wire `AnomalyOverlay` into `tui_loop` + `dashboard_render`

**Files:**

- Modify: `src/app/tui_loop.rs` (construct overlay local; key/mouse dispatch; render-context wiring)
- Modify: `src/app/dashboard_render.rs` (add render branch + add fields to view struct)

(Note: `Context.anomaly_events_ring` was already added in Task 6. `tui_loop.rs` and `dashboard_render.rs` access it via `&ctx.anomaly_events_ring`.)

- [ ] **Step 1: Construct `anomaly_overlay` local in `tui_loop.rs`**

Edit `src/app/tui_loop.rs`. The metrics overlay is constructed at line 77:

```rust
let mut metrics_overlay = crate::ui::metrics::MetricsOverlay::new();
```

Add a parallel line right after it:

```rust
let mut anomaly_overlay = crate::ui::anomaly_overlay::AnomalyOverlay::new();
```

- [ ] **Step 2: Add overlay + ring buffer to the render-context struct in `dashboard_render.rs`**

Edit `src/app/dashboard_render.rs`. Find the existing `pub metrics_overlay: &'a crate::ui::metrics::MetricsOverlay,` field (around line 50). Add two fields right after it:

```rust
    pub anomaly_overlay: &'a crate::ui::anomaly_overlay::AnomalyOverlay,
    pub anomaly_events_ring: &'a crate::app::anomaly_events_ring::AnomalyEventsRing,
```

- [ ] **Step 3: Add render branch in `dashboard_render.rs`**

Locate the existing `if view.metrics_overlay.is_open() { ... }` block (around line 129). After that block, add a parallel branch:

```rust
    if view.anomaly_overlay.is_open() {
        view.anomaly_overlay.render(
            frame,
            view.metrics_overlay_area_or_full_area, // match the area variable used by the m branch
            view.anomaly_events_ring,
        );
    }
```

(Replace `view.metrics_overlay_area_or_full_area` with the actual area expression used by the existing m-overlay branch — typically just `area` or a derived rect. Read the metrics_overlay branch first to learn the local name.)

- [ ] **Step 4: Wire the render-context struct construction site in `tui_loop.rs`**

Find the construction site at line ~221 where `metrics_overlay: &metrics_overlay,` appears. Add the two new fields:

```rust
                            anomaly_overlay: &anomaly_overlay,
                            anomaly_events_ring: &ctx.anomaly_events_ring,
```

- [ ] **Step 5: Add key dispatch for the `n` overlay**

The existing `m` key dispatch lives at two locations (per `grep -n "metrics_overlay" src/app/tui_loop.rs`):

(a) Lines 323-325 — when `metrics_overlay.is_open()`, the m-handler gets first crack:

```rust
if metrics_overlay.is_open() {
    crate::app::metrics_overlay::handle_metrics_overlay_key(
        &mut metrics_overlay,
        // ... existing args ...
    );
    // ... existing continue/match arm ...
}
```

Add a parallel branch immediately after that block:

```rust
if anomaly_overlay.is_open() {
    if crate::app::anomaly_overlay::handle_anomaly_overlay_key(
        &mut anomaly_overlay,
        ctx.anomaly_events_ring.len(),
        key.code,
    ) {
        // matches the m-overlay arm: consumed-key behavior
        // (use the same `continue` / `return` pattern used by the m branch)
    }
}
```

(b) Lines 474-478 — the literal `KeyCode::Char('m')` arm that toggles the overlay when no other overlay is open. Add a parallel `KeyCode::Char('n')` arm right after the m arm:

```rust
KeyCode::Char('n') => {
    if anomaly_overlay.is_open() {
        anomaly_overlay.close();
    } else {
        anomaly_overlay.open();
    }
}
```

- [ ] **Step 6: Add mouse dispatch for the `n` overlay**

Lines 718-721 are the existing m-overlay mouse-handler dispatch. Mirror that block for the anomaly overlay (immediately after the m branch):

```rust
if anomaly_overlay.is_open() {
    crate::app::anomaly_overlay::handle_anomaly_overlay_mouse(
        &mut anomaly_overlay,
        // viewport rect — same value the m branch uses
        viewport_rect_used_by_m_branch,
        ctx.anomaly_events_ring.len(),
        mouse_event,
    );
}
```

(Replace `viewport_rect_used_by_m_branch` and `mouse_event` with the actual local variables in this section — read the m branch first.)

- [ ] **Step 7: Run lib + integration suite + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets`
Expected: clean; existing test counts preserved.

- [ ] **Step 8: Commit**

```bash
git add src/app/tui_loop.rs src/app/dashboard_render.rs
git commit -m "phase 7 v3 (a+b): tui_loop + dashboard_render wire AnomalyOverlay (n key + mouse + render)"
```

---

## Task 11: Settings UI — Parameters tab `[anomaly.promote]` section + Rules tab annotation update

**Files:**

- Modify: `src/ui/settings.rs`

- [ ] **Step 1: Read the existing pattern**

Run: `grep -n "\\[anomaly\\]\\|min_confidence\\|cost_slope_usd_per_hour" src/ui/settings.rs | head -20`

Find how the existing `[anomaly]` section header is rendered and how `min_confidence` / `cost_slope_usd_per_hour` rows are built. Mirror that exactly.

- [ ] **Step 2: Add `[anomaly.promote]` section + 8 rows in the Parameters tab**

After the last existing `[anomaly]` row (likely `memory_growth_mb`), insert a new section header `[anomaly.promote]` and 8 rows:

```rust
// Phase 7 v3 (v1.46.0): per-kind promotion thresholds
rows.push(section_header_row("[anomaly.promote]"));
rows.push(string_row(
    "identity_churn",
    &state.config.anomaly.promote.identity_churn,
    /*choices*/ &["low", "medium", "high"],
));
rows.push(string_row(
    "error_burst",
    &state.config.anomaly.promote.error_burst,
    &["low", "medium", "high"],
));
rows.push(string_row(
    "cache_discontinuity",
    &state.config.anomaly.promote.cache_discontinuity,
    &["low", "medium", "high"],
));
rows.push(string_row(
    "cross_pane_edit_cluster",
    &state.config.anomaly.promote.cross_pane_edit_cluster,
    &["low", "medium", "high"],
));
rows.push(string_row(
    "cost_slope",
    &state.config.anomaly.promote.cost_slope,
    &["low", "medium", "high"],
));
rows.push(string_row(
    "token_slope",
    &state.config.anomaly.promote.token_slope,
    &["low", "medium", "high"],
));
rows.push(string_row(
    "memory_growth",
    &state.config.anomaly.promote.memory_growth,
    &["low", "medium", "high"],
));
rows.push(string_row(
    "subagent_side_effect",
    &state.config.anomaly.promote.subagent_side_effect,
    &["low", "medium", "high"],
));
```

(Function names — `section_header_row`, `string_row` — are placeholders for whatever helper functions or row construction syntax this file already uses. Mirror the helpers that build the existing `[anomaly] min_confidence` row.)

- [ ] **Step 3: Update Rules tab annotation for each of the 8 detector rows**

Find the existing rules tab block. Each anomaly detector row currently has a hardcoded annotation string like `"promotes to Recommendation when confidence=High"` (or for SubagentSideEffect, `"promotes when subagent_hint co-occurs with another anomaly"`). Replace the hardcoded string with a dynamic value built from the configured threshold.

Pattern:

```rust
// before
"promotes to Recommendation when confidence=High"

// after
&format!(
    "promotes to Recommendation when confidence ≥ {}",
    state.config.anomaly.promote.cost_slope
)
```

Apply this pattern to each of the 8 detector rows. The SubagentSideEffect row's annotation remains semantically distinct (mentioning the correlation framing) but its threshold value also reads from config:

```rust
&format!(
    "promotes when subagent_hint co-occurs with another anomaly (≥ {})",
    state.config.anomaly.promote.subagent_side_effect
)
```

- [ ] **Step 4: Add (or update) Settings render tests**

If `src/ui/settings.rs` has rendering tests for the Parameters/Rules tabs, add assertions that:

- Parameters tab includes the literal `[anomaly.promote]` section header.
- Parameters tab includes a row labeled `cost_slope` with value `high` at default.
- Rules tab CostSlope row mentions `≥ high` at default config.

Pattern (mirroring an existing render test):

```rust
#[test]
fn parameters_tab_includes_anomaly_promote_section() {
    let state = /* fixture with default config */;
    let rendered = render_parameters_tab(&state);
    assert!(rendered.contains("[anomaly.promote]"), "section header missing");
    assert!(rendered.contains("subagent_side_effect"), "subagent_side_effect row missing");
    assert!(rendered.contains("medium"), "subagent_side_effect default value missing");
}

#[test]
fn rules_tab_anomaly_rows_use_configured_threshold() {
    let state = /* fixture with default config */;
    let rendered = render_rules_tab(&state);
    assert!(
        rendered.contains("≥ high"),
        "default high threshold annotation missing"
    );
    assert!(
        rendered.contains("≥ medium"),
        "default medium threshold for SubagentSideEffect missing"
    );
}
```

(If the existing tests use a different rendering helper, mirror that pattern. The render helper names — `render_parameters_tab`, `render_rules_tab` — are placeholders.)

- [ ] **Step 5: Run tests + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --lib ui::settings`
Expected: clean; ui::settings test count up by 2.

- [ ] **Step 6: Commit**

```bash
git add src/ui/settings.rs
git commit -m "phase 7 v3 (a+b): Settings Parameters [anomaly.promote] +8 rows + Rules annotation reads from config"
```

---

## Task 12: Integration test — `event_loop` pushes events for visible signals

**Files:**

- Modify: `tests/event_loop_integration.rs` (add new test)

- [ ] **Step 1: Read the existing v1.45.0 detectors integration test as a template**

Run: `grep -n "fn .*phase_7_v2\|fn .*detectors\|fn .*subagent" tests/event_loop_integration.rs | head -10`

Read the test body to learn: how the test fixture builds a pane source that emits the right deque samples, how it constructs `PolicyGates` / `AnomalyConfig`, how it drives one tick of `event_loop`, and how it inspects results.

- [ ] **Step 2: Add a new integration test**

At the bottom of `tests/event_loop_integration.rs`, add:

```rust
#[test]
fn event_loop_pushes_anomaly_events_for_visible_signals() {
    // Phase 7 v3 (v1.46.0): assert the ring buffer captures every
    // visible signal with its `promoted` bool reflecting the per-kind
    // gate result.
    use qmonster::app::anomaly_events_ring::AnomalyEventsRing;
    use qmonster::app::config::Config;
    use qmonster::domain::anomaly::AnomalyKind;

    // Build fixture pane source that produces a deque of samples
    // guaranteed to fire CostSlope at High confidence.
    let pane_source = /* fixture: uses the same pattern as the v1.45.0
                        detectors integration test in this file */;

    let mut config = Config::default();
    config.anomaly.enabled = true;
    config.anomaly.window_polls = 20;
    // Use defaults for promote thresholds (cost_slope = "high",
    // subagent_side_effect = "medium").

    let mut ring = AnomalyEventsRing::new();

    // Drive one event_loop tick. The exact tick-driver helper lives
    // in this same test file and is reused by the v1.45.0 detectors
    // integration test — copy that call, adding `&mut ring` as the
    // new ring-buffer parameter.
    drive_one_tick_with_ring(&pane_source, &config, &mut ring);

    // Assert the ring captured the High-confidence CostSlope as
    // promoted, and any non-promoted Medium signals as visible-but-
    // not-promoted.
    assert!(!ring.is_empty(), "ring buffer should have captured the signal");
    let cost_slope_events: Vec<_> = ring
        .iter()
        .filter(|e| e.kind == AnomalyKind::CostSlope)
        .collect();
    assert_eq!(cost_slope_events.len(), 1, "exactly one CostSlope event");
    assert!(
        cost_slope_events[0].promoted,
        "High CostSlope must be promoted at default High threshold"
    );
}
```

(The `pane_source` and `drive_one_tick_with_ring` helpers must be reused from or modeled after the existing v1.45.0 detectors integration test in this same file. If the existing tick driver doesn't accept a ring buffer, extend it — Task 6 already added the parameter to `event_loop`, so the test driver must thread it through.)

- [ ] **Step 3: Run the new integration test**

Run: `cargo test --test event_loop_integration event_loop_pushes_anomaly_events_for_visible_signals`
Expected: 1 passed.

- [ ] **Step 4: Run full test suite + fmt + clippy**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets`
Expected: clean; integration count = 58 (up by 1 since v1.45.0 baseline of 57).

- [ ] **Step 5: Commit**

```bash
git add tests/event_loop_integration.rs
git commit -m "phase 7 v3 (a+b): event_loop integration test — ring buffer captures visible signals"
```

---

## Task 13: Docs + version surfaces

**Files:**

- Modify: `docs/ai/UI_MANUAL.md`
- Modify: `docs/ai/ARCHITECTURE.md`
- Modify: `docs/ai/VALIDATION.md`
- Modify: `docs/ai/PROJECT_BRIEF.md`
- Modify: `config/qmonster.example.toml`
- Modify: `README.md`
- Modify: `VERSION.md`
- Modify: `package.json`

- [ ] **Step 1: Add `n` overlay subsection to `docs/ai/UI_MANUAL.md`**

Locate the existing `m` overlay subsection (search `grep -n "^## m\|^### m " docs/ai/UI_MANUAL.md`). After it, add a parallel `n` overlay subsection:

```markdown
### n — Anomaly Events overlay

Press `n` to open the Anomaly Events overlay. Shows the last 100
`AnomalySignal`s recorded this session, newest first, with columns:
Time / Pane / Kind / Conf / Promoted / Reason.

- `n` (or `Esc` / `q`): close.
- `Up` / `Down` (or `j` / `k`): scroll one row.
- Mouse wheel: scroll.
- Click outside the overlay: close.

Buffer is in-memory and resets on session restart. SQLite persistence
(Phase 7 v3 part c) is a separate follow-up.

`Promoted = yes` means the signal passed its per-kind
`[anomaly.promote]` confidence threshold and produced a
Recommendation. `Promoted = no` means the signal was visible (passed
the global `[anomaly] min_confidence` filter) but did not meet the
per-kind promotion threshold.
```

- [ ] **Step 2: Add the `[anomaly.promote]` config row in the existing anomaly section of UI_MANUAL.md**

Find where the existing `[anomaly] min_confidence` is documented. Below it, document the new `[anomaly.promote]` block:

```markdown
**`[anomaly.promote]`** — per-kind promotion threshold (v1.46.0+).
Distinct from `[anomaly] min_confidence` (which is a visibility
filter): a visible signal is gated AGAIN here before becoming a
Recommendation. Defaults: 7 kinds = `"high"`, `subagent_side_effect`
= `"medium"`. Lower a kind's threshold to make it noisier; raise it
to mute noisy detectors.
```

- [ ] **Step 3: Append v1.46.0 paragraph to `docs/ai/ARCHITECTURE.md`**

After the existing v1.45.0 paragraph, append:

```markdown
**v1.46.0 (Phase 7 v3 a+b):** Per-kind promotion gate replaces the
hardcoded `severity ≥ Warning` filter. `[anomaly.promote]` is a
table of 8 `AnomalyKind` thresholds; `promote_anomalies_to_recommendations`
checks `signal.confidence >= gates.promote_min_confidence(sig.kind)`.
`severity_for(confidence)` is unchanged (High → Warning, Medium/Low
→ Concern). New `AnomalyEventsRing` (capacity 100, in-memory only)
captures every visible signal with its gate result; `n` overlay
visualizes the ring chronologically. SQLite persistence for the ring
is the (c) sub-feature of Phase 7 v3, deferred to v1.47+.
```

- [ ] **Step 4: Add a row in `docs/ai/VALIDATION.md`**

Find the existing release-validation table. Add a new row:

```markdown
| v1.46.0 | Phase 7 v3 (a+b): per-kind promotion gate + n overlay | cargo fmt + clippy + test --all-targets (lib ~1144, integration 58); SBOM diff guard |
```

(Match the exact column shape of the existing rows.)

- [ ] **Step 5: Update `docs/ai/PROJECT_BRIEF.md`**

Update the Phase line to include `Phase 7 v3 (a+b)` and update the Date line to add a `v1.46.0` segment with brief description.

- [ ] **Step 6: Add `[anomaly.promote]` block to `config/qmonster.example.toml`**

Locate the existing `[anomaly]` block. After the last `[anomaly]` field, add:

```toml
# Phase 7 v3 (v1.46.0): per-kind promotion thresholds.
# Distinct from [anomaly] min_confidence (visibility filter):
# this gates promotion to Recommendation only.
# Allowed values: "low", "medium", "high".
# [anomaly.promote]
# identity_churn         = "high"
# error_burst            = "high"
# cache_discontinuity    = "high"
# cross_pane_edit_cluster = "high"
# cost_slope             = "high"
# token_slope            = "high"
# memory_growth          = "high"
# subagent_side_effect   = "medium"
```

- [ ] **Step 7: Bump version surfaces**

```bash
# README: update the "latest version" cell in the release table to v1.46.0
# (use Edit tool with old_string/new_string for the specific row)

# VERSION.md: replace contents with `v1.46.0`
# (use Edit tool, replace `v1.45.0` with `v1.46.0`)

# package.json: change `"version": "1.45.0"` to `"version": "1.46.0"`
# (use Edit tool)
```

- [ ] **Step 8: Run full validation**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets`
Expected: clean; lib ~1144, integration 58.

- [ ] **Step 9: Commit**

```bash
git add docs/ai/UI_MANUAL.md docs/ai/ARCHITECTURE.md docs/ai/VALIDATION.md docs/ai/PROJECT_BRIEF.md config/qmonster.example.toml README.md VERSION.md package.json
git commit -m "phase 7 v3 (a+b): docs (UI_MANUAL n overlay subsection, ARCHITECTURE v1.46.0 paragraph, VALIDATION row, example.toml + version surfaces)"
```

---

## Task 14: Release ledger sync + tag

**Files:**

- Modify: `mission.yaml`
- Modify: `mission-history.yaml`
- Modify: `.mission/CURRENT_STATE.md`
- New: annotated git tag `v1.46.0`

- [ ] **Step 1: Update `.mission/CURRENT_STATE.md`**

Replace the v1.45.0 banner with a v1.46.0 banner. Mirror the v1.45.0 release-ledger-sync structure:

- Update `_Last updated:` line.
- Update Mission section (Title, version surfaces, branch/tag, release publication state — note v1.46 CI is pending).
- Replace `v1.45.0 Feature State` section with a `v1.46.0 Feature State` section listing the 11 enumerated changes from the spec.
- Update Latest Release Notes section.
- Reset Post-tag polish section to "no genuinely-post-v1.46.0 work has landed yet".
- Update Known External State to add v1.46.0 to the all-published list (note: pending CI verification).
- Update Active Follow-Ups: remove "Phase 7 v3" line, replace with "Phase 7 v3 (c) — SQLite persistence for anomaly history (deferred from v1.46.0)".

- [ ] **Step 2: Append v1.46.0 entry to `mission-history.yaml`**

Mirror the v1.45.0 timeline entry (the entry around `mission-history.yaml:17278`). Sequence number follows v1.45.0's. Fields:

- `version: "1.46.0"`
- `summary`, `intent_ko` describing Phase 7 v3 (a+b)
- `triggered_by`: "Operator scoped Phase 7 v3 (a+b) as a 14-task implementation sequence (Tasks 1-13 = impl commits, Task 14 = ledger sync)."
- `changes.added` lists the new files + new types + new functions.
- `changes.modified` lists modified files.
- `observable_effects` describes per-kind promote, n overlay, defaults.
- `related_commits` will be filled with the SHAs from Tasks 1-13.

- [ ] **Step 3: Update `mission.yaml`**

In `execution_hints.topology`, replace v1.45.0 description with v1.46.0 description (Phase 7 v3 a+b shipped, c deferred).
In `execution_hints.suggested_approach`, update to reflect v1.46.0 baseline.
In `budget_hint`, update `priority` to `"post-v1.46.0 Phase 7 v3 (c) — SQLite persistence for anomaly history"` and `scope` to summarize v1.46.0.
In `budget_hint.followups`, replace the v3-as-a-whole entry with `Phase 7 v3 (c) — SQLite persistence for anomaly history`.

- [ ] **Step 4: Run validation gates**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args && cargo test --all-targets && git diff --check`
Expected: clean.

- [ ] **Step 5: Commit the ledger sync**

```bash
git add mission.yaml mission-history.yaml .mission/CURRENT_STATE.md
git commit -m "v1.46.0 release ledger sync"
```

- [ ] **Step 6: Create annotated git tag**

```bash
git tag -a v1.46.0 -m "v1.46.0 — Phase 7 v3 (a+b): per-kind promotion gate + n overlay"
```

- [ ] **Step 7: Verify tag points at the ledger sync commit**

Run: `git rev-parse v1.46.0^{commit}` and `git log --oneline -1`
Expected: same SHA.

- [ ] **Step 8: STOP — do NOT push**

The push + publication-verification step is operator-controlled (matches v1.42/v1.43/v1.44/v1.45 pattern). The implementation cycle is complete at this commit. The orchestrator (you, the executing AI) reports back to the operator that v1.46.0 is locally tagged and awaits push.

---

## Validation summary (cumulative across all tasks)

After Task 14, the cumulative state should be:

- **Lib tests:** 1123 (v1.45 baseline) → ~1162 (+~39).
  - +1 (Task 1: AnomalyEvent smoke)
  - +3 (Task 2: AnomalyPromoteConfig defaults + AnomalyConfig.promote default + omit-section round-trip)
  - +3 (Task 3: per-kind helper parametric + defaults match spec + from_inputs parses promote)
  - +5 (Task 4: AnomalyEventsRing)
  - +6 (Task 5: promote-gate scenarios)
  - +5 (Task 7: AnomalyOverlay state)
  - +6 (Task 7: key handler)
  - +5 (Task 8: mouse handler)
  - +3 (Task 9: render)
  - +2 (Task 11: Settings)
  - Note: actual count may differ slightly depending on test consolidation; any test count between +30 and +45 is acceptable.
- **Integration tests:** 57 → 58 (+1 from Task 12).
- **`cargo fmt`, `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`, `git diff --check`** clean.
- **Local tag:** `v1.46.0` annotated, points at the release ledger sync commit.
- **Push + publication:** operator-controlled follow-up.

---

**End of plan.**
