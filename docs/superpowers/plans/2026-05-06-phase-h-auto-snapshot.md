# Phase H: opt-in auto-snapshot at reset boundary — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `[reset] auto_snapshot = true` is set, automatically run a runtime snapshot once per `(pane_id, quota_kind, window_id)` triple whenever the existing F-7c `recommend_snapshot_before_reset` rule emits a recommendation. Closes the last automation gap of the original token-optimization five-layer model (`.docs/init/Qmonster_종합_기획_보고서_v0.4.0_2026-04-20.md` § 9 layer 3 "Archive + checkpoint").

**Architecture:** Pure consumer of the existing F-7c `Recommendation` stream. The policy code, the `ResetConfig` thresholds (F-7d), the `SnapshotWriter` (Phase 2), the `SnapshotWritten` audit kind, and the `SystemNotice` push path are all reused unchanged. The only new code is one config field, one `PolicyGates` field, one `Context` HashMap, one new module (`src/app/auto_snapshot.rs`), and one call site in `src/app/event_loop.rs`. Reference spec: `docs/superpowers/specs/2026-05-06-phase-h-auto-snapshot-design.md`.

**Tech Stack:** Rust 2021, `cargo test --lib`, `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`, `cargo fmt --all --check`.

---

## File Map

- `src/app/config.rs` — `ResetConfig` gains `auto_snapshot: bool` (`#[serde(default)]`, default `false`).
- `src/policy/gates.rs` — `PolicyGates` gains `reset_auto_snapshot: bool`, copied from `config.reset.auto_snapshot` in `from_inputs`.
- `src/app/auto_snapshot.rs` (new) — `AutoSnapshotDedup` struct and `maybe_auto_snapshot(...)` orchestrator. All logic lives here; pure over its dependencies so unit tests can drive it directly.
- `src/app/mod.rs` — `pub mod auto_snapshot;`
- `src/app/bootstrap.rs` — `Context.auto_snapshot_dedup: HashMap<String, AutoSnapshotDedup>`, initialized in `Context::new`. Lifecycle reset on `PaneLifecycleEvent::{BecameDead, Reappeared}` mirrors `identity_history`.
- `src/app/event_loop.rs` — One call to `auto_snapshot::maybe_auto_snapshot(...)` between `policy.evaluate(...)` and the existing `out.recommendations` push. Plus pane-eviction wiring for `auto_snapshot_dedup`.
- `src/app/settings_overlay.rs` — `Parameters` tab: `[reset]` group gets a row for `auto_snapshot`. `Rules` tab: existing `snapshot_before_reset` row gains an annotation when `auto_snapshot = true`.
- `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml` — Phase H surface. Mirrors the F-7d documentation pattern.
- `package.json`, `VERSION.md`, `README.md`, `mission.yaml`, `mission-history.yaml`, `.mission/CURRENT_STATE.md`, `docs/ai/PROJECT_BRIEF.md`, `docs/ai/ARCHITECTURE.md` — release ledger sync to v1.42.0 (final task).

## Validation Gates (run after every task)

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

If any gate fails, fix before committing the task. Each task's "Commit" step assumes all three gates pass.

---

## Task 1: `ResetConfig.auto_snapshot` field

**Files:**

- Modify: `src/app/config.rs:254-283` (`ResetConfig` struct + `Default` impl)
- Test (append): `src/app/config.rs` existing `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing tests**

Append to the `#[cfg(test)] mod tests` block in `src/app/config.rs`:

```rust
#[test]
fn reset_config_auto_snapshot_defaults_false() {
    let r = ResetConfig::default();
    assert!(!r.auto_snapshot);
}

#[test]
fn reset_config_auto_snapshot_loads_from_toml() {
    let toml = r#"
[reset]
auto_snapshot = true
"#;
    let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
    assert!(cfg.reset.auto_snapshot);
}

#[test]
fn reset_config_missing_section_keeps_auto_snapshot_false() {
    let cfg: QmonsterConfig = toml::from_str("").expect("parse");
    assert!(!cfg.reset.auto_snapshot);
}

#[test]
fn reset_config_partial_section_keeps_other_defaults() {
    let toml = r#"
[reset]
auto_snapshot = true
"#;
    let cfg: QmonsterConfig = toml::from_str(toml).expect("parse");
    assert!(cfg.reset.auto_snapshot);
    assert!((cfg.reset.wait_pressure_threshold - 0.85).abs() < f32::EPSILON);
    assert_eq!(cfg.reset.wait_eta_secs, 30 * 60);
    assert!((cfg.reset.snapshot_pressure_threshold - 0.50).abs() < f32::EPSILON);
    assert_eq!(cfg.reset.snapshot_eta_secs, 5 * 60);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib reset_config_auto_snapshot
```

Expected: 4 tests fail with `error[E0609]: no field 'auto_snapshot' on type 'ResetConfig'`.

- [ ] **Step 3: Add the field to `ResetConfig`**

Modify `src/app/config.rs:254-272` — after the existing `snapshot_eta_secs` field, add:

```rust
    /// Phase H (v1.42.0): when `true`, the actuation hook in
    /// `src/app/auto_snapshot.rs` automatically writes one snapshot per
    /// `(pane_id, quota_kind, window_id)` whenever
    /// `recommend_snapshot_before_reset` fires. Default `false` —
    /// existing v1.41.0 operators see no behavior change. Reuses the
    /// F-7d `snapshot_pressure_threshold` / `snapshot_eta_secs`
    /// thresholds; this flag is the actuation gate, not a new
    /// detection threshold.
    pub auto_snapshot: bool,
```

Modify the `Default` impl at `src/app/config.rs:274-283` — append `auto_snapshot: false,` to the struct literal:

```rust
impl Default for ResetConfig {
    fn default() -> Self {
        Self {
            wait_pressure_threshold: 0.85,
            wait_eta_secs: 30 * 60,
            snapshot_pressure_threshold: 0.50,
            snapshot_eta_secs: 5 * 60,
            auto_snapshot: false,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib reset_config_auto_snapshot
```

Expected: 4 PASS.

- [ ] **Step 5: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/app/config.rs
git commit -m "phase H: ResetConfig auto_snapshot field"
```

---

## Task 2: `PolicyGates.reset_auto_snapshot` field

**Files:**

- Modify: `src/policy/gates.rs:55-145` (struct + `Default` impl), `src/policy/gates.rs:175-260` (`from_inputs`), `src/policy/gates.rs:300-330` (test default block)
- Test (append): `src/policy/gates.rs` existing `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block in `src/policy/gates.rs` (next to existing `from_inputs_reads_reset_config` test, around line 434):

