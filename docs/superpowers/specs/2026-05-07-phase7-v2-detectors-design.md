# 2026-05-07 — Phase 7 v2 (detectors): 4 new AnomalyKind variants (design)

Status: design accepted, plan pending.
Audience: Qmonster TUI maintainers and the Claude/Codex/Gemini implementers.
Baseline: `main` at `f3cca73` (post-v1.44.0 publication-verified, untagged).

## 1. Goal

Complete the v1 prelim spec's eight-kind anomaly catalog by adding
the remaining four detectors on top of the v1.44.0 promotion
infrastructure: `CostSlope`, `TokenSlope`, `MemoryGrowth`, and
`SubagentSideEffect`. The first three are slope/growth detectors
that consume `SignalSet` time-series already populated by Phase F
(`cost_usd`, `input_tokens`/`output_tokens`, `process_memory_mb`/
`agent_memory_bytes`). The fourth is a correlation annotator that
fires only when `subagent_hint` is observed alongside other
anomalies in the same window — never standalone, never claims
attribution.

All four detectors plug into the existing `eval_anomalies` →
`promote_anomalies_to_recommendations` pipeline. v2 promotion
behavior carries through automatically: confidence=High → severity
=Warning → Recommendation in the dashboard panel + Notify; confidence
=Medium → Concern → m overlay observation only.

## 2. Non-goals

- Per-subagent token / cost / error attribution. Phase D D3-C
  honesty note: providers do not expose structured per-subagent
  counters. SubagentSideEffect annotates correlation, not
  attribution.
- New `AuditEventKind` variants. Promoted recommendations flow
  through the existing `RecommendationEmitted` path.
- New `RequestedEffect` variants. Notify reuses the existing path.
- New SQLite tables. v2 history extension is in-memory only —
  matches v1's "in-memory + restart-rearms" contract.
- Cross-session anomaly persistence.
- Optional `n` overlay (separate anomaly tab). The m overlay
  ANOMALIES row remains the observation surface for v2.
- Operator-tunable promotion thresholds (`[anomaly] promote_min_*`).
  Promotion uses the existing `severity_for(confidence)` mapping
  shipped in v1.44.0.
- Severity::Risk for anomalies. v2 caps promotion at Warning.
- Output-tokens and agent-memory as primary metrics. The three
  slope detectors track a single primary metric each
  (`cost_usd`, `input_tokens`, `process_memory_mb`); secondary
  signals appear in evidence rows only, not as additional
  thresholds. v3 may add tunability if operator demand surfaces.

## 3. Architecture overview

`src/policy/rules/anomaly.rs::AnomalyHistory` grows five new
deques (cost / two token / two memory / one subagent_hint).
Four new pure detector functions and one new orchestrator hop
(SubagentSideEffect runs after the other seven are resolved so
it can see their results). The promote matrix expands from four
to eight kinds. No change to dedup, no change to the event-loop
control flow beyond extending the existing pre-rule history
push.

