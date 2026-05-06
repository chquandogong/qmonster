# 2026-05-06 — Phase 7 v1: anomaly observation surface (design)

Status: design accepted, plan pending.
Audience: Qmonster TUI maintainers and the Claude/Codex/Gemini implementers.
Baseline: `main` at `21a4aa2` (post-v1.41.0 release ledger sync).

Promotes the preliminary spec at
`.docs/codex/phase7-anomaly-detection-spec.md` (Codex, 2026-04-30)
into a v1-scoped, four-kind first slice. Phase 7 v2 (the remaining
four `AnomalyKind` variants and the Recommendation/Notify
promotion) is intentionally out of scope.

## 1. Goal

Open the original token-optimization plan's "이상 징후 분류 및
재점검 루프" (`.docs/init/Qmonster_종합_기획_보고서_v0.4.0_2026-04-20.md`
§ 3 #3, § 12) by combining four already-collected per-pane history
streams (D2 identity history, F-9 profile-switch error history,
F-3 + F-7b cache history, F-8 cross-pane file findings) into a
single per-pane `Vec<AnomalySignal>` surface. Operators see the
result as a new "ANOMALIES" row in the per-pane card of the
existing `m` (Metrics) overlay.

The first slice ships **observation only** — no Recommendation
emission, no Notify, no audit event, no SQLite table. v1 is the
infrastructure plus four conservative detectors; v2 layers
Recommendation promotion and additional detectors on top.

## 2. Non-goals

- The remaining four `AnomalyKind` variants from the prelim spec —
  `CostSlope`, `TokenSlope`, `MemoryGrowth`, `SubagentSideEffect`.
  These need new slope/growth math that the v1 slice deliberately
  defers.
- Anomaly → `Recommendation` promotion. v1 keeps anomalies on
  their own surface; the existing recommendation panel is
  unchanged.
- Anomaly → desktop `Notify`. The prelim spec already restricts
  this to Warning+ promotions, which v1 does not implement.
- New `AuditEventKind` variants. Anomaly emission is
  observation-only.
- Cross-session persistence. v1 stores anomalies in-memory on
  `PaneReport`; cross-restart continuity ships only if a v2 spec
  asks for it.
- Per-subagent token attribution (the prelim spec's permanently
  deferred D3-C item). Phase 7 v1 does not change this — it only
  packages the existing `subagent_hint` for one detector path,
  with no token attribution at all.
- New evidence types beyond the prelim spec's
  `(metric_name, before, after, sample_count, source_kind)`
  tuple. No raw-tail storage in evidence rows.

## 3. Architecture overview

Anomalies are detected by a new pure rule module that consumes
runtime histories already maintained by the dashboard. The rule
returns a `Vec<AnomalySignal>` per pane per tick, which is attached
to `PaneReport` (or the equivalent dashboard snapshot) and rendered
exclusively inside the m overlay.

| Layer            | File                                                                                                       | Change                                                                                                                                                                                                                                                                                                                                                  |
| ---------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Domain types     | `src/domain/anomaly.rs` (new)                                                                              | `AnomalySignal { kind, confidence, severity, evidence, window_polls, detected_at }`. `enum AnomalyKind { IdentityChurn, ErrorBurst, CacheDiscontinuity, CrossPaneEditCluster }`. `enum AnomalyConfidence { Low, Medium, High }`. `struct AnomalyEvidence { metric_name, before, after, sample_count, source_kind }`. `mod.rs` re-exports.               |
| Rule module      | `src/policy/rules/anomaly.rs` (new)                                                                        | Four `detect_<kind>(...) -> Option<AnomalySignal>` functions plus `pub fn eval_anomalies(pane_id, signals, history, gates, now) -> Vec<AnomalySignal>`. Each detector is pure; orchestrator filters by `gates.anomaly_enabled` and `gates.anomaly_min_confidence`.                                                                                      |
| Config surface   | `src/app/config.rs`                                                                                        | New `AnomalyConfig` with `[anomaly]` section. Fields (all `#[serde(default)]`): `enabled = false`, `window_polls = 20`, `min_confidence = "medium"`, `identity_churn_min_flips = 3`, `error_burst_threshold = 0.5`, `cache_discontinuity_drop = 0.30`, `cross_pane_cluster_min_findings = 3`. Conservative defaults; section absent ⇒ entire layer off. |
| Policy gates     | `src/policy/gates.rs`                                                                                      | `PolicyGates` gains six `anomaly_*` fields populated from `AnomalyConfig` in `from_inputs(PolicyGateInputs<'_>)`.                                                                                                                                                                                                                                       |
| Dashboard wiring | `src/app/dashboard_state.rs` (or `dashboard_runtime.rs`)                                                   | `PaneReport` (or pane snapshot) gains `anomalies: Vec<AnomalySignal>`. The dashboard runtime calls `eval_anomalies(...)` after the existing rule pipeline and stores the result. Rolling history adequacy guarded; gaps yield `None` from the affected detector.                                                                                        |
| m overlay render | `src/app/metrics_overlay.rs` + `src/ui/metrics.rs`                                                         | Per-pane card grows a new "ANOMALIES" row. Hidden when `anomalies.is_empty()`. Format: `ANOMALIES <n> <kind>:<conf>[, <kind>:<conf>][, …]` truncated by card width. Colors follow v1.39 severity-bar palette (Concern = soft yellow, no Notify glow).                                                                                                   |
| Settings overlay | `src/app/settings_overlay.rs`                                                                              | `Parameters` tab grows an `[anomaly]` block. `Rules` tab grows four anomaly rows (one per kind). `Badges` tab grows an `ANOMALIES` description row.                                                                                                                                                                                                     |
| Docs             | `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml` | UI_MANUAL §8.5 (m overlay) gains an "Anomalies row" subsection. ARCHITECTURE picks up a "Phase 7 v1 anomaly surface" paragraph. VALIDATION extends the m-overlay manual check. example config shows commented `[anomaly]` block with all defaults.                                                                                                      |