```rust
#[test]
fn from_inputs_propagates_reset_auto_snapshot() {
    let mut config = QmonsterConfig::default();
    config.reset.auto_snapshot = true;

    let gates = PolicyGates::from_inputs(PolicyGateInputs {
        actions: &config.actions,
        ux: &config.ux,
        quota: &config.quota,
        security: &config.security,
        cache: &config.cache,
        reset: &config.reset,
        profile_switch: &config.profile_switch,
        provider: Provider::Claude,
        confidence: IdentityConfidence::High,
    });
    assert!(gates.reset_auto_snapshot);

    config.reset.auto_snapshot = false;
    let gates_off = PolicyGates::from_inputs(PolicyGateInputs {
        actions: &config.actions,
        ux: &config.ux,
        quota: &config.quota,
        security: &config.security,
        cache: &config.cache,
        reset: &config.reset,
        profile_switch: &config.profile_switch,
        provider: Provider::Claude,
        confidence: IdentityConfidence::High,
    });
    assert!(!gates_off.reset_auto_snapshot);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --lib from_inputs_propagates_reset_auto_snapshot
```

Expected: FAIL with `error[E0609]: no field 'reset_auto_snapshot' on type 'PolicyGates'`.

- [ ] **Step 3: Add the field to `PolicyGates`**

Modify `src/policy/gates.rs` near the existing `reset_*` fields (line 61-64). Insert after `reset_snapshot_eta_secs`:

```rust
    /// Phase H (v1.42.0): when `true`, `app::auto_snapshot::maybe_auto_snapshot`
    /// auto-actuates the matching `Recommendation` once per
    /// `(pane_id, quota_kind, window_id)`. Mirrors `[reset] auto_snapshot`.
    pub reset_auto_snapshot: bool,
```

Modify the `Default` impl at `src/policy/gates.rs:140-143` — add `reset_auto_snapshot: false,` after `reset_snapshot_eta_secs`:

```rust
            reset_wait_pressure: 0.85,
            reset_wait_eta_secs: 30 * 60,
            reset_snapshot_pressure: 0.50,
            reset_snapshot_eta_secs: 5 * 60,
            reset_auto_snapshot: false,
```

Modify `from_inputs` at `src/policy/gates.rs:212-215` — add the propagation right after `reset_snapshot_eta_secs`:

```rust
            reset_wait_pressure: reset.wait_pressure_threshold,
            reset_wait_eta_secs: reset.wait_eta_secs,
            reset_snapshot_pressure: reset.snapshot_pressure_threshold,
            reset_snapshot_eta_secs: reset.snapshot_eta_secs,
            reset_auto_snapshot: reset.auto_snapshot,
```

Modify the test-fixture default block at `src/policy/gates.rs:321-324` — add `reset_auto_snapshot: false,`:

```rust
            reset_wait_pressure: 0.85,
            reset_wait_eta_secs: 30 * 60,
            reset_snapshot_pressure: 0.50,
            reset_snapshot_eta_secs: 5 * 60,
            reset_auto_snapshot: false,
```

- [ ] **Step 4: Run tests to verify the new test passes and existing ones stay green**

```bash
cargo test --lib --package qmonster gates
```

Expected: all gates tests PASS, including the new one.

- [ ] **Step 5: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/policy/gates.rs
git commit -m "phase H: PolicyGates.reset_auto_snapshot wired from ResetConfig"
```

---

## Task 3: `AutoSnapshotDedup` struct and module skeleton

**Files:**

- Create: `src/app/auto_snapshot.rs`
- Modify: `src/app/mod.rs` (add `pub mod auto_snapshot;`)

- [ ] **Step 1: Write the failing tests**

Create `src/app/auto_snapshot.rs` with this initial content (struct stub + tests only, no logic yet):

```rust
//! Phase H (v1.42.0): opt-in actuation hook for the F-7c
//! `recommend_snapshot_before_reset` recommendation. When
//! `[reset] auto_snapshot = true`, an operator opts in to having
//! the snapshot written automatically — once per
//! `(pane_id, quota_kind, window_id)` triple. The dedup key is
//! the provider-reported `quota_*_resets_at` (rounded to the
//! nearest minute) so all ticks inside a single reset window
//! agree on the same key.

/// Per-pane, per-quota-window dedup state for Phase H.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AutoSnapshotDedup {
    /// Last `quota_5h_resets_at` (rounded to minute) for which a
    /// snapshot was successfully written, or `None` if no snapshot
    /// has yet been written in the current 5h window.
    pub quota_5h_window_id: Option<u64>,
    /// Same as `quota_5h_window_id` but for the weekly window.
    pub quota_weekly_window_id: Option<u64>,
}

/// Quota window that fired the recommendation. Phase H tracks each
/// quota window's dedup independently so a single tick that fires
/// both can run two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaKind {
    FiveHour,
    Weekly,
}

impl QuotaKind {
    fn label(self) -> &'static str {
        match self {
            QuotaKind::FiveHour => "5h",
            QuotaKind::Weekly => "weekly",
        }
    }
}

impl AutoSnapshotDedup {
    /// Returns `true` if the given `(quota_kind, window_id)` has
    /// already been snapshotted in this window. Floor `window_id` to
    /// the nearest minute *before* calling so jitter on
    /// `quota_*_resets_at` doesn't break the match.
    pub fn matches(&self, kind: QuotaKind, window_id: u64) -> bool {
        match kind {
            QuotaKind::FiveHour => self.quota_5h_window_id == Some(window_id),
            QuotaKind::Weekly => self.quota_weekly_window_id == Some(window_id),
        }
    }

    /// Mark a successful snapshot for `(quota_kind, window_id)`.
    pub fn set(&mut self, kind: QuotaKind, window_id: u64) {
        match kind {
            QuotaKind::FiveHour => self.quota_5h_window_id = Some(window_id),
            QuotaKind::Weekly => self.quota_weekly_window_id = Some(window_id),
        }
    }
}