| Layer             | File                                                                                                       | Change                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Domain types      | `src/domain/anomaly.rs`                                                                                    | `AnomalyKind` gains four variants: `CostSlope`, `TokenSlope`, `MemoryGrowth`, `SubagentSideEffect`. `label()` extended to four new arms.                                                                                                                                                                                                                                           |
| Config surface    | `src/app/config.rs`                                                                                        | `AnomalyConfig` gains three threshold fields: `cost_slope_usd_per_hour: f64` (default 20.0), `token_slope_input_per_poll: u64` (default 20_000), `memory_growth_mb: f64` (default 1024.0). All `#[serde(default)]`. SubagentSideEffect needs no threshold (correlation-based).                                                                                                     |
| Policy gates      | `src/policy/gates.rs`                                                                                      | `PolicyGates` gains three corresponding `anomaly_*` fields. `from_inputs` propagates from `AnomalyConfig`.                                                                                                                                                                                                                                                                         |
| History surface   | `src/policy/rules/anomaly.rs`                                                                              | `AnomalyHistory` gains five `VecDeque`s: `cost_usd_samples`, `input_token_samples`, `output_token_samples`, `process_memory_samples`, `agent_memory_samples`, `subagent_hint_samples`. `trim(cap)` extended to truncate the new deques.                                                                                                                                            |
| Detectors         | `src/policy/rules/anomaly.rs`                                                                              | Four new pure functions: `detect_cost_slope`, `detect_token_slope`, `detect_memory_growth`, `detect_subagent_side_effect`. The first three follow the v1 detector signature `(history, window_polls, threshold, now) -> Option<AnomalySignal>`. The fourth has `(history, &[other_anomalies], window_polls, now)` because correlation requires reading already-resolved anomalies. |
| Orchestrator      | `src/policy/rules/anomaly.rs::eval_anomalies`                                                              | Expands from 4 to 7 independent detectors. After dedup + min_confidence filter, runs `detect_subagent_side_effect` against the post-dedup `out` slice and (with the same edge-triggered dedup contract) optionally appends an 8th `AnomalySignal`.                                                                                                                                 |
| Promote matrix    | `src/policy/rules/anomaly.rs::promote_anomalies_to_recommendations`                                        | Match arms expand from 4 to 8 kinds. Each new arm builds a kind-specific `(action, reason, next_step)` tuple per § 5.                                                                                                                                                                                                                                                              |
| Event-loop wiring | `src/app/event_loop.rs`                                                                                    | The existing pre-rule history push block (`Phase 7 v1` comment) gains six lines pushing `signals.cost_usd`, `signals.input_tokens`, `signals.output_tokens`, `signals.process_memory_mb`, `signals.agent_memory_bytes`, `signals.subagent_hint` into the new deques. Trim runs once for the whole struct. No change to deliver_effects / audit / promote ordering.                 |
| Settings overlay  | `src/ui/settings.rs`                                                                                       | `Parameters` tab: 3 new rows under `[anomaly]`. `Rules` tab: 4 new rows (one per new detector), each with the `(promotes to Recommendation when confidence=High)` annotation.                                                                                                                                                                                                      |
| Docs              | `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml` | UI_MANUAL anomaly subsection: extend the detector matrix to 8 kinds. ARCHITECTURE: v1.45.0 entry. VALIDATION: extend the anomaly checklist row. example.toml: add the 3 new threshold lines under `[anomaly]`.                                                                                                                                                                     |

Reused unchanged:

- `severity_for(confidence)` helper (v1.44.0).
- `promote_anomalies_to_recommendations` filter contract
  (`severity >= Warning` gate, kind-by-kind matrix).
- `Context.anomaly_history` and `Context.anomaly_dedup`
  HashMaps; lifecycle eviction continues to cover all deques.
- `eval_anomalies` edge-triggered dedup; the fourth detector
  participates the same way.
- F-9b cost-budget Notify pattern at `src/app/event_loop.rs:345-362`
  — the v1.44.0 promote block already mirrors it; v2 detector
  results flow through that promote block unchanged.

## 4. AnomalyConfig + threshold defaults

```toml
[anomaly]
enabled                          = false
window_polls                     = 20
min_confidence                   = "medium"

# v1 detector thresholds (unchanged)
identity_churn_min_flips         = 3
error_burst_threshold            = 0.5
cache_discontinuity_drop         = 0.30
cross_pane_cluster_min_findings  = 3

# v2 detector thresholds (NEW)
cost_slope_usd_per_hour          = 20.0
token_slope_input_per_poll       = 20000
memory_growth_mb                 = 1024
```

`SubagentSideEffect` has no threshold knob — it fires when a
subagent_hint is observed in the window AND any other anomaly
fires in the same `eval_anomalies` invocation. The correlation
contract is binary: present-with-others or silent.

## 5. Detector matrix (v2 — 4 new kinds)