Reused unchanged:

- D2 `IdentitySnapshot` history (gated by `[security] identity_drift_findings`).
- F-9 `profile_switch` per-pane error history (gated by
  `[profile_switch]`).
- F-3 `recent_token_samples` and F-7b `cache_drift_detected` rule
  output.
- F-8 `ConcurrentFileEdit` findings (gated by
  `[security] cross_pane_file_findings`).
- `RuntimeFact` + `SourceKind` (each `AnomalyEvidence` propagates
  the upstream `SourceKind`).
- v1.39 m-overlay per-card layout, severity bar palette, and
  resize/move geometry. No new modal.

## 4. `[anomaly]` config

```toml
[anomaly]
enabled                          = false  # master switch (Phase 7 v1 default)
window_polls                     = 20     # rolling window length for all four detectors
min_confidence                   = "medium"  # low | medium | high

# Per-detector thresholds (defaults match prelim spec § Configuration)
identity_churn_min_flips         = 3
error_burst_threshold            = 0.5
cache_discontinuity_drop         = 0.30
cross_pane_cluster_min_findings  = 3
```

`enabled = false` ⇒ `eval_anomalies` returns an empty `Vec` on
every tick; UI rendering is a no-op. Operators opt in by adding
`[anomaly] enabled = true` to `qmonster.toml`.

`min_confidence` filters detector output before it reaches
`PaneReport`. Detectors emit Low when one of their inputs is
missing or below saturation, Medium at the configured threshold,
High when the threshold is exceeded by a meaningful margin.
Mapping is detector-specific and documented in
`src/policy/rules/anomaly.rs`.

## 5. Detector matrix

| Kind                   | Input (already collected)                                     | Trigger                                                                                                        | Evidence (per `AnomalyEvidence`)                                                                                                           |
| ---------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `IdentityChurn`        | D2 `IdentitySnapshot` history per pane                        | Inside `window_polls`, count `(provider, worktree)` flips ≥ `identity_churn_min_flips`.                        | `metric = "provider_path"`, `before = "<old>"`, `after = "<new>"`, `sample_count = <flips>`, `source_kind = ProviderOfficial` (D2 source). |
| `ErrorBurst`           | F-9 `profile_switch` per-pane error history                   | Inside `window_polls`, error rate ≥ `error_burst_threshold`.                                                   | `metric = "error_rate"`, `before = "<rate_at_window_start>"`, `after = "<rate_now>"`, `sample_count = <window_polls>`.                     |
| `CacheDiscontinuity`   | F-3 `recent_token_samples` + F-7b `cache_drift_detected` flag | Either F-7b fires ≥ 2 times inside `window_polls`, OR `cache_hit_ratio` drops by ≥ `cache_discontinuity_drop`. | `metric = "cache_hit_ratio"`, `before = "<ratio_at_window_start>"`, `after = "<ratio_now>"`, `sample_count = <samples_seen>`.              |
| `CrossPaneEditCluster` | F-8 `ConcurrentFileEdit` findings                             | Inside `window_polls`, ≥ `cross_pane_cluster_min_findings` findings target the same absolute file path.        | `metric = "concurrent_edit_path"`, `before = "<path>"`, `after = "<finding_count>"`, `sample_count = <finding_count>`.                     |