/// Round a `quota_*_resets_at` Unix timestamp (seconds) down to
/// the nearest minute. This absorbs sub-minute jitter from F-5b /
/// F-6 so two ticks emitted within the same window share the same
/// `window_id`.
pub fn floor_to_minute(unix_seconds: u64) -> u64 {
    unix_seconds - (unix_seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_default_has_no_window_ids() {
        let d = AutoSnapshotDedup::default();
        assert_eq!(d.quota_5h_window_id, None);
        assert_eq!(d.quota_weekly_window_id, None);
    }

    #[test]
    fn dedup_set_and_matches_round_trip() {
        let mut d = AutoSnapshotDedup::default();
        assert!(!d.matches(QuotaKind::FiveHour, 1_700_000_000));
        d.set(QuotaKind::FiveHour, 1_700_000_000);
        assert!(d.matches(QuotaKind::FiveHour, 1_700_000_000));
        assert!(!d.matches(QuotaKind::FiveHour, 1_700_000_060)); // different minute
    }

    #[test]
    fn dedup_5h_and_weekly_independent() {
        let mut d = AutoSnapshotDedup::default();
        d.set(QuotaKind::FiveHour, 1_700_000_000);
        assert!(d.matches(QuotaKind::FiveHour, 1_700_000_000));
        assert!(!d.matches(QuotaKind::Weekly, 1_700_000_000));
        d.set(QuotaKind::Weekly, 1_700_000_000);
        assert!(d.matches(QuotaKind::Weekly, 1_700_000_000));
    }

    #[test]
    fn floor_to_minute_drops_seconds() {
        assert_eq!(floor_to_minute(1_700_000_037), 1_700_000_000 - 0); // depends, see body
        assert_eq!(floor_to_minute(60), 60);
        assert_eq!(floor_to_minute(59), 0);
        assert_eq!(floor_to_minute(120), 120);
        assert_eq!(floor_to_minute(121), 120);
    }
}
```

Note: the body of `floor_to_minute_drops_seconds` mixes a tricky line — the second `assert_eq` for `1_700_000_037` is intentionally annotated; verify by hand: `1_700_000_037 - (1_700_000_037 % 60) = 1_700_000_037 - 37 = 1_700_000_000`. Replace the line `assert_eq!(floor_to_minute(1_700_000_037), 1_700_000_000 - 0);` with:

```rust
        assert_eq!(floor_to_minute(1_700_000_037), 1_700_000_000);
```

Also append `pub mod auto_snapshot;` to `src/app/mod.rs` next to the existing `pub mod` declarations (alphabetical placement).

- [ ] **Step 2: Run tests to verify they pass**

```bash
cargo test --lib auto_snapshot
```

Expected: 4 PASS.

- [ ] **Step 3: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add src/app/auto_snapshot.rs src/app/mod.rs
git commit -m "phase H: AutoSnapshotDedup + QuotaKind + floor_to_minute"
```

---

## Task 4: Recommendation filter and quota-kind extraction

**Files:**

- Modify: `src/app/auto_snapshot.rs` (extend module)

- [ ] **Step 1: Write the failing tests**

Append to the `#[cfg(test)] mod tests` block in `src/app/auto_snapshot.rs`:

```rust
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::{Recommendation, Severity};

    fn make_rec(action: &'static str) -> Recommendation {
        Recommendation {
            action,
            reason: "test".into(),
            severity: Severity::Good,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        }
    }

    #[test]
    fn extract_kind_returns_5h_for_5h_action() {
        let rec = make_rec("snapshot before 5h window resets");
        assert_eq!(extract_quota_kind(&rec), Some(QuotaKind::FiveHour));
    }

    #[test]
    fn extract_kind_returns_weekly_for_weekly_action() {
        let rec = make_rec("snapshot before weekly window resets");
        assert_eq!(extract_quota_kind(&rec), Some(QuotaKind::Weekly));
    }

    #[test]
    fn extract_kind_returns_none_for_unrelated_action() {
        let rec = make_rec("quota: pause until 5h window resets");
        assert_eq!(extract_quota_kind(&rec), None);
    }

    #[test]
    fn extract_kind_returns_none_for_arbitrary_text() {
        let rec = make_rec("log storm detected");
        assert_eq!(extract_quota_kind(&rec), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib auto_snapshot::tests::extract_kind
```

Expected: 4 FAIL with `cannot find function 'extract_quota_kind'`.

- [ ] **Step 3: Add `extract_quota_kind` and string constants**

In `src/app/auto_snapshot.rs`, between the `floor_to_minute` function and the `#[cfg(test)]` block, add:

```rust
use crate::domain::recommendation::Recommendation;

/// `Recommendation.action` constant emitted by
/// `recommend_snapshot_before_reset` for the 5h window.
const SNAPSHOT_5H_ACTION: &str = "snapshot before 5h window resets";

/// `Recommendation.action` constant emitted by
/// `recommend_snapshot_before_reset` for the weekly window.
const SNAPSHOT_WEEKLY_ACTION: &str = "snapshot before weekly window resets";

/// Returns `Some(QuotaKind)` when the recommendation is a
/// `snapshot_before_reset` advisory (5h or weekly), `None` otherwise.
/// Phase H acts only on these two action strings; every other
/// recommendation flows through unchanged.
pub fn extract_quota_kind(rec: &Recommendation) -> Option<QuotaKind> {
    match rec.action {
        SNAPSHOT_5H_ACTION => Some(QuotaKind::FiveHour),
        SNAPSHOT_WEEKLY_ACTION => Some(QuotaKind::Weekly),
        _ => None,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib auto_snapshot::tests::extract_kind
```

Expected: 4 PASS.

- [ ] **Step 5: Add a coverage guard against the rule-side action constants drifting**

Append a guard test that pins the constants to the actual rule output:

```rust
    #[test]
    fn action_constants_match_reset_rule_output() {
        use crate::domain::identity::{Identity, IdentityConfidence, ResolvedIdentity, Provider, Role};
        use crate::domain::signal::{MetricValue, SignalSet};
        use crate::policy::gates::PolicyGates;
        use crate::policy::rules::reset::eval_reset;

        let id = ResolvedIdentity {
            identity: Identity {
                provider: Provider::Claude,
                instance: "1".into(),
                role: Role::Main,
                pane_id: "%1".into(),
            },
            confidence: IdentityConfidence::High,
        };

        // Build a SignalSet that fires both 5h and weekly recommendations.
        let mut signals = SignalSet::default();
        let now: u64 = 1_700_000_000;
        signals.quota_5h_pressure = Some(MetricValue { value: 0.95, source_kind: SourceKind::ProviderOfficial });
        signals.quota_5h_resets_at = Some(MetricValue { value: now + 60, source_kind: SourceKind::ProviderOfficial });
        signals.quota_weekly_pressure = Some(MetricValue { value: 0.95, source_kind: SourceKind::ProviderOfficial });
        signals.quota_weekly_resets_at = Some(MetricValue { value: now + 60, source_kind: SourceKind::ProviderOfficial });

        let mut gates = PolicyGates::default();
        gates.identity_confidence = IdentityConfidence::High;

        let recs = eval_reset(&id, &signals, &gates, now);
        let snapshot_actions: Vec<&str> = recs
            .iter()
            .filter(|r| r.action.starts_with("snapshot before"))
            .map(|r| r.action)
            .collect();
        assert!(snapshot_actions.contains(&SNAPSHOT_5H_ACTION),
            "5h action constant drifted from rule output: {snapshot_actions:?}");
        assert!(snapshot_actions.contains(&SNAPSHOT_WEEKLY_ACTION),
            "weekly action constant drifted from rule output: {snapshot_actions:?}");
    }
```

You may need to adjust the imports in this guard test to match the actual `Identity`/`Role` enum paths. If `MetricValue.value` is typed `f32` for pressure but `u64` for `resets_at`, two `MetricValue` literals are necessary — match the actual `SignalSet` field types as defined in `src/domain/signal.rs`.

- [ ] **Step 6: Run the guard test, fix any compile errors against actual types, verify pass**

```bash
cargo test --lib auto_snapshot::tests::action_constants_match_reset_rule_output
```

Expected: PASS. If it fails to compile, the test code's `MetricValue { value, source_kind }` literal is the most likely culprit — read `src/domain/signal.rs` for the actual field names and adjust.

- [ ] **Step 7: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green.

- [ ] **Step 8: Commit**

```bash
git add src/app/auto_snapshot.rs
git commit -m "phase H: extract_quota_kind + drift guard against reset rule"
```

---

## Task 5: `maybe_auto_snapshot` orchestrator

**Files:**

- Modify: `src/app/auto_snapshot.rs` (extend module)

- [ ] **Step 1: Write the failing tests**

Append to the `#[cfg(test)] mod tests` block in `src/app/auto_snapshot.rs`:

```rust
    use crate::domain::audit::{AuditEvent, AuditEventKind};
    use crate::store::sink::EventSink;
    use crate::store::{InMemorySink, PaneSnapshot, SnapshotInput, SnapshotWriter};
    use crate::store::paths::QmonsterPaths;
    use crate::app::system_notice::SystemNotice;
    use tempfile::TempDir;
    use std::cell::RefCell;

    /// Simple `SnapshotSinkLike` test double: records calls and
    /// returns `Ok(path)` or `Err(message)` on demand.
    struct SpySink {
        calls: RefCell<Vec<String>>,
        force_err: RefCell<bool>,
    }
    impl SpySink {
        fn new() -> Self { Self { calls: RefCell::new(vec![]), force_err: RefCell::new(false) } }
        fn forced(self) -> Self { *self.force_err.borrow_mut() = true; self }
    }

    fn make_snapshot_5h_rec() -> Recommendation { make_rec("snapshot before 5h window resets") }
    fn make_snapshot_weekly_rec() -> Recommendation { make_rec("snapshot before weekly window resets") }

    fn ctx_paths() -> (TempDir, QmonsterPaths) {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        (td, paths)
    }

    #[test]
    fn auto_snapshot_no_op_when_gate_off() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        let now: u64 = 1_700_000_000;
        let resets_at: u64 = now + 60;

        maybe_auto_snapshot(
            "%1",
            &recs,
            /* gate_on = */ false,
            Some(resets_at),
            None,
            &mut dedup,
            &writer,
            &sink,
            &mut notices,
        );

        assert!(notices.is_empty());
        assert!(sink.snapshot().is_empty());
        assert_eq!(dedup, AutoSnapshotDedup::default());
    }

    #[test]
    fn auto_snapshot_fires_once_per_window() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        let resets_at: u64 = 1_700_000_060;

        // Tick 1: fires.
        maybe_auto_snapshot(
            "%1", &recs, true, Some(resets_at), None, &mut dedup, &writer, &sink, &mut notices,
        );
        // Tick 2: dedup blocks.
        maybe_auto_snapshot(
            "%1", &recs, true, Some(resets_at), None, &mut dedup, &writer, &sink, &mut notices,
        );

        let events: Vec<_> = sink.snapshot();
        let snapshot_events: Vec<_> = events.iter().filter(|e| e.kind == AuditEventKind::SnapshotWritten).collect();
        assert_eq!(snapshot_events.len(), 1, "dedup should block tick 2");
        assert_eq!(notices.len(), 1, "one notice per actuation");
    }

    #[test]
    fn auto_snapshot_5h_and_weekly_independent() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec(), make_snapshot_weekly_rec()];
        let resets_5h: u64 = 1_700_000_060;
        let resets_weekly: u64 = 1_700_500_060;

        maybe_auto_snapshot(
            "%1", &recs, true, Some(resets_5h), Some(resets_weekly),
            &mut dedup, &writer, &sink, &mut notices,
        );

        let events: Vec<_> = sink.snapshot();
        let snapshot_events: Vec<_> = events.iter().filter(|e| e.kind == AuditEventKind::SnapshotWritten).collect();
        assert_eq!(snapshot_events.len(), 2, "both kinds run on the same tick");
        assert_eq!(notices.len(), 2);
        assert_eq!(dedup.quota_5h_window_id, Some(floor_to_minute(resets_5h)));
        assert_eq!(dedup.quota_weekly_window_id, Some(floor_to_minute(resets_weekly)));
    }

    #[test]
    fn auto_snapshot_new_window_resets_dedup() {
        let (_td, paths) = ctx_paths();
        let writer = SnapshotWriter::new(paths.clone());
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        // First window.
        maybe_auto_snapshot(
            "%1", &recs, true, Some(1_700_000_060), None, &mut dedup, &writer, &sink, &mut notices,
        );
        // Second window — new resets_at differs by more than a minute.
        maybe_auto_snapshot(
            "%1", &recs, true, Some(1_700_018_000), None, &mut dedup, &writer, &sink, &mut notices,
        );

        let events: Vec<_> = sink.snapshot();
        let snapshot_events: Vec<_> = events.iter().filter(|e| e.kind == AuditEventKind::SnapshotWritten).collect();
        assert_eq!(snapshot_events.len(), 2, "second window allows a fresh snapshot");
    }

    #[test]
    fn auto_snapshot_failure_does_not_dedup() {
        // SnapshotWriter pointed at a file path (rather than a directory)
        // so `create_dir_all` succeeds but `fs::write` to the directory
        // path fails — same trick as `write_operator_snapshot_failure_returns_warning_without_audit`.
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        let writer = SnapshotWriter::new(QmonsterPaths::at(file.path()));
        let sink = InMemorySink::new();
        let mut dedup = AutoSnapshotDedup::default();
        let mut notices: Vec<SystemNotice> = vec![];

        let recs = vec![make_snapshot_5h_rec()];
        maybe_auto_snapshot(
            "%1", &recs, true, Some(1_700_000_060), None, &mut dedup, &writer, &sink, &mut notices,
        );

        assert!(sink.snapshot().is_empty(), "no audit event on write failure");
        assert_eq!(dedup, AutoSnapshotDedup::default(), "dedup must NOT update on failure");
        assert_eq!(notices.len(), 1, "operator sees a Warning notice");
        assert_eq!(notices[0].severity, crate::domain::recommendation::Severity::Warning);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib auto_snapshot::tests::auto_snapshot_
```

Expected: 5 FAIL with `cannot find function 'maybe_auto_snapshot'`.

- [ ] **Step 3: Implement `maybe_auto_snapshot`**

Add to `src/app/auto_snapshot.rs` between `extract_quota_kind` and the `#[cfg(test)]` block:

```rust
use crate::app::system_notice::SystemNotice;
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;
use crate::store::sink::EventSink;
use crate::store::{SnapshotInput, SnapshotWriter};

/// Phase H entry point. Inspects `recs` for any
/// `snapshot_before_reset` recommendations and, when
/// `auto_snapshot_enabled` is true and the matching
/// `quota_*_resets_at` value is fresh (different from the dedup
/// state), writes a snapshot, records a `SnapshotWritten` audit
/// event, and pushes a Concern-severity `SystemNotice`. On
/// snapshot-writer failure, pushes a Warning `SystemNotice` and
/// leaves dedup untouched so the next tick can retry.
///
/// Pure over `dedup`, `writer`, `sink`, `notices` — none of the
/// callers' surfaces are mutated except via these arguments.
#[allow(clippy::too_many_arguments)]
pub fn maybe_auto_snapshot(
    pane_id: &str,
    recs: &[Recommendation],
    auto_snapshot_enabled: bool,
    quota_5h_resets_at: Option<u64>,
    quota_weekly_resets_at: Option<u64>,
    dedup: &mut AutoSnapshotDedup,
    writer: &SnapshotWriter,
    sink: &dyn EventSink,
    notices: &mut Vec<SystemNotice>,
) {
    if !auto_snapshot_enabled {
        return;
    }

    for rec in recs {
        let kind = match extract_quota_kind(rec) {
            Some(k) => k,
            None => continue,
        };

        let resets_at = match kind {
            QuotaKind::FiveHour => quota_5h_resets_at,
            QuotaKind::Weekly => quota_weekly_resets_at,
        };
        let resets_at = match resets_at {
            Some(v) => v,
            // Rule fired but signal absent — the rule body checks
            // `resets_at` itself, so this path should be unreachable
            // in practice. Be defensive and skip.
            None => continue,
        };
        let window_id = floor_to_minute(resets_at);

        if dedup.matches(kind, window_id) {
            continue;
        }

        let input = SnapshotInput {
            reason: format!(
                "auto_reset_boundary (kind={}, window_id={window_id})",
                kind.label()
            ),
            pane_summaries: vec![],
            notices: vec![],
        };

        match writer.write(&input) {
            Ok(path) => {
                sink.record(AuditEvent {
                    kind: AuditEventKind::SnapshotWritten,
                    pane_id: pane_id.to_string(),
                    severity: Severity::Safe,
                    summary: format!(
                        "auto-snapshot trigger=auto_reset_boundary quota_kind={} window_id={window_id} path={}",
                        kind.label(),
                        path.display()
                    ),
                    provider: None,
                    role: None,
                });
                notices.push(SystemNotice {
                    title: format!(
                        "auto-snapshot taken at {} reset boundary",
                        kind.label()
                    ),
                    body: format!("see {}", path.display()),
                    severity: Severity::Concern,
                    source_kind: SourceKind::ProjectCanonical,
                });
                dedup.set(kind, window_id);
            }
            Err(e) => {
                notices.push(SystemNotice {
                    title: format!(
                        "auto-snapshot failed at {} reset boundary",
                        kind.label()
                    ),
                    body: e.to_string(),
                    severity: Severity::Warning,
                    source_kind: SourceKind::ProjectCanonical,
                });
                // Do NOT update dedup — let the next tick retry.
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib auto_snapshot::tests::auto_snapshot_
```

Expected: 5 PASS.

- [ ] **Step 5: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/app/auto_snapshot.rs
git commit -m "phase H: maybe_auto_snapshot orchestrator + dedup tests"
```

---

## Task 6: `Context.auto_snapshot_dedup` plumbing

**Files:**

- Modify: `src/app/bootstrap.rs:26-145` (`Context` struct + `Context::new`)

- [ ] **Step 1: Write the failing test**

Append to the existing `#[cfg(test)] mod tests` block in `src/app/bootstrap.rs` (or create one if absent):

```rust
#[test]
fn context_new_initializes_empty_auto_snapshot_dedup() {
    use crate::adapters::polling_fixture::FixtureSource;
    use crate::notify::desktop::TestNotify;
    use crate::store::InMemorySink;

    let ctx = Context::new(
        QmonsterConfig::default(),
        FixtureSource::default(),
        TestNotify::default(),
        Box::new(InMemorySink::new()),
    );
    assert!(ctx.auto_snapshot_dedup.is_empty());
}
```

If `polling_fixture::FixtureSource` or `TestNotify` is not the existing test fixture name, find the fixture used by other Context tests in this file and follow that pattern instead.

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --lib context_new_initializes_empty_auto_snapshot_dedup
```

Expected: FAIL with `error[E0609]: no field 'auto_snapshot_dedup' on type 'Context'`.

- [ ] **Step 3: Add the field to `Context`**

In `src/app/bootstrap.rs`, modify the `Context` struct (around line 26-113). Insert after the `recent_error_observations` field (around line 82-83):

```rust
    /// Phase H (v1.42.0): per-pane dedup state for
    /// `app::auto_snapshot::maybe_auto_snapshot`. Evicted on
    /// `PaneLifecycleEvent::{BecameDead, Reappeared}` so a re-spawned
    /// pane starts fresh for the new reset window.
    pub auto_snapshot_dedup:
        std::collections::HashMap<String, crate::app::auto_snapshot::AutoSnapshotDedup>,
```

In `Context::new` (around line 115-144), insert `auto_snapshot_dedup: std::collections::HashMap::new(),` next to the `recent_error_observations` initializer:

```rust
            recent_error_observations: std::collections::HashMap::new(),
            auto_snapshot_dedup: std::collections::HashMap::new(),
```

- [ ] **Step 4: Add the field to `PaneLifecycle` eviction path (mirror identity_history pattern)**

Search for the existing `identity_history` and `reported_drifts` eviction call sites in `src/app/event_loop.rs` — they live near the lifecycle bookkeeping at the top of `run_once_with_target`. The pattern is something like:

```rust
ctx.identity_history.remove(&dead_id);
```

Find that block and add an analogous removal for `auto_snapshot_dedup`. If there is no explicit removal block (the field is currently fresh in this PR), the lifecycle reset happens through `ctx.lifecycle.forget(id)` — search for `forget` and the surrounding cleanup in `src/app/event_loop.rs:50-56`. Mirror whichever pattern the existing per-pane HashMap fields follow.

```rust
// Inside the existing `for id in &known { if !current_ids.contains(id) { ... } }` block:
ctx.auto_snapshot_dedup.remove(id);
```

If no such per-id eviction block exists for `identity_history` either (it may use `lifecycle.forget` only), follow the same convention — the dedup HashMap will be cleared elsewhere via the lifecycle hook. In that case, no event_loop change is needed for this task; defer to Task 7.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test --lib context_new_initializes_empty_auto_snapshot_dedup
```

Expected: PASS.

- [ ] **Step 6: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/app/bootstrap.rs src/app/event_loop.rs
git commit -m "phase H: Context.auto_snapshot_dedup HashMap"
```

---

## Task 7: Wire `maybe_auto_snapshot` into `run_once_with_target`

**Files:**

- Modify: `src/app/event_loop.rs` (one new call between `policy.evaluate` and the cost-budget alert push at lines 250-320)

- [ ] **Step 1: Write the failing integration test**

Find the existing event-loop integration test fixture (search `event_loop::run_once` in `tests/` or `src/app/event_loop.rs`'s own `#[cfg(test)] mod tests`). Append a test that drives a `SignalSet` whose `quota_5h_pressure = 0.95` and `quota_5h_resets_at = now+60`, runs `run_once_with_target`, and asserts:

1. `SnapshotWritten` audit event appears in the in-memory sink with `summary` containing `"trigger=auto_reset_boundary"`.
2. The dedup HashMap entry for the pane is non-empty.
3. The `dashboard.recommendations` (or `EvalOutput.recommendations`) still contains the `snapshot before 5h window resets` recommendation — Phase H is observe-and-act, the recommendation rendering path is unchanged.

```rust
#[test]
fn event_loop_auto_snapshot_fires_when_enabled() {
    // ... build Context with config.reset.auto_snapshot = true,
    // signals fixture with quota_5h_pressure=0.95, resets_at=now+60.
    // After run_once_with_target:
    //   - sink contains AuditEventKind::SnapshotWritten
    //   - ctx.auto_snapshot_dedup[pane_id].quota_5h_window_id is Some
}

#[test]
fn event_loop_auto_snapshot_off_baseline_regression() {
    // Same fixture, config.reset.auto_snapshot = false (default).
    // After run_once_with_target:
    //   - sink does NOT contain SnapshotWritten
    //   - dedup remains empty
    //   - the recommendation still appears in EvalOutput / PaneReport.recommendations
}
```

Look at an existing test in `src/app/event_loop.rs`'s test module that invokes `run_once_with_target` and copy the Context + fixture construction shape. The signal fields you need to populate are `quota_5h_pressure`, `quota_5h_resets_at`, `quota_weekly_pressure`, `quota_weekly_resets_at`. The provider must be `Claude` or `Codex` (Gemini's `quota_*_resets_at` is None — see `src/policy/rules/reset.rs:9-11`).

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib event_loop_auto_snapshot
```

Expected: FAIL — the auto-snapshot path is not yet wired.

- [ ] **Step 3: Add the call site**

In `src/app/event_loop.rs`, locate `let mut out: EvalOutput = ctx.policy.evaluate(...)` near line 250. After the `EvalOutput` is built and the cost-budget alerts have been folded in (around line 320), insert:

```rust
            // Phase H (v1.42.0): opt-in actuation hook on the F-7c
            // recommend_snapshot_before_reset advisory. Reads from the
            // recommendations the rule just produced; writes one snapshot
            // per (pane_id, quota_kind, window_id) triple where
            // window_id = quota_*_resets_at floored to the nearest minute.
            if let Some(writer) = ctx.snapshot_writer.as_ref() {
                let dedup = ctx
                    .auto_snapshot_dedup
                    .entry(pane.pane_id.clone())
                    .or_default();
                let mut auto_notices: Vec<crate::app::system_notice::SystemNotice> = vec![];
                crate::app::auto_snapshot::maybe_auto_snapshot(
                    &pane.pane_id,
                    &out.recommendations,
                    gates.reset_auto_snapshot,
                    signals.quota_5h_resets_at.as_ref().map(|m| m.value),
                    signals.quota_weekly_resets_at.as_ref().map(|m| m.value),
                    dedup,
                    writer,
                    ctx.sink.as_ref(),
                    &mut auto_notices,
                );
                // Forward any auto-snapshot notices to the dashboard
                // notice path — same channel as version-drift notices.
                for n in auto_notices {
                    // The dashboard owns push_notice; expose it via the
                    // existing per-tick dashboard handle. If the runtime
                    // does not yet have a SystemNotice queue here, route
                    // these through the same path Settings overlay /
                    // version drift uses (search for `push_notice` in
                    // `src/app/dashboard_runtime.rs:93` for the canonical
                    // entry point).
                    let _ = n; // placeholder; see ROUTING NOTE below
                }
            }