| Kind                 | Input (already collected)                                                            | Trigger                                                                                           | Confidence                                                                 | Evidence                                                                                                                                                                                    |
| -------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CostSlope`          | `cost_usd_samples` deque                                                             | `slope_usd_per_hour ≥ cost_slope_usd_per_hour`. Slope = `(newest − oldest) / window_secs × 3600`. | `slope ≥ 1.5 × threshold` ⇒ High; `slope ≥ threshold` ⇒ Medium.            | `metric = "cost_slope_usd_per_hour"`, `before = "{oldest:.2}"`, `after = "{newest:.2}"`, `sample_count = ratios.len()`, `source_kind = ProviderOfficial`.                                   |
| `TokenSlope`         | `input_token_samples` (primary), `output_token_samples` (evidence-only secondary)    | `slope_per_poll ≥ token_slope_input_per_poll`. Slope = `(newest − oldest) / window_polls`.        | Same High / Medium tiers as CostSlope.                                     | `metric = "input_tokens_per_poll"`, `before = "{oldest}"`, `after = "{newest}"`, `sample_count = window_polls`, `source_kind = ProviderOfficial`. Reason adds output_tokens secondary.      |
| `MemoryGrowth`       | `process_memory_samples` (primary), `agent_memory_samples` (evidence-only secondary) | `growth_mb ≥ memory_growth_mb`. Growth = `newest − oldest` (simple delta over window).            | Same High / Medium tiers.                                                  | `metric = "process_memory_mb"`, `before = "{oldest:.0}"`, `after = "{newest:.0}"`, `sample_count = window_polls`, `source_kind = Estimated`. Reason adds agent_memory secondary.            |
| `SubagentSideEffect` | `subagent_hint_samples` deque + `&[other_anomalies]` from orchestrator               | `subagent_hint = true` ≥ 1 time in window AND `other_anomalies` is non-empty.                     | Always Medium (single-tier — correlation strength is binary, not numeric). | `metric = "subagent_correlation"`, `before = "no_subagent"`, `after = "subagent_active"`, `sample_count = subagent_hint_count`, `source_kind = Estimated`. Reason lists co-occurring kinds. |

Each detector returns `None` when its upstream gate is off (e.g.,
`signals.cost_usd` is `None` for the entire window), when sample
count is insufficient (`< window_polls`), or when the threshold is
not met. The orchestrator collects the four `Option<AnomalySignal>`
values, applies edge-triggered dedup against `Context.anomaly_dedup`
keyed by `(pane_id, kind)`, and filters by `min_confidence`.

`window_secs` for CostSlope = `window_polls × poll_interval_secs`.
The poll interval is fixed at 5 seconds (matches the F-7d reset_eta
constant baseline). No new operator knob for the poll interval —
it lives at the existing tmux poller.

## 6. Orchestrator hop (SubagentSideEffect runs last)

```rust
pub fn eval_anomalies(...) -> Vec<AnomalySignal> {
    if !gates.anomaly_enabled { return Vec::new(); }

    // 1. Run 7 independent detectors (4 v1 + 3 v2 slope)
    let kinds_with_results = vec![
        (IdentityChurn, detect_identity_churn(...)),
        (ErrorBurst, detect_error_burst(...)),
        (CacheDiscontinuity, detect_cache_discontinuity(...)),
        (CrossPaneEditCluster, detect_cross_pane_edit_cluster(...)),
        (CostSlope, detect_cost_slope(...)),
        (TokenSlope, detect_token_slope(...)),
        (MemoryGrowth, detect_memory_growth(...)),
    ];

    // 2. Apply edge-triggered dedup + min_confidence filter (v1.43.0 logic)
    let mut out = Vec::new();
    for (kind, raw) in kinds_with_results { /* existing dedup logic */ }

    // 3. SubagentSideEffect runs LAST against the post-dedup `out` slice
    if let Some(side_effect) = detect_subagent_side_effect(history, &out, window_polls, now) {
        // Same edge-triggered dedup gate against (pane_id, SubagentSideEffect)
        // ... filter by min_confidence ...
        out.push(side_effect);
    }

    out
}
```

The dedup HashMap (`Context.anomaly_dedup`) gains no new key
shape — `(pane_id, AnomalyKind)` already covers all eight kinds.

## 7. Data flow (per pane per tick)

```
poll → SignalSet
 → push pre-rule observations into Context.anomaly_history    [extend with 6 new fields]
 → policy.evaluate(...)                                       [v1 unchanged]
 → push post-rule observations into Context.anomaly_history   [v1 unchanged]
 → eval_anomalies(...)                                        [extended: 7 detectors + 1 correlation hop]
 → promote_anomalies_to_recommendations(&anomalies)           [v1.44.0; matrix extended to 8 kinds]
 → out.recommendations.extend(promoted.iter().cloned())       [v1.44.0]
 → if any promoted is Warning+: out.effects.push(Notify);     [v1.44.0]
 → deliver_effects(...)                                       [v1.44.0 ordering preserved]
 → audit alert_event loop                                     [v1.44.0 ordering preserved]
 → PaneReport.anomalies = anomalies                           [v1 unchanged]
 → ui render
     ├─ dashboard recommendation panel: shows Warning+ promoted recs
     ├─ desktop notification: fires for Warning+ via Notify
     └─ m overlay pane card: ANOMALIES row covers 8 kinds total