Each detector returns `None` when its upstream gate is off, when
sample count is insufficient, or when the threshold is not met.
The orchestrator collects the four `Option<AnomalySignal>` values,
filters by `min_confidence`, and applies a per-`(pane_id, kind,
window_start)` dedup so the same anomaly is not re-emitted while
its window is still open.

The `window_start` for dedup is derived as `now -
(window_polls * poll_interval_secs)` rounded to the nearest minute.
A new window begins when the rolling-history slice advances past
the previously-seen `window_start`.

## 6. Data flow (per poll tick)

```
poll → SignalSet
 → policy.evaluate(...)
     → existing rules unchanged
     → rules::anomaly::eval_anomalies(pane_id, signals, runtime_history, gates, now)
         ├─ if !gates.anomaly_enabled                → return Vec::new()
         ├─ for kind in {IdentityChurn, ErrorBurst, CacheDiscontinuity, CrossPaneEditCluster}:
         │   ├─ signal = detect_<kind>(history, gates)?  // Option<AnomalySignal>
         │   ├─ filter by gates.anomaly_min_confidence
         │   ├─ check (pane_id, kind, window_start) dedup
         │   └─ push when fresh
         └─ return Vec<AnomalySignal>
 → PaneReport.anomalies = result          [new field]
 → ui render
     ├─ dashboard pane card  : unchanged
     └─ m overlay pane card  : if !anomalies.is_empty() → render ANOMALIES row
```

`PaneReport` lifecycle: created fresh per tick from the rolling
runtime, so the new `anomalies` field is recomputed without any
explicit invalidation step.

## 7. UI surface — m overlay

In v1.39's per-pane card layout, each card has a body area below
the title. Phase 7 v1 inserts one new line at the bottom of that
body area, after the existing CTX/COST/TOKENS/CACHE/RESET rows:

```
ANOMALIES 2  IdentityChurn:medium, ErrorBurst:high
```

Width handling:

- The `ANOMALIES` count is always shown when `> 0`.
- The kind:confidence list is truncated by available card width;
  truncation appends `, …`.
- When the list does not fit on a single line, the row wraps via
  `wrap_aligned_field` (already in v1.38), preserving the leading
  label.
- When `enabled = false` or all detectors return `None`, the row
  is omitted (no zero-state placeholder); this preserves the
  v1.41.0 m-overlay layout for opt-out operators.

Color: the kind label uses the v1.39 severity-bar foreground
palette indexed by the underlying `severity` field
(Concern → soft yellow, Warning → orange, Risk → red — but v1
detectors only emit Concern). Confidence label is rendered in the
default text color.

The new row participates in the overlay's existing scroll, drag,
and resize behavior (`[/]/=/,/.`) without further wiring.

## 8. UI surface — Settings overlay

- **`Parameters` tab**: new `[anomaly]` group listing `enabled`,
  `window_polls`, `min_confidence`, and four threshold fields with
  their current values. Read-only (matches the existing pattern
  for `[reset]` and `[cache]`).