```

ROUTING NOTE: `Context` does not currently own a `snapshot_writer` field, and `run_once_with_target` runs _before_ the dashboard exists. Two compatible options — pick whichever the surrounding code already uses for similar runtime side-effects:

1. **Option A (preferred):** Add `pub snapshot_writer: Option<SnapshotWriter>` to `Context` next to `archive: Option<ArchiveWriter>` (already there). Initialize it in the same place `archive` is wired in `src/app/startup.rs` or wherever the runtime root is built. Push the `auto_notices` into a per-iteration `SystemNotice` queue that the caller of `run_once_with_target` already drains (look at how version-drift notices flow today — search for `route_version_drift` and `system_notice::SystemNotice` consumers).

2. **Option B:** Keep the snapshot writer plumbed through the existing operator-snapshot path (`write_operator_snapshot` in `src/app/operator_actions.rs`). Re-export the underlying `SnapshotWriter` in `Context` and drive `maybe_auto_snapshot` from the same hook the operator key `s` uses.

If neither path is obvious, read `write_operator_snapshot` in `src/app/operator_actions.rs:17-48` for the canonical surfaces (`SnapshotWriter`, `EventSink`) and replicate that wiring shape.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib event_loop_auto_snapshot
```

Expected: PASS.

- [ ] **Step 5: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green. The full lib test count goes from 1028 → ~1034 (Task 1: 4, Task 2: 1, Task 3: 4, Task 4: 5 (4 + 1 drift guard), Task 5: 5, Task 6: 1, Task 7: 2 = 22 new lib unit tests; plus ~2 integration tests for Task 7. Adjust the actual count based on what cargo reports.)

