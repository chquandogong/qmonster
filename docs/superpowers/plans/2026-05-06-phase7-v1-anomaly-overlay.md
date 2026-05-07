# Phase 7 v1: anomaly observation surface — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open the original token-optimization plan's "이상 징후 분류" loop by combining four already-collected per-pane history streams (D2 identity, F-9 error hints, F-3 + F-7b cache, F-8 cross-pane file findings) into a single per-pane `Vec<AnomalySignal>` that the m (Metrics) overlay renders as one new row per pane card. Observation only — no Recommendation, no Notify, no audit event, no SQLite table.

**Architecture:** Pure addition. New domain types in `src/domain/anomaly.rs`; new pure rule module `src/policy/rules/anomaly.rs` with four detectors + `eval_anomalies` orchestrator; new `AnomalyHistory` + `last_emitted_at` per-pane state in `Context` (lifecycle-evicted like Phase H's `auto_snapshot_dedup`); event-loop pushes observations into the history each tick and calls `eval_anomalies` after the existing rule pipeline; result rides on a new `PaneReport.anomalies: Vec<AnomalySignal>` field. m overlay renders one new line per pane card. No code in `policy/` outside the new module. No new audit kinds, no `Recommendation` emission, no `Notify`. Reference spec: `docs/superpowers/specs/2026-05-06-phase7-v1-anomaly-overlay-design.md`.

**Tech Stack:** Rust 2021, `cargo test --lib`, `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`, `cargo fmt --all --check`.

---

## File Map

- `src/domain/anomaly.rs` (new) — `AnomalySignal`, `AnomalyKind`, `AnomalyConfidence`, `AnomalyEvidence` types + `mod.rs` re-export.
- `src/app/config.rs` — `AnomalyConfig` struct + `[anomaly]` section on `QmonsterConfig`.
- `src/policy/gates.rs` — six `anomaly_*` fields, populated in `from_inputs(PolicyGateInputs<'_>)`.
- `src/app/bootstrap.rs` — `Context.anomaly_history: HashMap<String, AnomalyHistory>` + `Context.anomaly_dedup: HashMap<(String, AnomalyKind), Option<u64>>`, both initialized empty in `Context::new`. Lifecycle eviction in `event_loop.rs` mirrors `identity_history` / `recent_error_observations` / `auto_snapshot_dedup`.
- `src/policy/rules/anomaly.rs` (new) — `AnomalyHistory` struct + four `detect_*` pure functions + `eval_anomalies` orchestrator + edge-triggered dedup helper.
- `src/policy/rules/mod.rs` — register the new module.
- `src/app/event_loop.rs` — push current observations into `AnomalyHistory` each tick (after `policy.evaluate`); call `eval_anomalies` next; attach result to `PaneReport.anomalies`. Existing rule output flow unchanged.
- `src/app/event_loop.rs::PaneReport` — add `pub anomalies: Vec<AnomalySignal>` field.
- `src/ui/metrics.rs` — render `ANOMALIES <n> <kind>:<conf>[, …]` line in pane card body when `!anomalies.is_empty()`.
- `src/ui/settings.rs` — `Parameters` tab: new `[anomaly]` group; `Rules` tab: four anomaly rows; `Badges` tab: ANOMALIES description.
- `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml` — Phase 7 v1 surface.
- `package.json`, `VERSION.md`, `README.md`, `mission.yaml`, `mission-history.yaml`, `.mission/CURRENT_STATE.md`, `docs/ai/PROJECT_BRIEF.md`, `docs/ai/ARCHITECTURE.md` — release ledger sync to v1.43.0 (final task).

## Validation Gates (run after every task)

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

If any gate fails, fix before committing the task.

Baseline lib test count entering this plan: **1049** (post-v1.42.0 ledger). Each task notes its expected delta.

---

## Task 1: Domain types — `src/domain/anomaly.rs`

**Files:**

- Create: `src/domain/anomaly.rs`
- Modify: `src/domain/mod.rs` (add `pub mod anomaly;`)

- [ ] **Step 1: Create the new module**

```rust
//! Phase 7 v1 (v1.43.0): per-pane anomaly observation types. Pure
//! data only — `Vec<AnomalySignal>` rides on `PaneReport`, the m
//! overlay reads it. v1 emits no Recommendation, no Notify, no
//! audit event; severity is informational metadata, not a gate.

use crate::domain::origin::SourceKind;

/// One detected anomaly. Built by a `policy::rules::anomaly::detect_*`
/// function and (after edge-triggered dedup) attached to
/// `PaneReport.anomalies`. Observation surface only — no ID, no
/// recommendation_id, no audit linkage.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalySignal {
    pub kind: AnomalyKind,
    pub confidence: AnomalyConfidence,
    pub severity: Severity,
    pub evidence: Vec<AnomalyEvidence>,
    /// The rolling-window length the detector inspected (`window_polls`).
    pub window_polls: usize,
    /// Unix seconds when the detector first observed this anomaly in
    /// the current edge-triggered window.
    pub detected_at: u64,
}

/// Phase 7 v1 ships four detector kinds; v2 adds the remaining four
/// (`CostSlope`, `TokenSlope`, `MemoryGrowth`, `SubagentSideEffect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnomalyKind {
    IdentityChurn,
    ErrorBurst,
    CacheDiscontinuity,
    CrossPaneEditCluster,
}

impl AnomalyKind {
    pub fn label(self) -> &'static str {
        match self {
            AnomalyKind::IdentityChurn => "IdentityChurn",
            AnomalyKind::ErrorBurst => "ErrorBurst",
            AnomalyKind::CacheDiscontinuity => "CacheDiscontinuity",
            AnomalyKind::CrossPaneEditCluster => "CrossPaneEditCluster",
        }
    }
}

/// Detector confidence. Operator filters via
/// `[anomaly] min_confidence`. Detector mapping is documented in
/// `policy::rules::anomaly`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnomalyConfidence {
    Low,
    Medium,
    High,
}

impl AnomalyConfidence {
    pub fn label(self) -> &'static str {
        match self {
            AnomalyConfidence::Low => "low",
            AnomalyConfidence::Medium => "medium",
            AnomalyConfidence::High => "high",
        }
    }
}

/// Per-anomaly evidence row — what changed, by how much, over how
/// many samples, and which `SourceKind` saw it. Never embeds raw
/// pane-tail text.
#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyEvidence {
    pub metric_name: &'static str,
    pub before: String,
    pub after: String,
    pub sample_count: usize,
    pub source_kind: SourceKind,
}