- **`Rules` tab**: four new rows, one per `AnomalyKind`, each
  showing the trigger condition in human form (e.g. "≥ 3 provider
  flips in 20 polls").
- **`Badges` tab**: a new row `ANOMALIES — anomaly count and kinds
per pane (in m overlay)` documenting the source ("aggregated
  from D2/F-9/F-3/F-7b/F-8 history").

## 9. Error handling

- Upstream gate off (e.g. `[security] identity_drift_findings = false`)
  ⇒ history slice is empty ⇒ detector returns `None`. No error.
- `window_polls` exceeds available history length ⇒ detector treats
  it as "insufficient data", returns `None`.
- `min_confidence` higher than detector output ⇒ filtered out
  silently.
- No I/O paths, no audit writes, no notify emissions ⇒ no new
  failure modes.
- Malformed `[anomaly]` config ⇒ `serde(default)` swallows; layer
  stays off.

## 10. Test plan

| Test                                             | Kind        | Asserts                                                                                                                                 |
| ------------------------------------------------ | ----------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `anomaly_disabled_by_default`                    | unit        | `AnomalyConfig::default().enabled == false`. Missing `[anomaly]` section yields the same.                                               |
| `anomaly_disabled_returns_empty`                 | unit        | `gates.anomaly_enabled = false` ⇒ `eval_anomalies` returns empty `Vec` regardless of input history.                                     |
| `detect_identity_churn_positive`                 | unit        | Synthetic D2 history with 4 flips inside window ⇒ `Some(AnomalySignal { kind: IdentityChurn, confidence: Medium, evidence_count: 1 })`. |
| `detect_identity_churn_below_threshold`          | unit        | 2 flips inside window ⇒ `None`.                                                                                                         |
| `detect_error_burst_positive`                    | unit        | F-9 history with 12/20 errors ⇒ `Some(AnomalySignal { kind: ErrorBurst, … })`.                                                          |
| `detect_error_burst_below_threshold`             | unit        | 6/20 errors ⇒ `None`.                                                                                                                   |
| `detect_cache_discontinuity_drop_positive`       | unit        | Cache hit ratio drops 0.85 → 0.41 inside window ⇒ `Some(AnomalySignal { kind: CacheDiscontinuity, … })`.                                |
| `detect_cache_discontinuity_repeated_drift`      | unit        | F-7b fires twice inside window without a 30 % drop ⇒ still `Some(AnomalySignal)` (alternate trigger path).                              |
| `detect_cross_pane_edit_cluster_positive`        | unit        | F-8 emits 4 findings on `/abs/foo.rs` inside window ⇒ `Some(AnomalySignal { kind: CrossPaneEditCluster, … })`.                          |
| `detect_cross_pane_edit_cluster_different_paths` | unit        | 4 findings spread across 4 different paths ⇒ `None` (cluster requires same path).                                                       |
| `anomaly_min_confidence_filter`                  | unit        | Detector emits Medium signal; `gates.anomaly_min_confidence = High` ⇒ filtered out.                                                     |
| `anomaly_dedup_window`                           | unit        | Same pane + kind + window_start across two ticks ⇒ second tick emits empty.                                                             |
| `anomaly_event_loop_integration`                 | integration | Synthetic input drives an IdentityChurn detection ⇒ `PaneReport.anomalies` length 1, m-overlay snapshot shows the new row.              |
| `anomaly_off_baseline_regression`                | integration | Default config (`enabled = false`) drives the same input ⇒ `PaneReport.anomalies` empty, m-overlay layout matches v1.42.0 baseline.     |

Existing 1028 lib + 50 event-loop integration + 18 false-positive
regression + 6 idle-state regression tests must remain green at the
v1.43.0 release commit. Phase 7 v1 grows the lib count by ~14
(the 12 unit tests above + 2 small struct/serde tests).

## 11. Validation gates (release ledger)

Phase 7 v1 ships as v1.43.0 layered on Phase H's v1.42.0. Required
gates at the release-ledger-sync commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — expected ~1048 lib tests (+14 vs
  v1.42.0).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`
- `scripts/release/dry-run.sh` — full local mirror including SBOM
  diff guard.

## 12. v2 follow-ups (deliberately deferred)

- The remaining four `AnomalyKind` variants (`CostSlope`,
  `TokenSlope`, `MemoryGrowth`, `SubagentSideEffect`) and their
  detectors. Adds new slope/growth math; not load-bearing for v1.
- Anomaly → `Recommendation` promotion when `severity ≥ Warning`
  and confidence is High. Reuses the existing recommendation flow
  rather than introducing a new one.
- Anomaly → desktop `Notify` (Warning+ only, dedup on the same
  key as the recommendation).
- Cross-session persistence in a new `anomaly_summaries` SQLite
  table — only if support workflows demand it.
- Optional `n` overlay (anomaly tab) when the m-overlay row gets
  too dense.
- Per-subagent token attribution remains permanently deferred
  (D3-C honesty note).

## 13. Open questions (none load-bearing for v1)

- Should the m-overlay row include a per-anomaly age (e.g. `@5m`)?
  Defaulting to "no" for now to keep the line short; revisit if
  operators ask for it.
- Should the dedup key include the `confidence` field so a
  Medium → High upgrade emits a fresh row? Defaulting to "no" so
  upgrades are silent during a window; revisit if v2 needs it.
- Should the `Settings` Rules row link directly to the underlying
  upstream gate (D2/F-9/etc.) when that gate is off? Plan-stage
  decision.