- [ ] **Step 6: Commit**

```bash
git add src/app/event_loop.rs src/app/bootstrap.rs
git commit -m "phase H: wire maybe_auto_snapshot into run_once_with_target"
```

---

## Task 8: Settings overlay row

**Files:**

- Modify: `src/app/settings_overlay.rs` (or `src/ui/settings.rs` — whichever owns the `Parameters` and `Rules` tab layouts)

- [ ] **Step 1: Locate the Parameters tab `[reset]` block and the Rules tab `snapshot_before_reset` row**

```bash
grep -nE "\[reset\]|snapshot_before_reset|wait_for_reset" src/app/settings_overlay.rs src/ui/settings.rs 2>/dev/null
```

Read the rows surrounding the existing `wait_pressure_threshold` / `snapshot_pressure_threshold` rows so you can match indentation and tab-routing patterns exactly.

- [ ] **Step 2: Write a failing snapshot test (or extend an existing one)**

Find the existing settings-overlay snapshot test (search `snapshot!` or `assert_snapshot!` in the file). Add a row to the expected snapshot output that includes `auto_snapshot   false (default)` under the `[reset]` group, and an annotation on the `snapshot_before_reset` row. If the file uses inline `assert_eq!` against a string-built layout, append a new assertion for the row text.

If no snapshot test exists for the reset block specifically, add a unit test that builds the `[reset]` Parameters group via the renderer and asserts `auto_snapshot` is present in the output.