```

## 8. Error handling

- All four detectors return `None` when input data is sparse
  (`Option::None` cumulatively or `< window_polls` samples) or
  when threshold is not met. No panic paths.
- `Option<f64>` / `Option<u64>` deque entries: detectors filter
  `None` values via `iter().filter_map(|x| *x)` before computing
  slope. If the resulting non-None vec has `< 2` entries, return
  `None` (insufficient delta).
- `[anomaly] enabled = false` (default): `eval_anomalies` returns
  empty Vec → promote returns empty → no recommendations, no
  Notify. v1.44.0 baseline preserved.
- Provider that does not expose `cost_usd` (some F-9b is opt-in):
  `cost_usd_samples` accumulates `None` values → `detect_cost_slope`
  returns `None`. No fire.
- Provider with no subagent_hint capability (Gemini): `subagent_hint`
  is always false → `detect_subagent_side_effect` always silent.

## 9. UI / docs surfaces

- **m overlay ANOMALIES row**: row template unchanged. Now covers
  8 kinds; format `ANOMALIES <n> <kind>:<conf>[, <kind>:<conf>]`
  scales naturally.
- **Dashboard recommendation panel**: 4 new action strings appear
  alongside the existing 4 + the v1 Recommendations from other
  rules.
- **Desktop notification**: fires for any new Warning+ promoted
  recommendation via the v1.44.0 path.
- **Settings overlay Parameters tab**: 3 new rows
  (`anomaly cost_slope_usd_per_hour`, `anomaly token_slope_input_per_poll`,
  `anomaly memory_growth_mb`) under the `[anomaly]` group.
- **Settings overlay Rules tab**: 4 new rows
  (`anomaly: CostSlope`, `anomaly: TokenSlope`, `anomaly: MemoryGrowth`,
  `anomaly: SubagentSideEffect`), each with the
  `(promotes to Recommendation when confidence=High)` annotation
  for the slope detectors. SubagentSideEffect's annotation reads
  `(promotes when subagent_hint co-occurs with another anomaly)`
  to reflect the binary-confidence semantics.
- **Settings overlay Badges tab**: unchanged.
- **`UI_MANUAL.md`**: extend the v1.43.0 + v1.44.0 anomaly
  section with a "v2 detectors (v1.45.0)" subsection naming the
  4 new kinds and their input signals.
- **`ARCHITECTURE.md`**: v1.45.0 entry below the v1.44.0 entry.
- **`VALIDATION.md`**: extend the anomaly checklist row.
- **`config/qmonster.example.toml`**: add 3 new commented threshold
  lines under the existing `[anomaly]` block.

## 10. Test plan

| Test                                                           | Kind        | Asserts                                                                                                                                                     |
| -------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cost_slope_pure_positive_high_confidence`                     | unit        | Slope ≥ 1.5× threshold ⇒ Some(High, Warning).                                                                                                               |
| `cost_slope_pure_positive_medium_confidence`                   | unit        | Slope ≥ threshold but < 1.5× ⇒ Some(Medium, Concern).                                                                                                       |
| `cost_slope_below_threshold`                                   | unit        | Slope < threshold ⇒ None.                                                                                                                                   |
| `cost_slope_insufficient_samples`                              | unit        | < window_polls samples ⇒ None.                                                                                                                              |
| `token_slope_pure_positive_high_confidence`                    | unit        | Same.                                                                                                                                                       |
| `token_slope_pure_positive_medium_confidence`                  | unit        | Same.                                                                                                                                                       |
| `token_slope_below_threshold`                                  | unit        | Same.                                                                                                                                                       |
| `token_slope_insufficient_samples`                             | unit        | Same.                                                                                                                                                       |
| `memory_growth_pure_positive_high_confidence`                  | unit        | Same.                                                                                                                                                       |
| `memory_growth_pure_positive_medium_confidence`                | unit        | Same.                                                                                                                                                       |
| `memory_growth_below_threshold`                                | unit        | Same.                                                                                                                                                       |
| `memory_growth_insufficient_samples`                           | unit        | Same.                                                                                                                                                       |
| `subagent_side_effect_fires_when_subagent_and_other_anomalies` | unit        | `subagent_hint=true` in window + ≥ 1 other anomaly ⇒ Some(Medium, Concern).                                                                                 |
| `subagent_side_effect_silent_when_no_other_anomalies`          | unit        | `subagent_hint=true` but no other anomalies ⇒ None.                                                                                                         |
| `subagent_side_effect_silent_when_no_subagent_hint`            | unit        | Other anomalies present but `subagent_hint=false` throughout window ⇒ None.                                                                                 |
| `eval_anomalies_runs_subagent_side_effect_after_others`        | unit        | Orchestrator order: SubagentSideEffect sees the post-dedup `out` slice from the other 7 detectors.                                                          |
| `eval_anomalies_returns_8_kinds_when_all_fire`                 | unit        | All 8 detectors fire simultaneously ⇒ result vec contains all 8 distinct kinds.                                                                             |
| `anomaly_history_v2_fields_default_empty`                      | unit        | New 5 deques start empty.                                                                                                                                   |
| `anomaly_history_trim_caps_v2_deques`                          | unit        | `trim(cap)` truncates the new deques.                                                                                                                       |
| `anomaly_config_cost_slope_default`                            | unit        | `cost_slope_usd_per_hour == 20.0`.                                                                                                                          |
| `anomaly_config_token_slope_default`                           | unit        | `token_slope_input_per_poll == 20_000`.                                                                                                                     |
| `anomaly_config_memory_growth_default`                         | unit        | `memory_growth_mb == 1024.0`.                                                                                                                               |
| `anomaly_config_v2_thresholds_load_from_toml`                  | unit        | TOML override sets all 3 to non-default values; defaults preserved when section absent.                                                                     |
| `policy_gates_v2_anomaly_thresholds_propagate`                 | unit        | `from_inputs` propagates 3 new thresholds.                                                                                                                  |
| `promote_matrix_8_kind_distinct_actions`                       | unit        | Promote matrix emits 8 distinct `action` strings (no collisions).                                                                                           |
| `promote_subagent_side_effect_action_format`                   | unit        | SubagentSideEffect's reason includes the co-occurring kinds CSV.                                                                                            |
| `event_loop_v2_detectors_fire_when_enabled_and_history_full`   | integration | Synthetic high-confidence cost slope drives the full pipeline; `PaneReport.recommendations` contains the promoted CostSlope rec; `effects` contains Notify. |
| `event_loop_v2_baseline_no_promotion_when_anomaly_disabled`    | integration | Same fixture with `[anomaly] enabled = false` ⇒ no promoted recs, no Notify, no anomalies in PaneReport.                                                    |