use crate::domain::recommendation::Severity;
```

Append `pub mod anomaly;` to `src/domain/mod.rs` next to the other `pub mod` declarations (alphabetical).

- [ ] **Step 2: Sanity tests** — append to a `#[cfg(test)] mod tests` in `src/domain/anomaly.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_labels_are_stable() {
        assert_eq!(AnomalyKind::IdentityChurn.label(), "IdentityChurn");
        assert_eq!(AnomalyKind::ErrorBurst.label(), "ErrorBurst");
        assert_eq!(AnomalyKind::CacheDiscontinuity.label(), "CacheDiscontinuity");
        assert_eq!(AnomalyKind::CrossPaneEditCluster.label(), "CrossPaneEditCluster");
    }

    #[test]
    fn confidence_orders_low_lt_medium_lt_high() {
        assert!(AnomalyConfidence::Low < AnomalyConfidence::Medium);
        assert!(AnomalyConfidence::Medium < AnomalyConfidence::High);
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test --lib anomaly
```

Expected: 2 PASS.

- [ ] **Step 4: Validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 1051 lib tests pass (1049 + 2).

- [ ] **Step 5: Commit**

```bash
git add src/domain/anomaly.rs src/domain/mod.rs
git commit -m "phase 7 v1: AnomalySignal/AnomalyKind/AnomalyConfidence/AnomalyEvidence"
```

---

## Task 2: `[anomaly]` config section — `AnomalyConfig`

**Files:**

- Modify: `src/app/config.rs` — add `AnomalyConfig` struct + `[anomaly]` field on `QmonsterConfig`.

- [ ] **Step 1: Write the failing tests** — append to the existing `#[cfg(test)] mod tests` in `src/app/config.rs`:

```rust
#[test]
fn anomaly_config_disabled_by_default() {
    let c = AnomalyConfig::default();
    assert!(!c.enabled);
    assert_eq!(c.window_polls, 20);
    assert_eq!(c.min_confidence, "medium");
    assert_eq!(c.identity_churn_min_flips, 3);
    assert!((c.error_burst_threshold - 0.5).abs() < f32::EPSILON);
    assert!((c.cache_discontinuity_drop - 0.30).abs() < f32::EPSILON);
    assert_eq!(c.cross_pane_cluster_min_findings, 3);
}

#[test]
fn anomaly_config_loads_from_toml_with_partial_overrides() {
    let toml = r#"
[anomaly]
enabled = true
window_polls = 30
"#;
    let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
    assert!(cfg.anomaly.enabled);
    assert_eq!(cfg.anomaly.window_polls, 30);
    assert_eq!(cfg.anomaly.min_confidence, "medium"); // default kept
    assert_eq!(cfg.anomaly.identity_churn_min_flips, 3); // default kept
}

#[test]
fn anomaly_config_missing_section_disables_layer() {
    let cfg: QmonsterConfig = toml::from_str("").expect("parse");
    assert!(!cfg.anomaly.enabled);
}
```

- [ ] **Step 2: Run tests to verify fail**

```bash
cargo test --lib anomaly_config
```

Expected: 3 FAIL with `error[E0433]: failed to resolve: use of undeclared type 'AnomalyConfig'` or `error[E0609]: no field 'anomaly' on type 'QmonsterConfig'`.

- [ ] **Step 3: Add the struct** — insert after the existing `ResetConfig` block (around line 254-283):

```rust
/// Phase 7 v1 (v1.43.0): operator-tunable thresholds for the
/// anomaly observation layer in `src/policy/rules/anomaly.rs`.
/// Defaults are deliberately conservative — the entire layer is
/// off by default, the rolling window is 20 polls, and each
/// detector's threshold is set so a brief blip does not fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnomalyConfig {
    /// Master switch. When false, `eval_anomalies` returns an empty
    /// Vec on every tick; UI rendering is a no-op.
    pub enabled: bool,
    /// Rolling-window length used by every detector. 20 polls at the
    /// default 5-second tick is a 100-second window — long enough to
    /// catch a real burst, short enough to clear quickly.
    pub window_polls: usize,
    /// Detector output below this confidence is filtered before
    /// reaching `PaneReport`. Values: `"low"`, `"medium"`, `"high"`.
    pub min_confidence: String,
    /// IdentityChurn fires when (provider, current_path) flips
    /// `>= identity_churn_min_flips` times inside `window_polls`.
    pub identity_churn_min_flips: usize,
    /// ErrorBurst fires when error rate over `window_polls` is
    /// `>= error_burst_threshold`.
    pub error_burst_threshold: f32,
    /// CacheDiscontinuity fires when `cache_hit_ratio` drops by
    /// at least this much across the window (or when F-7b fires
    /// twice in the window — alternate trigger).
    pub cache_discontinuity_drop: f32,
    /// CrossPaneEditCluster fires when at least this many
    /// `ConcurrentFileEdit` findings target the same path inside
    /// the window.
    pub cross_pane_cluster_min_findings: usize,
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            window_polls: 20,
            min_confidence: "medium".to_string(),
            identity_churn_min_flips: 3,
            error_burst_threshold: 0.5,
            cache_discontinuity_drop: 0.30,
            cross_pane_cluster_min_findings: 3,
        }
    }
}
```

Add `pub anomaly: AnomalyConfig,` to `QmonsterConfig` next to the other `[*]Config` fields, and set `anomaly: AnomalyConfig::default(),` in the `Default for QmonsterConfig` impl.

- [ ] **Step 4: Run tests**

```bash
cargo test --lib anomaly_config
```

Expected: 3 PASS.

- [ ] **Step 5: Validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 1054 lib tests pass (1051 + 3).

- [ ] **Step 6: Commit**

```bash
git add src/app/config.rs
git commit -m "phase 7 v1: AnomalyConfig + [anomaly] section"
```

---

## Task 3: PolicyGates `anomaly_*` fields

**Files:**

- Modify: `src/policy/gates.rs` — six `anomaly_*` fields + `Default` + `from_inputs` + test fixture default block.

- [ ] **Step 1: Write the failing test** — append to the gates test module:

```rust
#[test]
fn from_inputs_propagates_anomaly_config() {
    let mut config = QmonsterConfig::default();
    config.anomaly.enabled = true;
    config.anomaly.window_polls = 25;
    config.anomaly.identity_churn_min_flips = 4;
    config.anomaly.error_burst_threshold = 0.7;
    config.anomaly.cache_discontinuity_drop = 0.40;
    config.anomaly.cross_pane_cluster_min_findings = 5;

    // Use the same `from_inputs` shape the existing `from_inputs_*`
    // tests use; the per-field shape may use spread or explicit fields.
    let gates = make_gates_from(&config);  // helper if it exists; otherwise inline
    assert!(gates.anomaly_enabled);
    assert_eq!(gates.anomaly_window_polls, 25);
    assert_eq!(gates.anomaly_min_confidence, AnomalyConfidence::Medium);
    assert_eq!(gates.anomaly_identity_churn_min_flips, 4);
    assert!((gates.anomaly_error_burst_threshold - 0.7).abs() < f32::EPSILON);
    assert!((gates.anomaly_cache_discontinuity_drop - 0.40).abs() < f32::EPSILON);
    assert_eq!(gates.anomaly_cross_pane_cluster_min_findings, 5);
}
```

If `make_gates_from` doesn't exist, mirror the literal pattern from `from_inputs_propagates_reset_auto_snapshot` (Phase H Task 2) which constructs `PolicyGateInputs` directly with `&config.actions`, `&config.ux`, `&config.quota`, `&config.security`, `&config.cache`, `&config.reset`, `&config.profile_switch`, plus a NEW `&config.anomaly` field — see Step 3.

- [ ] **Step 2: Run test to verify fail**

```bash
cargo test --lib from_inputs_propagates_anomaly_config
```

Expected: FAIL with `error[E0609]: no field 'anomaly_enabled' on type 'PolicyGates'`.

- [ ] **Step 3: Add fields to `PolicyGates`** — append after the existing `reset_auto_snapshot` field:

```rust
    /// Phase 7 v1 (v1.43.0): when `false`, `eval_anomalies` returns
    /// an empty Vec immediately. Mirrors `[anomaly] enabled`.
    pub anomaly_enabled: bool,
    /// Rolling-window length for every detector.
    pub anomaly_window_polls: usize,
    /// Minimum confidence for a detector signal to reach `PaneReport`.
    pub anomaly_min_confidence: crate::domain::anomaly::AnomalyConfidence,
    /// IdentityChurn detector: minimum (provider, path) flips inside
    /// `anomaly_window_polls`.
    pub anomaly_identity_churn_min_flips: usize,
    /// ErrorBurst detector: minimum error rate over the window.
    pub anomaly_error_burst_threshold: f32,
    /// CacheDiscontinuity detector: minimum cache_hit_ratio drop.
    pub anomaly_cache_discontinuity_drop: f32,
    /// CrossPaneEditCluster detector: minimum same-path findings.
    pub anomaly_cross_pane_cluster_min_findings: usize,
```

- [ ] **Step 4: Update `Default for PolicyGates`** — append:

```rust
            anomaly_enabled: false,
            anomaly_window_polls: 20,
            anomaly_min_confidence: crate::domain::anomaly::AnomalyConfidence::Medium,
            anomaly_identity_churn_min_flips: 3,
            anomaly_error_burst_threshold: 0.5,
            anomaly_cache_discontinuity_drop: 0.30,
            anomaly_cross_pane_cluster_min_findings: 3,
```

- [ ] **Step 5: Update `PolicyGateInputs` to carry `anomaly`** — add the new field to `PolicyGateInputs<'a>`:

```rust
pub struct PolicyGateInputs<'a> {
    pub actions: &'a ActionsConfig,
    pub ux: &'a UxConfig,
    pub quota: &'a QuotaConfig,
    pub security: &'a SecurityConfig,
    pub cache: &'a CacheConfig,
    pub reset: &'a ResetConfig,
    pub profile_switch: &'a ProfileSwitchConfig,
    pub anomaly: &'a AnomalyConfig,  // NEW
    pub provider: Provider,
    pub confidence: IdentityConfidence,
}
```

Update `from_inputs` to destructure `anomaly` and populate the new gate fields. Use a helper that maps the `min_confidence: String` to `AnomalyConfidence`:

```rust
fn parse_min_confidence(s: &str) -> AnomalyConfidence {
    match s {
        "low" => AnomalyConfidence::Low,
        "high" => AnomalyConfidence::High,
        _ => AnomalyConfidence::Medium, // fallback for "medium" or any unknown string
    }
}
```

- [ ] **Step 6: Update every existing call site of `PolicyGates::from_inputs`** — search and add `anomaly: &config.anomaly,` to each `PolicyGateInputs { ... }` literal:

```bash
grep -rn "PolicyGateInputs {" src/ tests/
```

Update each match. The most critical one is in `src/app/event_loop.rs` near line 196.

- [ ] **Step 7: Update the existing `PolicyGates` test-fixture default block** — search for the long literal that hand-rolls a `PolicyGates` for tests (around line 321 in the file) and add the seven new `anomaly_*` lines.

- [ ] **Step 8: Run tests**

```bash
cargo test --lib gates
```

Expected: all gates tests pass, including `from_inputs_propagates_anomaly_config`.

- [ ] **Step 9: Validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 1055 lib tests pass (1054 + 1).

- [ ] **Step 10: Commit**

```bash
git add src/policy/gates.rs src/app/event_loop.rs
git commit -m "phase 7 v1: PolicyGates anomaly_* fields"
```

---

## Task 4: `AnomalyHistory` + Context plumbing

**Files:**

- Create part of: `src/policy/rules/anomaly.rs` — declare `AnomalyHistory` here so it lives next to the detectors.
- Modify: `src/policy/rules/mod.rs` — `pub mod anomaly;`.
- Modify: `src/app/bootstrap.rs` — add `anomaly_history` and `anomaly_dedup` fields to `Context`, initialize, plus lifecycle eviction in `event_loop.rs`.

- [ ] **Step 1: Skeleton `src/policy/rules/anomaly.rs`** — create the file with:

```rust
//! Phase 7 v1 (v1.43.0): per-pane anomaly observation rules.
//! Pure detection over `AnomalyHistory`; the orchestrator
//! `eval_anomalies` applies edge-triggered dedup and minimum
//! confidence filtering.
//!
//! v1 ships four kinds. v2 adds CostSlope, TokenSlope,
//! MemoryGrowth, SubagentSideEffect.

use std::collections::VecDeque;

use crate::domain::anomaly::{AnomalyKind, AnomalySignal};

/// Per-pane rolling history the detectors consume. The event loop
/// pushes one observation per tick (newest pushed to front, trim
/// to `window_polls`). Each field corresponds to one detector.
#[derive(Debug, Default, Clone)]
pub struct AnomalyHistory {
    /// IdentityChurn input: (provider_label, current_path) snapshot
    /// per tick. Most-recent-first.
    pub identity_snapshots: VecDeque<(String, String)>,
    /// ErrorBurst input: `signals.error_hint` per tick. Most-recent-first.
    pub error_hints: VecDeque<bool>,
    /// CacheDiscontinuity input #1: `cache_hit_ratio` per tick when
    /// the signal is populated, else None. Most-recent-first.
    pub cache_hit_ratios: VecDeque<Option<f32>>,
    /// CacheDiscontinuity input #2: did F-7b fire this tick?
    /// Determined by the caller from the recommendation slice.
    pub cache_drift_fires: VecDeque<bool>,
    /// CrossPaneEditCluster input: paths of `ConcurrentFileEdit`
    /// findings emitted this tick, if any. Most-recent-first; each
    /// entry is the set of paths that fired together.
    pub cross_pane_edit_paths: VecDeque<Vec<String>>,
}

impl AnomalyHistory {
    /// Trim every deque to `cap`. Called after each push.
    pub fn trim(&mut self, cap: usize) {
        while self.identity_snapshots.len() > cap { self.identity_snapshots.pop_back(); }
        while self.error_hints.len() > cap { self.error_hints.pop_back(); }
        while self.cache_hit_ratios.len() > cap { self.cache_hit_ratios.pop_back(); }
        while self.cache_drift_fires.len() > cap { self.cache_drift_fires.pop_back(); }
        while self.cross_pane_edit_paths.len() > cap { self.cross_pane_edit_paths.pop_back(); }
    }
}

/// Stub — Tasks 5–9 fill in the detectors and orchestrator.
#[allow(dead_code)]
pub fn eval_anomalies() -> Vec<AnomalySignal> {
    Vec::new()
}
```

Append `pub mod anomaly;` to `src/policy/rules/mod.rs` (alphabetical).

- [ ] **Step 2: Add Context fields** — in `src/app/bootstrap.rs`, after `auto_snapshot_dedup`, insert:

```rust
    /// Phase 7 v1 (v1.43.0): per-pane rolling input history for the
    /// anomaly detectors in `policy::rules::anomaly`. Trimmed to
    /// `gates.anomaly_window_polls` after each push. Evicted on
    /// `PaneLifecycleEvent::{BecameDead, Reappeared}` so a re-spawned
    /// pane starts fresh.
    pub anomaly_history:
        std::collections::HashMap<String, crate::policy::rules::anomaly::AnomalyHistory>,
    /// Phase 7 v1: per-(pane_id, AnomalyKind) edge-triggered dedup.
    /// `Some(unix_seconds)` while the kind is currently considered
    /// active in the rolling window; `None` when the detector has
    /// last returned None and the kind has rearmed for fresh emission.
    pub anomaly_dedup:
        std::collections::HashMap<(String, crate::domain::anomaly::AnomalyKind), Option<u64>>,
```

In `Context::new`, append:

```rust
            anomaly_history: std::collections::HashMap::new(),
            anomaly_dedup: std::collections::HashMap::new(),
```

- [ ] **Step 3: Lifecycle eviction** — in `src/app/event_loop.rs` inside the `if matches!(lc, L::BecameDead | L::Reappeared)` block (lines ~114–144), append after the existing `auto_snapshot_dedup.remove`:

```rust
ctx.anomaly_history.remove(&pane.pane_id);
ctx.anomaly_dedup.retain(|(pid, _kind), _v| pid != &pane.pane_id);
```

- [ ] **Step 4: Sanity test** — append to `src/app/bootstrap.rs` test module:

```rust
#[test]
fn context_new_initializes_empty_anomaly_state() {
    use crate::policy::rules::anomaly::AnomalyHistory;
    use crate::domain::anomaly::AnomalyKind;
    use std::collections::HashMap;
    let history: HashMap<String, AnomalyHistory> = HashMap::new();
    let dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();
    assert!(history.is_empty());
    assert!(dedup.is_empty());
}
```

(Same fallback pattern as Phase H Task 6 — verifies the type signatures import cleanly.)

- [ ] **Step 5: Run validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 1056 lib tests pass (1055 + 1).

- [ ] **Step 6: Commit**

```bash
git add src/policy/rules/anomaly.rs src/policy/rules/mod.rs src/app/bootstrap.rs src/app/event_loop.rs
git commit -m "phase 7 v1: AnomalyHistory + Context.anomaly_history/anomaly_dedup"
```

---

## Task 5: `detect_identity_churn`

**Files:**

- Modify: `src/policy/rules/anomaly.rs`.

- [ ] **Step 1: Write tests** — append to a `#[cfg(test)] mod tests` block in `src/policy/rules/anomaly.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};

    fn snap(provider: &str, path: &str) -> (String, String) {
        (provider.to_string(), path.to_string())
    }

    fn churn_history(snapshots: Vec<(&str, &str)>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for (p, path) in snapshots.into_iter().rev() {
            h.identity_snapshots.push_front(snap(p, path));
        }
        h
    }

    #[test]
    fn detect_identity_churn_fires_at_threshold() {
        // 3 flips in window: claude→codex→claude→codex (4 distinct snapshots, 3 transitions).
        let h = churn_history(vec![
            ("claude", "/r"), ("codex", "/r"), ("claude", "/r"), ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000);
        let s = sig.expect("should fire at 3 flips");
        assert_eq!(s.kind, AnomalyKind::IdentityChurn);
        assert!(matches!(s.confidence, AnomalyConfidence::Medium | AnomalyConfidence::High));
        assert_eq!(s.evidence.len(), 1);
        assert_eq!(s.evidence[0].metric_name, "provider_path");
        assert_eq!(s.evidence[0].sample_count, 3);
    }

    #[test]
    fn detect_identity_churn_below_threshold_returns_none() {
        let h = churn_history(vec![("claude", "/r"), ("codex", "/r")]); // 1 flip
        assert!(detect_identity_churn(&h, 20, 3, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_identity_churn_empty_history_returns_none() {
        let h = AnomalyHistory::default();
        assert!(detect_identity_churn(&h, 20, 3, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_identity_churn_high_confidence_when_far_above_threshold() {
        // 5 flips when threshold is 3 → High confidence
        let h = churn_history(vec![
            ("claude", "/r"), ("codex", "/r"), ("claude", "/r"),
            ("codex", "/r"), ("claude", "/r"), ("codex", "/r"),
        ]);
        let sig = detect_identity_churn(&h, 20, 3, 1_700_000_000).unwrap();
        assert_eq!(sig.confidence, AnomalyConfidence::High);
    }
}
```

- [ ] **Step 2: Implement `detect_identity_churn`** — insert in `src/policy/rules/anomaly.rs` between the `AnomalyHistory` impl block and the `eval_anomalies` stub:

```rust
use crate::domain::anomaly::{AnomalyConfidence, AnomalyEvidence};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;

/// IdentityChurn: count `(provider, path)` flips inside the window.
/// `min_flips` is the threshold; below it, return `None`. Confidence
/// is `Medium` at exactly threshold, `High` at 1.5x threshold or more.
pub fn detect_identity_churn(
    history: &AnomalyHistory,
    window_polls: usize,
    min_flips: usize,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.identity_snapshots.len() < 2 {
        return None;
    }
    let window_slice: Vec<&(String, String)> = history
        .identity_snapshots
        .iter()
        .take(window_polls)
        .collect();
    let mut flips = 0usize;
    let mut last_change_after: Option<&(String, String)> = None;
    let mut last_change_before: Option<&(String, String)> = None;
    for pair in window_slice.windows(2) {
        if pair[0] != pair[1] {
            flips += 1;
            last_change_after = Some(pair[0]);
            last_change_before = Some(pair[1]);
        }
    }
    if flips < min_flips {
        return None;
    }
    let confidence = if flips as f32 >= 1.5 * min_flips as f32 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    let before = last_change_before
        .map(|(p, path)| format!("{p}:{path}"))
        .unwrap_or_default();
    let after = last_change_after
        .map(|(p, path)| format!("{p}:{path}"))
        .unwrap_or_default();
    Some(AnomalySignal {
        kind: AnomalyKind::IdentityChurn,
        confidence,
        severity: Severity::Concern,
        evidence: vec![AnomalyEvidence {
            metric_name: "provider_path",
            before,
            after,
            sample_count: flips,
            source_kind: SourceKind::Estimated,
        }],
        window_polls,
        detected_at: now_unix_seconds,
    })
}
```

- [ ] **Step 3: Run tests + gates**

```bash
cargo test --lib detect_identity_churn
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 4 detector tests PASS, full lib at 1060 (1056 + 4).

- [ ] **Step 4: Commit**

```bash
git add src/policy/rules/anomaly.rs
git commit -m "phase 7 v1: detect_identity_churn detector + 4 tests"
```

---

## Task 6: `detect_error_burst`

**Files:**

- Modify: `src/policy/rules/anomaly.rs`.

- [ ] **Step 1: Tests**

```rust
    fn errors_history(bools: Vec<bool>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for b in bools.into_iter().rev() {
            h.error_hints.push_front(b);
        }
        h
    }

    #[test]
    fn detect_error_burst_fires_above_threshold() {
        // 12 errors in 20 ticks → 0.60 ≥ 0.5
        let mut bools = vec![true; 12];
        bools.extend(vec![false; 8]);
        let h = errors_history(bools);
        let sig = detect_error_burst(&h, 20, 0.5, 1_700_000_000);
        let s = sig.expect("should fire at 60% > 50%");
        assert_eq!(s.kind, AnomalyKind::ErrorBurst);
        assert_eq!(s.evidence[0].metric_name, "error_rate");
        assert_eq!(s.evidence[0].sample_count, 20);
    }

    #[test]
    fn detect_error_burst_below_threshold() {
        let mut bools = vec![true; 6];
        bools.extend(vec![false; 14]);
        let h = errors_history(bools);
        assert!(detect_error_burst(&h, 20, 0.5, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_error_burst_insufficient_samples() {
        let h = errors_history(vec![true, true, true]); // only 3 samples, window 20
        assert!(detect_error_burst(&h, 20, 0.5, 1_700_000_000).is_none());
    }
```

- [ ] **Step 2: Implementation** — append to `src/policy/rules/anomaly.rs`:

```rust
/// ErrorBurst: error rate over the most recent `window_polls` of
/// `signals.error_hint`. Returns None when the buffer is shorter
/// than `window_polls` (no false-positive on cold start).
pub fn detect_error_burst(
    history: &AnomalyHistory,
    window_polls: usize,
    threshold: f32,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    if history.error_hints.len() < window_polls {
        return None;
    }
    let window: Vec<bool> = history
        .error_hints
        .iter()
        .take(window_polls)
        .copied()
        .collect();
    let count = window.iter().filter(|b| **b).count();
    let rate = count as f32 / window_polls as f32;
    if rate < threshold {
        return None;
    }
    let confidence = if rate >= threshold * 1.5 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    // before = error rate at oldest end of window (last bucket); we approximate
    // by taking the second-half average vs first-half average for evidence.
    let half = window_polls / 2;
    let half_count = window[..half].iter().filter(|b| **b).count();
    let other_count = window[half..].iter().filter(|b| **b).count();
    let half_rate = if half == 0 { 0.0 } else { half_count as f32 / half as f32 };
    let other_rate = if window_polls - half == 0 {
        0.0
    } else {
        other_count as f32 / (window_polls - half) as f32
    };
    Some(AnomalySignal {
        kind: AnomalyKind::ErrorBurst,
        confidence,
        severity: Severity::Concern,
        evidence: vec![AnomalyEvidence {
            metric_name: "error_rate",
            before: format!("{other_rate:.2}"),
            after: format!("{half_rate:.2}"),
            sample_count: window_polls,
            source_kind: SourceKind::Estimated,
        }],
        window_polls,
        detected_at: now_unix_seconds,
    })
}
```

- [ ] **Step 3: Run tests + gates**

```bash
cargo test --lib detect_error_burst
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 3 new tests PASS, full lib at 1063 (1060 + 3).

- [ ] **Step 4: Commit**

```bash
git add src/policy/rules/anomaly.rs
git commit -m "phase 7 v1: detect_error_burst detector + 3 tests"
```

---

## Task 7: `detect_cache_discontinuity`

**Files:**

- Modify: `src/policy/rules/anomaly.rs`.

- [ ] **Step 1: Tests**

```rust
    fn cache_history(ratios: Vec<Option<f32>>, drift_fires: Vec<bool>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for r in ratios.into_iter().rev() {
            h.cache_hit_ratios.push_front(r);
        }
        for d in drift_fires.into_iter().rev() {
            h.cache_drift_fires.push_front(d);
        }
        h
    }

    #[test]
    fn detect_cache_discontinuity_fires_on_30pp_drop() {
        // hit ratio 0.85 (oldest in window) → 0.41 (newest) → drop = 0.44 ≥ 0.30
        let mut ratios = vec![Some(0.41); 10];
        ratios.extend(vec![Some(0.85); 10]);
        let h = cache_history(ratios, vec![false; 20]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000);
        let s = sig.expect("0.44 drop should fire");
        assert_eq!(s.kind, AnomalyKind::CacheDiscontinuity);
        assert_eq!(s.evidence[0].metric_name, "cache_hit_ratio");
    }

    #[test]
    fn detect_cache_discontinuity_fires_on_repeated_drift() {
        // No 30pp drop, but F-7b fired twice in window.
        let h = cache_history(vec![Some(0.50); 20], vec![true, false, true, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false]);
        let sig = detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000);
        assert!(sig.is_some(), "alternate trigger: F-7b fired ≥ 2 times");
    }

    #[test]
    fn detect_cache_discontinuity_quiet_window_no_fire() {
        let h = cache_history(vec![Some(0.50); 20], vec![false; 20]);
        assert!(detect_cache_discontinuity(&h, 20, 0.30, 1_700_000_000).is_none());
    }
```

- [ ] **Step 2: Implementation** — append:

```rust
/// CacheDiscontinuity: fires either when `cache_hit_ratio` drops by
/// at least `min_drop` over the window, OR when F-7b cache-drift
/// fires `>= 2` times inside the window.
pub fn detect_cache_discontinuity(
    history: &AnomalyHistory,
    window_polls: usize,
    min_drop: f32,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    let drift_count = history
        .cache_drift_fires
        .iter()
        .take(window_polls)
        .filter(|b| **b)
        .count();
    let alternate_trigger = drift_count >= 2;

    let ratios: Vec<f32> = history
        .cache_hit_ratios
        .iter()
        .take(window_polls)
        .filter_map(|r| *r)
        .collect();

    let mut drop_value: Option<f32> = None;
    let mut before_ratio: Option<f32> = None;
    let mut after_ratio: Option<f32> = None;
    if ratios.len() >= 2 {
        let newest = ratios.first().copied().unwrap();
        let oldest = ratios.last().copied().unwrap();
        if oldest - newest >= min_drop {
            drop_value = Some(oldest - newest);
            before_ratio = Some(oldest);
            after_ratio = Some(newest);
        }
    }

    if !alternate_trigger && drop_value.is_none() {
        return None;
    }

    let confidence = if drop_value.map(|d| d >= min_drop * 1.5).unwrap_or(false)
        || drift_count >= 4
    {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };

    let evidence = AnomalyEvidence {
        metric_name: "cache_hit_ratio",
        before: before_ratio.map(|r| format!("{r:.2}")).unwrap_or_else(|| format!("drift_fires={drift_count}")),
        after: after_ratio.map(|r| format!("{r:.2}")).unwrap_or_else(|| format!("drift_fires={drift_count}")),
        sample_count: ratios.len(),
        source_kind: SourceKind::ProviderOfficial,
    };

    Some(AnomalySignal {
        kind: AnomalyKind::CacheDiscontinuity,
        confidence,
        severity: Severity::Concern,
        evidence: vec![evidence],
        window_polls,
        detected_at: now_unix_seconds,
    })
}
```

- [ ] **Step 3: Run tests + gates**

```bash
cargo test --lib detect_cache_discontinuity
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 3 new tests PASS, full lib at 1066 (1063 + 3).

- [ ] **Step 4: Commit**

```bash
git add src/policy/rules/anomaly.rs
git commit -m "phase 7 v1: detect_cache_discontinuity detector + 3 tests"
```

---

## Task 8: `detect_cross_pane_edit_cluster`

**Files:**

- Modify: `src/policy/rules/anomaly.rs`.

- [ ] **Step 1: Tests**

```rust
    fn cross_pane_history(per_tick_paths: Vec<Vec<&str>>) -> AnomalyHistory {
        let mut h = AnomalyHistory::default();
        for tick in per_tick_paths.into_iter().rev() {
            h.cross_pane_edit_paths
                .push_front(tick.into_iter().map(String::from).collect());
        }
        h
    }

    #[test]
    fn detect_cross_pane_edit_cluster_same_path_threshold() {
        // 4 ticks each emitting `/r/foo.rs` → 4 findings on same path
        let h = cross_pane_history(vec![
            vec!["/r/foo.rs"], vec!["/r/foo.rs"], vec!["/r/foo.rs"], vec!["/r/foo.rs"],
            vec![], vec![], vec![], vec![], vec![], vec![],
            vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![],
        ]);
        let sig = detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000);
        let s = sig.expect("4 same-path findings ≥ 3 threshold");
        assert_eq!(s.kind, AnomalyKind::CrossPaneEditCluster);
        assert_eq!(s.evidence[0].metric_name, "concurrent_edit_path");
    }

    #[test]
    fn detect_cross_pane_edit_cluster_different_paths_no_fire() {
        let h = cross_pane_history(vec![
            vec!["/r/a.rs"], vec!["/r/b.rs"], vec!["/r/c.rs"], vec!["/r/d.rs"],
        ]);
        assert!(detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).is_none());
    }

    #[test]
    fn detect_cross_pane_edit_cluster_below_threshold() {
        let h = cross_pane_history(vec![vec!["/r/foo.rs"], vec!["/r/foo.rs"]]); // only 2
        assert!(detect_cross_pane_edit_cluster(&h, 20, 3, 1_700_000_000).is_none());
    }
```

- [ ] **Step 2: Implementation** — append:

```rust
use std::collections::HashMap;

/// CrossPaneEditCluster: count findings per path across the window.
/// Fires when any single path appears `>= min_findings` times.
pub fn detect_cross_pane_edit_cluster(
    history: &AnomalyHistory,
    window_polls: usize,
    min_findings: usize,
    now_unix_seconds: u64,
) -> Option<AnomalySignal> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for tick_paths in history.cross_pane_edit_paths.iter().take(window_polls) {
        for path in tick_paths {
            *counts.entry(path.as_str()).or_insert(0) += 1;
        }
    }
    let (winning_path, winning_count) = counts.iter().max_by_key(|(_, c)| **c)?;
    if *winning_count < min_findings {
        return None;
    }
    let confidence = if *winning_count >= min_findings * 2 {
        AnomalyConfidence::High
    } else {
        AnomalyConfidence::Medium
    };
    Some(AnomalySignal {
        kind: AnomalyKind::CrossPaneEditCluster,
        confidence,
        severity: Severity::Concern,
        evidence: vec![AnomalyEvidence {
            metric_name: "concurrent_edit_path",
            before: winning_path.to_string(),
            after: winning_count.to_string(),
            sample_count: *winning_count,
            source_kind: SourceKind::Estimated,
        }],
        window_polls,
        detected_at: now_unix_seconds,
    })
}
```

- [ ] **Step 3: Run tests + gates**

```bash
cargo test --lib detect_cross_pane_edit_cluster
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 3 new tests PASS, full lib at 1069 (1066 + 3).

- [ ] **Step 4: Commit**

```bash
git add src/policy/rules/anomaly.rs
git commit -m "phase 7 v1: detect_cross_pane_edit_cluster detector + 3 tests"
```

---

## Task 9: `eval_anomalies` orchestrator + edge-triggered dedup

**Files:**

- Modify: `src/policy/rules/anomaly.rs`.

- [ ] **Step 1: Tests**

```rust
    use std::collections::HashMap;
    use crate::policy::gates::PolicyGates;

    fn fixture_gates() -> PolicyGates {
        PolicyGates {
            anomaly_enabled: true,
            anomaly_window_polls: 20,
            anomaly_min_confidence: AnomalyConfidence::Medium,
            anomaly_identity_churn_min_flips: 3,
            anomaly_error_burst_threshold: 0.5,
            anomaly_cache_discontinuity_drop: 0.30,
            anomaly_cross_pane_cluster_min_findings: 3,
            ..PolicyGates::default()
        }
    }

    #[test]
    fn eval_anomalies_returns_empty_when_disabled() {
        let mut gates = fixture_gates();
        gates.anomaly_enabled = false;
        let history = churn_history(vec![
            ("claude", "/r"), ("codex", "/r"), ("claude", "/r"), ("codex", "/r"),
        ]);
        let mut dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();
        let result = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_000);
        assert!(result.is_empty());
        assert!(dedup.is_empty());
    }

    #[test]
    fn eval_anomalies_edge_triggered_emit_then_suppress_then_rearm_then_emit() {
        let gates = fixture_gates();
        let history = churn_history(vec![
            ("claude", "/r"), ("codex", "/r"), ("claude", "/r"), ("codex", "/r"),
        ]);
        let mut dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();

        // Tick 1: detector returns Some, dedup empty → emit, dedup set
        let r1 = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_000);
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].kind, AnomalyKind::IdentityChurn);

        // Tick 2: detector still Some, dedup occupied → suppress
        let r2 = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_005);
        assert!(r2.is_empty());

        // Tick 3: history clears (e.g. only one snapshot left) → detector None, rearm
        let quiet = churn_history(vec![("claude", "/r")]);
        let r3 = eval_anomalies("%1", &quiet, &gates, &mut dedup, 1_700_000_010);
        assert!(r3.is_empty());

        // Tick 4: detector Some again, dedup rearmed → emit
        let r4 = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_015);
        assert_eq!(r4.len(), 1, "should re-emit after rearm");
    }

    #[test]
    fn eval_anomalies_min_confidence_filters_low_confidence() {
        let mut gates = fixture_gates();
        gates.anomaly_min_confidence = AnomalyConfidence::High;
        // Exactly threshold flips → Medium (filtered out at min_confidence=High)
        let history = churn_history(vec![
            ("claude", "/r"), ("codex", "/r"), ("claude", "/r"), ("codex", "/r"),
        ]);
        let mut dedup: HashMap<(String, AnomalyKind), Option<u64>> = HashMap::new();
        let result = eval_anomalies("%1", &history, &gates, &mut dedup, 1_700_000_000);
        assert!(result.is_empty(), "Medium signal filtered at min_confidence=High");
    }
```

- [ ] **Step 2: Implementation** — replace the stub `eval_anomalies` with:

```rust
/// Phase 7 v1 orchestrator. Runs every detector against `history`,
/// applies edge-triggered dedup against `dedup` (per `(pane_id,
/// kind)` key), filters by `gates.anomaly_min_confidence`, and
/// returns the surviving signals. Mutates `dedup` in place per the
/// edge-triggered semantics:
///
/// - detector Some + dedup None  → emit; record now in dedup
/// - detector Some + dedup Some  → suppress (still active)
/// - detector None + dedup Some  → rearm (clear dedup)
/// - detector None + dedup None  → no-op
pub fn eval_anomalies(
    pane_id: &str,
    history: &AnomalyHistory,
    gates: &PolicyGates,
    dedup: &mut std::collections::HashMap<(String, AnomalyKind), Option<u64>>,
    now_unix_seconds: u64,
) -> Vec<AnomalySignal> {
    if !gates.anomaly_enabled {
        return Vec::new();
    }

    let kinds_with_results: Vec<(AnomalyKind, Option<AnomalySignal>)> = vec![
        (AnomalyKind::IdentityChurn, detect_identity_churn(
            history,
            gates.anomaly_window_polls,
            gates.anomaly_identity_churn_min_flips,
            now_unix_seconds,
        )),
        (AnomalyKind::ErrorBurst, detect_error_burst(
            history,
            gates.anomaly_window_polls,
            gates.anomaly_error_burst_threshold,
            now_unix_seconds,
        )),
        (AnomalyKind::CacheDiscontinuity, detect_cache_discontinuity(
            history,
            gates.anomaly_window_polls,
            gates.anomaly_cache_discontinuity_drop,
            now_unix_seconds,
        )),
        (AnomalyKind::CrossPaneEditCluster, detect_cross_pane_edit_cluster(
            history,
            gates.anomaly_window_polls,
            gates.anomaly_cross_pane_cluster_min_findings,
            now_unix_seconds,
        )),
    ];

    let mut out = Vec::new();
    for (kind, raw) in kinds_with_results {
        let key = (pane_id.to_string(), kind);
        let prev = dedup.get(&key).copied().flatten();
        match (raw, prev) {
            (Some(sig), None) => {
                if sig.confidence >= gates.anomaly_min_confidence {
                    dedup.insert(key, Some(now_unix_seconds));
                    out.push(sig);
                }
                // If filtered by confidence, do NOT record in dedup so a
                // future High-confidence emit can still fire.
            }
            (Some(_sig), Some(_)) => {
                // Suppress — still active in this window.
            }
            (None, Some(_)) => {
                // Rearm.
                dedup.insert(key, None);
            }
            (None, None) => {
                // No-op.
            }
        }
    }
    out
}
```

Make sure `use crate::policy::gates::PolicyGates;` is at the top of the module.

- [ ] **Step 3: Run tests + gates**

```bash
cargo test --lib eval_anomalies
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 3 new tests PASS, full lib at 1072 (1069 + 3).

- [ ] **Step 4: Commit**

```bash
git add src/policy/rules/anomaly.rs
git commit -m "phase 7 v1: eval_anomalies orchestrator + edge-triggered dedup"
```

---

## Task 10: `PaneReport.anomalies` + event-loop wiring

**Files:**

- Modify: `src/app/event_loop.rs` — add `anomalies` field to `PaneReport`; push observations into `AnomalyHistory` each tick; call `eval_anomalies` and attach result.

- [ ] **Step 1: Add `PaneReport.anomalies` field**

In `src/app/event_loop.rs:717` (the `PaneReport` struct), append:

```rust
    /// Phase 7 v1 (v1.43.0): anomaly signals for this pane this
    /// tick, after edge-triggered dedup. Empty Vec when
    /// `[anomaly] enabled = false` (default), when no detector
    /// fired, or when the layer is disabled by `gates.anomaly_enabled`.
    pub anomalies: Vec<crate::domain::anomaly::AnomalySignal>,
```

Also append `anomalies: vec![],` to every existing `PaneReport { ... }` literal in the file (search `PaneReport {` to find them — there are several, one for the live path and one or more for dead-pane fast paths).

- [ ] **Step 2: Push current observations into `AnomalyHistory`**

In `src/app/event_loop.rs::run_once_with_target`, BEFORE the call to `policy.evaluate(...)` (around line 250), push the current tick's observations into the pane's `AnomalyHistory` and trim:

```rust
            // Phase 7 v1: push the current tick's observations into the
            // pane's AnomalyHistory so the anomaly detectors see a fresh
            // sample this iteration. Trim to gates.anomaly_window_polls
            // after each push.
            {
                let cap = gates.anomaly_window_polls.max(1);
                let entry = ctx
                    .anomaly_history
                    .entry(pane.pane_id.clone())
                    .or_default();
                let provider_label = format!("{:?}", resolved.identity.provider);
                entry
                    .identity_snapshots
                    .push_front((provider_label, pane.current_path.clone()));
                entry.error_hints.push_front(signals.error_hint);
                entry
                    .cache_hit_ratios
                    .push_front(signals.cache_hit_ratio.as_ref().map(|m| m.value));
                entry.trim(cap);
            }
```

The `cache_drift_fires` and `cross_pane_edit_paths` push happens AFTER `policy.evaluate(...)` because we need the rule output to know whether F-7b fired and whether `ConcurrentFileEdit` findings emerged.

- [ ] **Step 3: After `policy.evaluate`, push the post-rule observations and call `eval_anomalies`**

After the cost-budget alert push (around line 320, where the F-9b alerts are folded into `out.recommendations`), and AFTER the existing identity-drift detection block (around line 344), insert:

```rust
            // Phase 7 v1: complete the AnomalyHistory push for fields
            // that depend on the rule output (F-7b cache drift, F-8
            // ConcurrentFileEdit findings).
            {
                let cap = gates.anomaly_window_polls.max(1);
                let entry = ctx
                    .anomaly_history
                    .entry(pane.pane_id.clone())
                    .or_default();
                let cache_drift_fired = out
                    .recommendations
                    .iter()
                    .any(|r| r.action.contains("cache drift"));
                entry.cache_drift_fires.push_front(cache_drift_fired);
                let concurrent_paths: Vec<String> = out
                    .recommendations
                    .iter()
                    .filter_map(|_r| None) // placeholder
                    .collect();
                // Real path source: cross_pane findings landed in this
                // tick's reports list, but each report carries its own
                // findings — see the cross-pane processing block below
                // and route from there if necessary. For v1 we read
                // `out.cross_pane_findings` if the rule populated it,
                // else default empty.
                entry
                    .cross_pane_edit_paths
                    .push_front(concurrent_paths);
                entry.trim(cap);
            }
```

NOTE: the `concurrent_paths` collection is a known soft spot — it depends on whether `ConcurrentFileEdit` findings are visible at this point in the loop or only land later (cross-pane evaluation runs after the per-pane loop in `run_once_with_target`, around line 467). If they land later, defer the `cross_pane_edit_paths` push to that block (after `r.cross_pane_findings.push(f);` at line 476 of the existing code), iterating each `PaneReport` to push only the paths whose `kind == ConcurrentFileEdit`. Read the surrounding code and choose the cleanest insertion point — the goal is one push per pane per tick.

- [ ] **Step 4: Call `eval_anomalies` and attach to `PaneReport`**

After the final history push and BEFORE the `PaneReport { ... }` construction (find the existing live-path `PaneReport { ... pane_id: pane.pane_id.clone(), ... }` block), compute the anomalies:

```rust
            let anomalies = {
                let history = ctx
                    .anomaly_history
                    .get(&pane.pane_id)
                    .cloned()
                    .unwrap_or_default();
                crate::policy::rules::anomaly::eval_anomalies(
                    &pane.pane_id,
                    &history,
                    &gates,
                    &mut ctx.anomaly_dedup,
                    current_unix_ms() / 1000,
                )
            };
```

Then add `anomalies,` to the `PaneReport { ... }` literal.

If the borrow checker complains (because `ctx.anomaly_history` is read while `ctx.anomaly_dedup` is written), `clone` the relevant history into a local first, then call `eval_anomalies` with `&history`.

- [ ] **Step 5: Two integration tests** — append to `tests/event_loop_integration.rs` (or wherever Task 7 of Phase H landed its integration tests):

```rust
#[test]
fn event_loop_anomalies_fire_when_enabled_and_history_full() {
    // Build a Context with config.anomaly = { enabled: true, window_polls: 5,
    //   identity_churn_min_flips: 2, ..default }. Drive 5 ticks where
    //   provider/path flips on every tick. After tick 5, assert PaneReport.anomalies
    //   contains exactly one IdentityChurn signal.
    // Match the Phase H integration-test fixture style; use a Codex pane.
}

#[test]
fn event_loop_anomalies_off_baseline_regression() {
    // Same fixture, config.anomaly default (enabled=false). After tick 5,
    // assert PaneReport.anomalies is empty AND ctx.anomaly_dedup is empty.
}
```

Read `tests/event_loop_integration.rs` for the canonical fixture before writing the literal — Phase H added two similar tests around the time of Task 7. Mirror them.

- [ ] **Step 6: Run tests + gates**

```bash
cargo test --lib
cargo test --test event_loop_integration
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

Expected: lib unchanged at 1072 (anomaly tests are already in lib); event-loop integration grows by 2 (was 52 after Phase H → 54).

- [ ] **Step 7: Commit**

```bash
git add src/app/event_loop.rs tests/event_loop_integration.rs
git commit -m "phase 7 v1: PaneReport.anomalies + event_loop wiring + 2 integration tests"
```

---

## Task 11: m overlay ANOMALIES row

**Files:**

- Modify: `src/ui/metrics.rs` — render one new line per pane card body when `pane_report.anomalies` is non-empty.

- [ ] **Step 1: Locate the pane-card rendering function**

```bash
grep -nE "fn build_pane_card|fn render_pane_card|fn pane_card_lines|render_metrics" src/ui/metrics.rs
```

Find where individual pane cards are built — the v1.39 layout puts CTX/COST/TOKENS/CACHE/RESET rows there.

- [ ] **Step 2: Write a snapshot/string test**

If the file uses `assert_snapshot!`, find an existing snapshot test and add a fixture pane with `anomalies = vec![AnomalySignal { kind: IdentityChurn, confidence: Medium, ... }]`, then assert the rendered output contains `"ANOMALIES 1"` and `"IdentityChurn:medium"`.

If the file uses `Vec<Line<'_>>` returns, write a unit test:

```rust
#[test]
fn pane_card_includes_anomalies_row_when_present() {
    let pane = /* fixture PaneReport with anomalies = vec![...IdentityChurn Medium...] */;
    let lines = build_pane_card_lines(&pane, /* width, ... */);
    let rendered = lines_to_text(&lines);
    assert!(rendered.contains("ANOMALIES 1"));
    assert!(rendered.contains("IdentityChurn:medium"));
}

#[test]
fn pane_card_omits_anomalies_row_when_empty() {
    let pane = /* fixture with anomalies = vec![] */;
    let lines = build_pane_card_lines(&pane, /* width, ... */);
    let rendered = lines_to_text(&lines);
    assert!(!rendered.contains("ANOMALIES"));
}
```

If `lines_to_text` doesn't exist, write a 5-line helper that flattens a `&[Line<'_>]` to a String by concatenating all spans.

- [ ] **Step 3: Run test to verify fail** — Expected: 2 FAIL.

- [ ] **Step 4: Implement the row**

Inside the pane-card builder, after the existing CTX/COST/TOKENS/CACHE/RESET rows (look for a pattern like `if let Some(ctx) = pane.signals.context_pressure { ... lines.push(...) }`), add:

```rust
    if !pane_report.anomalies.is_empty() {
        let mut summary = format!("ANOMALIES {} ", pane_report.anomalies.len());
        let pieces: Vec<String> = pane_report
            .anomalies
            .iter()
            .map(|a| format!("{}:{}", a.kind.label(), a.confidence.label()))
            .collect();
        summary.push_str(&pieces.join(", "));
        // Truncate to card width if needed; reuse wrap_aligned_field
        // helper if the file uses it for long rows. Otherwise let the
        // ratatui line wrap naturally.
        lines.push(Line::from(Span::styled(
            summary,
            Style::default().fg(Color::Yellow), // soft-yellow for Concern severity
        )));
    }
```

Use whatever `Style` / `Color` helpers the file already uses — if v1.39 introduced a severity-bar palette function, call that with the highest severity in `pane_report.anomalies`.

- [ ] **Step 5: Run tests + gates**

```bash
cargo test --lib pane_card
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 2 new tests PASS, full lib at 1074 (1072 + 2).

- [ ] **Step 6: Commit**

```bash
git add src/ui/metrics.rs
git commit -m "phase 7 v1: m overlay pane card ANOMALIES row"
```

---

## Task 12: Settings overlay rows

**Files:**

- Modify: `src/ui/settings.rs` — Parameters tab `[anomaly]` group, Rules tab four rows, Badges tab ANOMALIES row.

- [ ] **Step 1: Locate the existing format**

```bash
grep -nE "\[reset\]|\[cache\]|setting_row|rule_row|build_parameter_body_lines|build_rule_body_lines|build_badge_body_lines" src/ui/settings.rs
```

(Phase H Task 8 used `setting_row(label, value, default)` and `rule_row(label, condition)` — same helpers.)

- [ ] **Step 2: Write or extend tests**

Add to the settings-overlay test module:

```rust
#[test]
fn parameters_tab_shows_anomaly_section() {
    let mut config = QmonsterConfig::default();
    config.anomaly.enabled = true;
    config.anomaly.window_polls = 25;
    let lines = build_parameter_body_lines(&config);
    let rendered = rendered_text(&lines);
    assert!(rendered.contains("anomaly enabled"));
    assert!(rendered.contains("on"));
    assert!(rendered.contains("anomaly window_polls"));
    assert!(rendered.contains("25"));
}

#[test]
fn rules_tab_shows_four_anomaly_rows() {
    let config = QmonsterConfig::default();
    let lines = build_rule_body_lines(&config);
    let rendered = rendered_text(&lines);
    assert!(rendered.contains("anomaly: IdentityChurn"));
    assert!(rendered.contains("anomaly: ErrorBurst"));
    assert!(rendered.contains("anomaly: CacheDiscontinuity"));
    assert!(rendered.contains("anomaly: CrossPaneEditCluster"));
}

#[test]
fn badges_tab_includes_anomalies_description() {
    let config = QmonsterConfig::default();
    let lines = build_badge_body_lines(&config);
    let rendered = rendered_text(&lines);
    assert!(rendered.contains("ANOMALIES"));
}
```

(Helpers `rendered_text` / `build_*_body_lines` are already used by other settings tests — check the file's existing test module first to confirm names.)

- [ ] **Step 3: Run tests to verify fail** — Expected: 3 FAIL.

- [ ] **Step 4: Add the Parameters rows**

In `build_parameter_body_lines`, after the existing `[reset] auto_snapshot` row Phase H added, insert seven anomaly rows using the same `setting_row` / `on_off` helpers Phase H matched. Defaults are taken from `AnomalyConfig::default()`. Example shape:

```rust
    setting_row(
        "anomaly enabled",
        on_off(config.anomaly.enabled).to_string(),
        on_off(defaults.anomaly.enabled).to_string(),
    ),
    setting_row(
        "anomaly window_polls",
        config.anomaly.window_polls.to_string(),
        defaults.anomaly.window_polls.to_string(),
    ),
    setting_row(
        "anomaly min_confidence",
        config.anomaly.min_confidence.clone(),
        defaults.anomaly.min_confidence.clone(),
    ),
    setting_row(
        "anomaly identity_churn_min_flips",
        config.anomaly.identity_churn_min_flips.to_string(),
        defaults.anomaly.identity_churn_min_flips.to_string(),
    ),
    setting_row(
        "anomaly error_burst_threshold",
        format!("{:.2}", config.anomaly.error_burst_threshold),
        format!("{:.2}", defaults.anomaly.error_burst_threshold),
    ),
    setting_row(
        "anomaly cache_discontinuity_drop",
        format!("{:.2}", config.anomaly.cache_discontinuity_drop),
        format!("{:.2}", defaults.anomaly.cache_discontinuity_drop),
    ),
    setting_row(
        "anomaly cross_pane_cluster_min_findings",
        config.anomaly.cross_pane_cluster_min_findings.to_string(),
        defaults.anomaly.cross_pane_cluster_min_findings.to_string(),
    ),
```

- [ ] **Step 5: Add the Rules rows**

In `build_rule_body_lines`, after the existing reset/snapshot rules block, insert:

```rust
    rule_row(
        "anomaly: IdentityChurn",
        format!(
            "fires when (provider, path) flips ≥ {} times in {} polls",
            config.anomaly.identity_churn_min_flips,
            config.anomaly.window_polls,
        ),
    ),
    rule_row(
        "anomaly: ErrorBurst",
        format!(
            "fires when error rate ≥ {:.0}% over {} polls",
            config.anomaly.error_burst_threshold * 100.0,
            config.anomaly.window_polls,
        ),
    ),
    rule_row(
        "anomaly: CacheDiscontinuity",
        format!(
            "fires when cache_hit_ratio drops ≥ {:.0}pp OR F-7b fires ≥ 2× in {} polls",
            config.anomaly.cache_discontinuity_drop * 100.0,
            config.anomaly.window_polls,
        ),
    ),
    rule_row(
        "anomaly: CrossPaneEditCluster",
        format!(
            "fires when ≥ {} ConcurrentFileEdit findings target the same path in {} polls",
            config.anomaly.cross_pane_cluster_min_findings,
            config.anomaly.window_polls,
        ),
    ),
```

- [ ] **Step 6: Add the Badges row**

In `build_badge_body_lines`, append:

```rust
    badge_row(
        "ANOMALIES",
        "anomaly count and kinds per pane (in m overlay); aggregated from D2/F-9/F-3+F-7b/F-8 history; SourceKind: Estimated/ProviderOfficial",
    ),
```

(If `badge_row` doesn't exist, follow the existing badge-row format — likely just an inline `Line::from(...)` with the label and description.)

- [ ] **Step 7: Run tests + gates**

```bash
cargo test --lib settings
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: 3 new tests PASS, full lib at 1077 (1074 + 3).

- [ ] **Step 8: Commit**

```bash
git add src/ui/settings.rs
git commit -m "phase 7 v1: Settings Parameters/Rules/Badges rows for [anomaly]"
```

---

## Task 13: Docs sync

**Files:**

- Modify: `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml`.

No code changes. No tests.

- [ ] **Step 1: `docs/ai/UI_MANUAL.md` — m overlay anomaly subsection**

Find the m overlay section (search for `§8.5` / "Metrics overlay" / "m overlay"). Append a subsection explaining the new ANOMALIES row:

```markdown
### Phase 7 v1: anomaly observation surface (v1.43.0)

When `[anomaly] enabled = true`, each pane card in the m
(Metrics) overlay grows a new bottom row:
```

ANOMALIES <n> <kind>:<conf>[, ...]

```

The row appears only when `pane.anomalies` is non-empty —
operators who keep the layer off see no layout change.

Detector matrix:
- IdentityChurn — provider/path flips inside the rolling window
- ErrorBurst — error_hint rate inside the rolling window
- CacheDiscontinuity — cache_hit_ratio drop, or F-7b fires ≥ 2×
- CrossPaneEditCluster — same-path ConcurrentFileEdit findings

Severity is always `Concern` in v1; v2 will add Recommendation
promotion + Notify when confidence is High and severity is
Warning+. v1 emits no audit event and no Notify.

Operator opts in by adding `[anomaly] enabled = true` to
`qmonster.toml`. Defaults (window_polls=20, min_confidence=medium,
threshold values per detector) keep noise low — see
`config/qmonster.example.toml` for the full default block.
```

- [ ] **Step 2: `docs/ai/ARCHITECTURE.md` — Phase 7 v1 paragraph**

Append a paragraph below the v1.42.0 (Phase H) entry:

```markdown
v1.43.0 ships **Phase 7 v1 anomaly observation surface**. New
domain types in `src/domain/anomaly.rs` (`AnomalySignal`,
`AnomalyKind` with 4 v1 variants, `AnomalyConfidence`,
`AnomalyEvidence`) and a new pure rule module
`src/policy/rules/anomaly.rs` with four detectors plus the
`eval_anomalies` orchestrator. Detectors consume
`AnomalyHistory` (per-pane rolling window kept in
`Context.anomaly_history`); each tick the event loop pushes the
current observations (provider/path snapshot, `error_hint`,
`cache_hit_ratio`, F-7b cache-drift fire flag, F-8
ConcurrentFileEdit paths) and trims to
`gates.anomaly_window_polls`. Edge-triggered dedup lives in
`Context.anomaly_dedup: HashMap<(pane_id, AnomalyKind),
Option<u64>>` — emit on Some-after-None edge, suppress while the
detector continues to return Some, rearm when it returns None,
emit fresh after rearm. Output rides on a new
`PaneReport.anomalies: Vec<AnomalySignal>` field; the m overlay
pane card renders one new line when the vec is non-empty. v1
emits no `Recommendation`, no `Notify`, no audit event — pure
observation surface. v2 plans add `CostSlope`, `TokenSlope`,
`MemoryGrowth`, `SubagentSideEffect` detectors plus
Recommendation/Notify promotion gated on confidence + severity.
```

- [ ] **Step 3: `docs/ai/VALIDATION.md` — checklist row**

Append a checkbox row:

```markdown
- [x] Phase 7 v1 anomaly observation (v1.43.0): `[anomaly] enabled = false`
      by default; `enabled = true` opens the m overlay ANOMALIES row
      per pane via `policy::rules::anomaly::eval_anomalies` over a
      per-pane rolling `AnomalyHistory`; edge-triggered dedup keeps
      one emission per active window.
```

- [ ] **Step 4: `config/qmonster.example.toml` — `[anomaly]` block**

Append (commented out — opt-in):

```toml
# Phase 7 v1 anomaly observation surface (v1.43.0).
# Master switch + four conservative-default detectors. When enabled,
# the m overlay grows an "ANOMALIES" row per pane card. v1 emits no
# Recommendation, no Notify, no audit event — observation only.
# Operators wanting Recommendation promotion / Notify wait for v2.
# [anomaly]
# enabled                          = false
# window_polls                     = 20
# min_confidence                   = "medium"
# identity_churn_min_flips         = 3
# error_burst_threshold            = 0.5
# cache_discontinuity_drop         = 0.30
# cross_pane_cluster_min_findings  = 3
```

- [ ] **Step 5: Validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green (docs changes don't move tests).

- [ ] **Step 6: Commit**

```bash
git add docs/ai/UI_MANUAL.md docs/ai/ARCHITECTURE.md docs/ai/VALIDATION.md config/qmonster.example.toml
git commit -m "phase 7 v1: docs (UI_MANUAL, ARCHITECTURE, VALIDATION, example.toml)"
```

---

## Task 14: v1.43.0 release ledger sync

**Files:**

- Modify: `package.json`, `VERSION.md`, `README.md`, `mission.yaml`, `mission-history.yaml`, `.mission/CURRENT_STATE.md`, `docs/ai/PROJECT_BRIEF.md` (Date + Phase line), `docs/ai/ARCHITECTURE.md` (Date line).

Mirror the v1.42.0 ledger-sync pattern (commit `c35dc86`). NO source-code changes.

- [ ] **Step 1: Read the v1.42.0 ledger sync diff for reference**

```bash
git show c35dc86 -- package.json VERSION.md README.md docs/ai/PROJECT_BRIEF.md docs/ai/ARCHITECTURE.md mission.yaml mission-history.yaml .mission/CURRENT_STATE.md | head -300
```

Mirror the structural pattern. Write fresh prose for Phase 7 v1.

- [ ] **Step 2: `package.json`** — `1.42.0` → `1.43.0`.

- [ ] **Step 3: `VERSION.md`** — bump to `1.43.0`. One-liner:
      `v1.43.0 — Phase 7 v1: anomaly observation surface ([anomaly] enabled).`

- [ ] **Step 4: `README.md`** — add v1.43.0 row.

- [ ] **Step 5: `mission.yaml`** — Phase line gets `+ Phase 7 v1` after `Phase H`. Update `execution_hints.topology`, `suggested_approach`, `budget_hint.priority`, `budget_hint.scope`, `budget_hint.followups` per the v1.42.0 pattern. Drop "Phase 7 v1 spec/plan pending — implement next" from followups. Promote next item (Phase 7 v2 outline; Phase H polish; or whatever the operator picks next) to top followup or leave the followup list slimmer.

- [ ] **Step 6: `mission-history.yaml`** — append v1.43.0 entry. Use SHAs from `git log v1.42.0..HEAD --oneline`.

- [ ] **Step 7: `.mission/CURRENT_STATE.md`** — full refresh:
  - `_Last updated: 2026-05-07 (Claude, v1.43.0 release ledger sync)_` (or whatever date the implementer is acting on)
  - Mission title v1.43.0; version surfaces line v1.43.0
  - v1.43.0 Feature State section (Phase 7 v1 anomaly surface, files touched, behavior on/off)
  - Active Follow-Ups: drop "Phase 7 v1 plan", promote whatever's next (v2 detectors, anomaly→Recommendation promotion, etc.)
  - Validation Baseline: lib ~1077, integration ~54, etc.

- [ ] **Step 8: `docs/ai/PROJECT_BRIEF.md`** — Date line: append v1.43.0 segment. Phase line: add `+ Phase 7 v1` after `Phase H`.

- [ ] **Step 9: `docs/ai/ARCHITECTURE.md`** — Date line: append v1.43.0 segment.

- [ ] **Step 10: Run final validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --all-targets
```

Expected: all green.

- [ ] **Step 11: Commit and tag (annotated for consistency with v1.40 / v1.41 / v1.42)**

```bash
git add package.json VERSION.md README.md mission.yaml mission-history.yaml .mission/CURRENT_STATE.md docs/ai/PROJECT_BRIEF.md docs/ai/ARCHITECTURE.md
git commit -m "v1.43.0 release ledger sync"
git tag -a v1.43.0 HEAD -m "v1.43.0 — Phase 7 v1 anomaly observation surface"
```

DO NOT push. The user will push after final review.

- [ ] **Step 12: Verify final state**

```bash
git log --oneline -20
git tag --list | tail -5
git for-each-ref --format='%(refname:short) %(objecttype)' refs/tags/v1.4*
```

Expected: 14+ Phase 7 v1 commits + 1 release ledger sync, the v1.43.0 annotated tag pointing at the ledger sync.

---

## Self-review

**1. Spec coverage:**

- §1 Goal — Tasks 5–9 (detectors + orchestrator) + Task 11 (m overlay) deliver the four-kind anomaly surface; Tasks 1–3 + 10 deliver the infrastructure.
- §2 Non-goals — explicitly preserved: no Recommendation emission (Tasks 5–9 build `AnomalySignal`, never push to `out.recommendations`), no Notify (Task 10 wiring doesn't add to `out.effects`), no new audit kinds (no `AuditEvent::record` calls anywhere in the new code), no SQLite, no per-subagent attribution.
- §3 Architecture — exactly mirrored: `AnomalySignal` types in domain, detectors in policy/rules, orchestrator + dedup in policy/rules, history + dedup in Context, render in m overlay only.
- §4 Config — Task 2 ships every prescribed field with the prescribed defaults.
- §5 Detector matrix — Tasks 5–8 implement all four kinds with the prescribed inputs and triggers; evidence rows match.
- §6 Data flow — Task 10's wiring follows the spec's pseudo-code exactly (push observations → eval_anomalies → attach to PaneReport → render).
- §7 m overlay — Task 11 renders the prescribed `ANOMALIES <n> <kind>:<conf>` row.
- §8 Settings — Task 12 adds Parameters group, Rules rows, Badges row.
- §9 Error handling — every detector returns None on insufficient input; gates filter unknowns; no I/O to fail.
- §10 Test plan — all 14 tests in the spec table are present across Tasks 1–10 (4 detector positives, 4 detector negatives + extras, 1 disabled-returns-empty, 1 min_confidence filter, 1 dedup edge-trigger, 1 integration enabled, 1 integration off-baseline regression).
- §11 Validation gates — Task 14 enforces them at release time.
- §12 v2 follow-ups — explicitly NOT implemented (deferred per the spec).

**2. Placeholder scan:**

- Task 10 Step 3 has a soft spot around `cross_pane_edit_paths` collection — depends on whether F-8 cross-pane findings are accessible mid-loop or only after the per-pane phase. This is a known plan-stage decision the implementer must resolve by reading `event_loop.rs:415-490`. Flag in the implementer prompt; not a placeholder, a documented integration choice.
- Task 11 fixture-builder details (`build_pane_card_lines` exact name, `lines_to_text` helper) depend on what the file actually exposes; the plan instructs the implementer to read first and adapt. Same pattern Phase H used; not a placeholder.
- No "TBD", "TODO", "implement later", "fill in details" in the plan.

**3. Type consistency:**

- `AnomalySignal`, `AnomalyKind`, `AnomalyConfidence`, `AnomalyEvidence` — used consistently across Tasks 1, 5–10, 11, 12.
- `AnomalyConfig` field names match between `src/app/config.rs` (Task 2) and `PolicyGates` (Task 3) — same names with `anomaly_` prefix on the gates side.
- `AnomalyHistory` field names match between Task 4 (declaration), Tasks 5–8 (consumers), and Task 10 (producer in event_loop).
- `eval_anomalies` signature `(pane_id, history, gates, dedup, now)` is consistent between Task 9 (definition) and Task 10 (call site).

If any of these issues arise during execution, they are plan-stage integration choices — the implementer should flag DONE_WITH_CONCERNS or escalate, not silently guess.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-06-phase7-v1-anomaly-overlay.md`.

Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, two-stage review per task, fast iteration. Required sub-skill: `superpowers:subagent-driven-development`.
2. **Inline Execution** — execute tasks in this session via `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