- [ ] **Step 3: Run test to verify it fails**

```bash
cargo test --lib settings
```

Expected: FAIL — the new row is missing.

- [ ] **Step 4: Add the Parameters row**

In the renderer for the `[reset]` block, append a row formatted identically to the existing `wait_pressure_threshold` / `snapshot_pressure_threshold` rows (exact pattern depends on file):

```rust
            ("auto_snapshot",
             format!("{} {}",
                if cfg.reset.auto_snapshot { "true" } else { "false (default)" },
                if cfg.reset.auto_snapshot { "(auto-actuates snapshot_before_reset)" } else { "" }
             )),
```

Adjust to match the file's actual row tuple shape — the goal is a single row showing the current value plus a one-phrase explanation when on.

In the Rules tab, find the `snapshot_before_reset` row and append a small annotation: `(auto when [reset] auto_snapshot = true)`. Read the existing row format and follow it.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test --lib settings
```

Expected: PASS.

- [ ] **Step 6: Run full validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/app/settings_overlay.rs src/ui/settings.rs
git commit -m "phase H: Settings Parameters/Rules rows for auto_snapshot"
```

---

## Task 9: Docs sync

**Files:**

- Modify: `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml`

No tests for this task — pure documentation.

- [ ] **Step 1: `docs/ai/UI_MANUAL.md` — Phase H subsection**