Existing 1097 lib + 56 event-loop integration tests must remain
green at the v1.45.0 release commit. New: ~22 unit + 2 integration.
Final: ~1119 lib + 58 integration.

## 11. Validation gates (release ledger)

Phase 7 v2 detectors ships as v1.45.0 layered on v1.44.0. Required
gates at the release-ledger-sync commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — expected ~1119 lib tests (+22 vs
  v1.44.0 baseline of 1097); ~58 event-loop integration tests
  (+2 vs the v1.44.0 baseline of 56).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`
- `scripts/release/dry-run.sh` — full local mirror including SBOM
  diff guard.

## 12. v3 / future follow-ups (deliberately deferred)

- Per-subagent token / cost / error attribution — permanently
  deferred (Phase D D3-C honesty note).
- Risk-severity escalation. v2 caps promotion at Warning; a v3
  rule may bump certain anomalies (e.g. very high cost slope) to
  Risk severity.
- Operator-tunable promotion thresholds
  (`[anomaly] promote_min_severity`, `[anomaly] promote_min_confidence`).
- Operator opt-out from Notify
  (`[anomaly] notify_on_promote = false`).
- Cross-session anomaly persistence (SQLite `anomaly_summaries`
  table).
- Optional `n` overlay (anomaly tab) — only if the m overlay
  ANOMALIES row gets too dense for the operator to scan.
- TokenSlope tracking `input + output` aggregate — only if
  operator demand surfaces.
- MemoryGrowth tracking `process + agent` aggregate — same.
- Multi-tier confidence for SubagentSideEffect (e.g. High when
  ≥ 2 other anomalies co-fire) — only if v2's binary correlation
  proves too coarse in practice.

## 13. Open questions (none load-bearing for v1.45.0)

- Should the m overlay ANOMALIES row visually distinguish v2
  slope detectors from v1 boolean detectors? Defaulting to "no"
  for v2 — same row format. v3 may add a glyph or color cue.
- Should the SubagentSideEffect Recommendation include a link
  to the subagent's tail or audit history? Not in v2 — the
  `next_step` text just says "review the subagent's recent
  output" without dictating where.
- Should the example.toml ship with the new thresholds
  uncommented (showing operator they exist) vs commented
  (consistent with v1's opt-in style)? Commented for now —
  matches v1 style, less noise for operators who never opt in.