Find the existing reset-awareness section (search `RESET` / `reset_before` / `snapshot_before_reset`). Append a subsection like:

```markdown
### Phase H: opt-in auto-snapshot at reset boundary (v1.42.0)

When `[reset] auto_snapshot = true`, Qmonster automatically writes
a snapshot once per `(pane, quota window)` whenever the F-7c
`recommend_snapshot_before_reset` advisory fires. The snapshot
lands under `<qmonster-root>/snapshots/<timestamp>.json` and
records an audit event `SnapshotWritten` with
`trigger=auto_reset_boundary` and `quota_kind=5h|weekly`. A
Concern-severity `SystemNotice` flashes once per write so the
operator sees the path. The recommendation itself is still shown
in the dashboard recommendation panel — Phase H does not consume
or hide it.

Default: `auto_snapshot = false`. v1.41.0 operators with no
`[reset]` section see no behavior change.
```

- [ ] **Step 2: `docs/ai/ARCHITECTURE.md` — paragraph below the F-7d entry**

Find the F-7d block (search `Phase F F-7d`). Append directly below:

```markdown
v1.42.0 ships **Phase H opt-in auto-snapshot at reset boundary**.
A new `app::auto_snapshot::maybe_auto_snapshot` hook in
`src/app/auto_snapshot.rs` runs after `policy.evaluate(...)` in
the event loop. When `[reset] auto_snapshot = true`, it filters
recommendations for the F-7c `snapshot_before_reset` advisory and
writes one snapshot per `(pane_id, quota_kind, window_id)` where
`window_id = quota_*_resets_at` floored to the nearest minute.
The hook reuses the `SnapshotWriter` and `SnapshotWritten` audit
kind unchanged; metadata (`trigger=auto_reset_boundary`,
`quota_kind`, `window_id`) lives in the audit `summary` string.
Failures push a Warning `SystemNotice` and leave dedup untouched
so the next tick can retry. Restart inside a window resets
in-memory dedup, permitting one extra snapshot for that window —
acceptable, the snapshot is harmless. The policy code, the F-7d
thresholds, and the existing dashboard recommendation flow are
all untouched.
```

- [ ] **Step 3: `docs/ai/VALIDATION.md` — extend reset checklist**

Find the reset-related checklist (search `reset` or `Phase F F-7c`). Append:

```markdown
- [x] Phase H opt-in auto-snapshot (v1.42.0): `[reset] auto_snapshot = false`
      by default; `auto_snapshot = true` writes one snapshot per
      `(pane_id, quota_kind, window_id)` triple via
      `app::auto_snapshot::maybe_auto_snapshot`.
```

- [ ] **Step 4: `config/qmonster.example.toml` — comment block under `[reset]`**

Find the existing `[reset]` block. Append a commented stanza:

```toml
# Phase H opt-in auto-snapshot at reset boundary (v1.42.0).
# When true, Qmonster writes one snapshot per (pane, quota window)
# whenever the F-7c recommend_snapshot_before_reset advisory fires.
# Default false: existing operators see no behavior change.
# auto_snapshot = false
```

- [ ] **Step 5: Run validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --lib
```

Expected: all green (docs changes don't move the test count).

- [ ] **Step 6: Commit**

```bash
git add docs/ai/UI_MANUAL.md docs/ai/ARCHITECTURE.md docs/ai/VALIDATION.md config/qmonster.example.toml
git commit -m "phase H: docs (UI_MANUAL, ARCHITECTURE, VALIDATION, example.toml)"
```

---

## Task 10: Release ledger sync — v1.42.0

**Files:**

- Modify: `package.json`, `VERSION.md`, `README.md` (release table), `mission.yaml`, `mission-history.yaml`, `.mission/CURRENT_STATE.md`, `docs/ai/PROJECT_BRIEF.md` (Date + Phase line), `docs/ai/ARCHITECTURE.md` (Date line)

This task follows the v1.39 → v1.40 → v1.41 ledger-sync pattern exactly. Do not touch source code in this task — pure ledger work.

- [ ] **Step 1: Bump `package.json` version**

```bash
grep -n '"version"' package.json
```

Change `"version": "1.41.0"` → `"version": "1.42.0"`.

- [ ] **Step 2: Bump `VERSION.md`**

```bash
grep -n "1.41" VERSION.md
```

Replace `1.41.0` with `1.42.0` and update any one-line summary to reference Phase H opt-in auto-snapshot.

- [ ] **Step 3: Update `README.md` release table**

Add a row for v1.42.0 with a short description: "Phase H: opt-in auto-snapshot at reset boundary (`[reset] auto_snapshot`)".

- [ ] **Step 4: Update `mission.yaml`**

The `Phase` line near the top of `mission.yaml` (search for the v1.41.0 phase enumeration) needs `Phase H` appended after `Phase F F-9b` plus a v1.42.0 description block. The `execution_hints.topology` and `budget_hint.followups` sections need updating to reflect that Phase H is now shipped, leaving Phase 7 v1 as the next followup.

Read the v1.41.0 entries first (see the v1.40 → v1.41 diff in the working tree's git log for the canonical pattern):

```bash
git log -p --follow mission.yaml | head -300
```

Mirror the structure for v1.42.0.

- [ ] **Step 5: Append a v1.42.0 entry to `mission-history.yaml`**

Follow the existing entry shape (`version`, `date`, `summary`, `related_commits`, `notes`). Use the commit SHA range from this PR's commits.

- [ ] **Step 6: Update `.mission/CURRENT_STATE.md`**

Replace the v1.41.0 baseline with v1.42.0:

- `_Last updated: 2026-05-06 (Claude, v1.42.0 release ledger sync)_`
- Mission title and version surfaces line: `1.42.0`.
- Active Follow-Ups #2 (Phase H) → mark complete; Phase 7 v1 promotes to top followup. Reference `docs/superpowers/specs/2026-05-06-phase7-v1-anomaly-overlay-design.md` and `docs/superpowers/plans/2026-05-06-phase7-v1-anomaly-overlay.md` (the plan ships in the v1.43.0 cycle).
- Validation Baseline counts updated (`cargo test --all-targets` lib count, etc.).

- [ ] **Step 7: Update `docs/ai/PROJECT_BRIEF.md` Date and Phase lines**

Append a v1.42.0 segment to the Date line. Append `Phase H` to the Phase line.

- [ ] **Step 8: Update `docs/ai/ARCHITECTURE.md` Date line**

Append a v1.42.0 segment with one sentence summary.

- [ ] **Step 9: Run validation gates**

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
cargo test --all-targets
```

`--all-targets` instead of `--lib` because release-time validation runs the full integration suite per `docs/ai/VALIDATION.md`. Expected: all green.

- [ ] **Step 10: Commit and tag**

```bash
git add package.json VERSION.md README.md mission.yaml mission-history.yaml .mission/CURRENT_STATE.md docs/ai/PROJECT_BRIEF.md docs/ai/ARCHITECTURE.md
git commit -m "v1.42.0 release ledger sync"
git tag v1.42.0
```

CI publication for the v1.42.0 tag follows the v1.37 / v1.38 / v1.39 / v1.40 / v1.41 pipeline path automatically once the tag is pushed; that publication-verification round is a separate followup.

- [ ] **Step 11: Verify final state**

```bash
git log --oneline -12
git diff origin/main..HEAD --stat
```

Expected: ~10 phase-H commits + 1 release ledger sync commit, the v1.42.0 tag pointing at the ledger sync, and roughly the file map listed at the top of this plan.

---

## Self-review

Before declaring the plan complete:

- **Spec coverage:** every section of `docs/superpowers/specs/2026-05-06-phase-h-auto-snapshot-design.md` (§1 Goal through §11 Open questions) is covered by Tasks 1-10 above. §11 open questions are deferred to plan-stage decisions, all of them safely defaulted (recommendation_id = stable hash; notice body = full path; dedup state not exposed in Settings v1).
- **Type consistency:** `AutoSnapshotDedup`, `QuotaKind`, `floor_to_minute`, `extract_quota_kind`, `maybe_auto_snapshot` names are stable across Tasks 3-7. `reset_auto_snapshot` (gate field), `auto_snapshot` (config field), `auto_snapshot_dedup` (Context field) deliberately use slightly different names so the linkage point is obvious.
- **Routing note:** Task 7 Step 3 leaves the `SystemNotice` forwarding path open because the codebase has multiple `push_notice` consumers and the right one depends on whether the existing `Context` already exposes `snapshot_writer`. The implementer is expected to read the surrounding routing one more time and pick the path that matches the version-drift / settings-overlay precedent.
- **Test count:** Tasks 1-7 add ~24 lib unit tests + 2 integration tests. Final `cargo test --lib` should report ~1052 (1028 baseline + 24). Adjust the v1.42.0 release-ledger summary in Task 10 to the actual count cargo reports.

If any of these issues arise during execution, treat them as plan-stage decisions: fix the plan inline before continuing.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-06-phase-h-auto-snapshot.md`.

Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration. REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`.
2. **Inline Execution** — execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
