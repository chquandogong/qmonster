# 2026-05-07 — Phase 7 v2 (promotion): anomaly → Recommendation + Notify (design)

Status: design accepted, plan pending.
Audience: Qmonster TUI maintainers and the Claude/Codex/Gemini implementers.
Baseline: `main` at `67b2f2a` (post-v1.43.0 publication-verified, untagged).

## 1. Goal

Promote the Phase 7 v1 anomaly observation surface (v1.43.0) into the
operator-facing recommendation + notify channels. When a v1 detector
fires with `confidence = High`, the surviving `AnomalySignal` becomes
a `Recommendation` (rendered in the dashboard recommendation panel)
and adds `RequestedEffect::Notify` to the per-tick effect set
(desktop notification at the same threshold the F-9b cost-budget
alerts already use).

Phase 7 v2 ships in two cycles. v1.44.0 (this spec) adds the
**promotion infrastructure**, applied to the four v1 detectors
(`IdentityChurn`, `ErrorBurst`, `CacheDiscontinuity`,
`CrossPaneEditCluster`). v1.45.0 (separate spec) will add the
**four new detectors** (`CostSlope`, `TokenSlope`, `MemoryGrowth`,
`SubagentSideEffect`); they automatically inherit the promotion
infrastructure.

## 2. Non-goals

- The remaining four `AnomalyKind` variants (`CostSlope`,
  `TokenSlope`, `MemoryGrowth`, `SubagentSideEffect`). They are
  v1.45.0 scope; this cycle only ships the promotion path applied
  to the existing v1 detectors.
- Cross-session persistence in a new SQLite table. v1's "in-memory
  - restart-rearms" contract is unchanged.
- Optional `n` overlay (separate anomaly tab). The m overlay
  ANOMALIES row stays the only observation surface.
- New `AuditEventKind` variants. Promoted recommendations flow
  through the existing `RecommendationEmitted` path.
- New `RequestedEffect` variants. Notify reuses the existing
  `RequestedEffect::Notify`.
- Operator-tunable promotion thresholds (`[anomaly] promote_min_*`).
  The promotion gate is the natural pair `severity ≥ Warning` AND
  `confidence = High` — encoded as the same condition through the
  detector severity bump (see § 4). v3 may add tunability if
  operator demand surfaces.
- Per-subagent token attribution stays permanently deferred (Phase
  D D3-C honesty note).
- Severity::Risk for anomalies. v2 caps promotion at Warning;
  Risk is reserved for future cycles or operator-explicit knobs.

## 3. Architecture overview

`eval_anomalies` already produces `Vec<AnomalySignal>` per pane per
tick (v1.43.0). v2 layers a new pure pipeline stage:

```
Vec<AnomalySignal>
   │  (filter severity ≥ Warning)
   ▼
promote_anomalies_to_recommendations(&signals) → Vec<Recommendation>
   │
   ▼
out.recommendations.extend(promoted.iter().cloned())
out.effects.push(Notify) when any promoted is Warning+
```

The detector severity comes from confidence: `confidence = High ⇒
severity = Warning`, otherwise `Concern`. v1's hardcoded
`Severity::Concern` becomes a per-detector `let severity =
severity_for(confidence)` line. The promotion stage is one new
pure function that takes a slice of signals and returns a vec of
recommendations, mirroring `cost_budget::recommendation_for_budget_alert`.

| Layer             | File                                                                       | Change                                                                                                                                                                                                                                                                                                                                                         |
| ----------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Detectors         | `src/policy/rules/anomaly.rs`                                              | Each of the four `detect_*` functions replaces its hardcoded `severity: Severity::Concern` with `severity: severity_for(confidence)` where `severity_for(High) = Warning` and `severity_for(_) = Concern`. New private helper `fn severity_for(c: AnomalyConfidence) -> Severity`.                                                                             |
| Promote module    | `src/policy/rules/anomaly.rs`                                              | New `pub fn promote_anomalies_to_recommendations(signals: &[AnomalySignal]) -> Vec<Recommendation>` — filters by `severity >= Severity::Warning`, maps each kind to a `Recommendation` with kind-specific `action` / `reason` / `next_step` (see § 4).                                                                                                         |
| Event-loop wiring | `src/app/event_loop.rs`                                                    | After `eval_anomalies` (v1 wiring) and PaneReport assembly: call `promote_anomalies_to_recommendations(&anomalies)` once. Push results into `out.recommendations` (or the equivalent dashboard recommendation accumulator). When any promoted rec is `>= Severity::Warning`, push `RequestedEffect::Notify` (mirroring F-9b cost-budget pattern at line ~314). |
| Settings overlay  | `src/ui/settings.rs`                                                       | `Rules` tab — append `(promotes to Recommendation when confidence=High)` to each of the four anomaly rule rows. Mirrors Phase H Task 8 `(auto when [reset] auto_snapshot = true)` pattern.                                                                                                                                                                     |
| Docs              | `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md` | UI_MANUAL anomaly subsection: add a "Promotion" paragraph. ARCHITECTURE: v1.44.0 entry. VALIDATION: extend the anomaly checklist row with promotion. `config/qmonster.example.toml` is unchanged — no new knobs.                                                                                                                                               |

Reused unchanged:

- `src/domain/anomaly.rs` — types stay identical.
- `src/app/config.rs::AnomalyConfig` — no new fields.
- `src/policy/gates.rs::PolicyGates` — no new fields.
- `Context.anomaly_history`, `Context.anomaly_dedup` — no schema change.
- `eval_anomalies` signature — unchanged (still returns
  `Vec<AnomalySignal>`).
- Edge-triggered dedup — promotion fires once per active window
  naturally because anomaly emission already does.
- m overlay ANOMALIES row — observation surface stays in place
  alongside the new dashboard recommendation panel entry.

## 4. Detector severity bump

Each of the four v1 detectors currently emits `Severity::Concern`
unconditionally. Replace with:

```rust
fn severity_for(confidence: AnomalyConfidence) -> Severity {
    match confidence {
        AnomalyConfidence::High => Severity::Warning,
        _ => Severity::Concern,
    }
}
```

In each `detect_*` function, replace the hardcoded
`Severity::Concern` line with `severity: severity_for(confidence),`.
Confidence is computed earlier in each detector body (v1.43.0
already maps detector strength to `Medium` / `High`). No detector
input or threshold changes.

The `Severity::Risk` variant is intentionally unused here. v2 keeps
Warning as the maximum promotion level; Risk is reserved for
future cycles or for operator-explicit override knobs.

## 5. Detector → Recommendation mapping

`promote_anomalies_to_recommendations` builds one `Recommendation`
per Warning+ `AnomalySignal`. Action strings, reason format, and
next-step text are kind-specific. Severity copies from the input
signal. `is_strong = false`, `suggested_command = None`,
`profile = None`, `side_effects = vec![]`.

| AnomalyKind          | `action: &'static str`                        | `reason` template                                                                                                                    | `next_step`                                                                                              |
| -------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------- |
| IdentityChurn        | `"anomaly: identity churn detected"`          | `"pane identity flipped {flips} times in {window} polls — verify operator intent or pane reuse; latest flip: {before} → {after}"`    | `"pause provider switching; review pane assignments"`                                                    |
| ErrorBurst           | `"anomaly: error burst detected"`             | `"error rate {newer_rate:.0}% over the most recent half of the {window}-poll window (baseline {older_rate:.0}%); confidence {conf}"` | `"check the recent provider output; consider profile_switch if errors persist"`                          |
| CacheDiscontinuity   | `"anomaly: cache discontinuity detected"`     | `"cache hit ratio dropped from {before} to {after} (or F-7b cache-drift fired ≥ 2× in {window} polls)"`                              | `"press 's' to snapshot before reset; consider /compact when conversation is large"`                     |
| CrossPaneEditCluster | `"anomaly: cross-pane edit cluster detected"` | `"{count} concurrent file-edit findings on `{path}` in {window} polls — multiple panes editing the same file"`                       | `"coordinate edits between panes; the existing F-8 ConcurrentFileEdit findings panel lists which panes"` |

`source_kind` copies from `AnomalySignal.evidence[0].source_kind`
(matches the upstream signal's authority).

When the input signal is `severity = Concern` (i.e.
`confidence < High`), `promote_anomalies_to_recommendations` skips
it — observation only, no Recommendation, no Notify. The signal
still reaches `PaneReport.anomalies` and the m overlay ANOMALIES
row.

## 6. Data flow (per pane per tick)

```
poll → SignalSet
 → push pre-rule observations into Context.anomaly_history    [v1 unchanged]
 → policy.evaluate(...)                                       [v1 unchanged]
 → push post-rule observations into Context.anomaly_history   [v1 unchanged]
 → eval_anomalies(...) → Vec<AnomalySignal>                   [v1, with confidence-aware severity]
 → promote_anomalies_to_recommendations(&anomalies)           [NEW pure function]
     ├─ for sig in signals where severity >= Warning:
     │   └─ push Recommendation { action, reason, severity: sig.severity, ... }
     └─ return Vec<Recommendation>
 → out.recommendations.extend(promoted.iter().cloned())       [NEW one line]
 → if promoted.iter().any(|r| r.severity >= Warning) {
       out.effects.push(RequestedEffect::Notify);             [NEW one line, F-9b pattern]
   }
 → PaneReport.anomalies = anomalies                           [v1 unchanged]
 → ui render
     ├─ dashboard recommendation panel: shows promoted Recommendations alongside other rules
     ├─ desktop notification: fires when Notify effect is emitted (existing path)
     └─ m overlay pane card: ANOMALIES row still shows the underlying signals
```

Edge-triggered dedup (`Context.anomaly_dedup`) already ensures
each anomaly emits once per active window. The promotion stage
inherits this — no separate dedup needed for promoted
Recommendations or Notify.

## 7. Error handling

- `promote_anomalies_to_recommendations` is pure (no I/O, no
  global state, no panic paths). Empty input or no Warning+
  signals yields an empty Vec; harmless when `out.recommendations`
  receives `[]`.
- The `severity_for` helper is total over `AnomalyConfidence`.
- `[anomaly] enabled = false` (default): `eval_anomalies` returns
  `Vec::new()` → `promote_*` returns `Vec::new()` → `out.recommendations`
  unchanged, `out.effects` unchanged. v1.43.0 baseline behavior is
  preserved end-to-end.
- A future detector that emits `Severity::Risk` would still be
  promoted (the gate is `>= Warning`), Notify would still fire.
  This is forward-compatible.

## 8. UI / docs surfaces

- **Dashboard recommendation panel**: the existing recommendation
  list now includes promoted anomaly recommendations
  (severity Warning, confidence High). They render with the same
  formatting as cost_budget / profile_switch / identity_drift
  recommendations.
- **Desktop notification**: fires once per promoted Recommendation
  per active window (same path as F-9b cost-budget alerts).
- **m overlay ANOMALIES row**: unchanged. Still shows underlying
  `AnomalySignal`s (including Concern-severity Medium-confidence
  signals that did not promote).
- **Settings overlay `Rules` tab**: each of the four anomaly rows
  gains a `(promotes to Recommendation when confidence=High)`
  annotation. Mirrors the Phase H pattern of inline annotations.
- **`UI_MANUAL.md`**: extend the v1.43.0 anomaly subsection with a
  "Promotion to Recommendation + Notify" paragraph explaining when
  the dashboard panel and desktop notification fire.
- **`ARCHITECTURE.md`**: v1.44.0 entry (paragraph below the
  v1.43.0 entry) describing the promotion architecture and reuse
  contract.
- **`VALIDATION.md`**: extend the v1.43.0 anomaly checklist row to
  mention promotion.
- **`config/qmonster.example.toml`**: unchanged. Operator opts in
  with the same `[anomaly] enabled = true` they already used for
  v1.43.0 — no new knobs.

## 9. Test plan

| Test                                                                  | Kind        | Asserts                                                                                                                                                                                                                                          |
| --------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `severity_for_high_returns_warning`                                   | unit        | `severity_for(High) == Warning`                                                                                                                                                                                                                  |
| `severity_for_medium_low_returns_concern`                             | unit        | `severity_for(Medium) == Concern` and `severity_for(Low) == Concern`                                                                                                                                                                             |
| `detect_identity_churn_emits_warning_when_high_confidence`            | unit        | High-confidence input ⇒ `severity == Warning`                                                                                                                                                                                                    |
| `detect_identity_churn_emits_concern_when_medium_confidence`          | unit        | Medium-confidence input ⇒ `severity == Concern` (regression guard)                                                                                                                                                                               |
| `detect_error_burst_emits_warning_when_high_confidence`               | unit        | Same                                                                                                                                                                                                                                             |
| `detect_error_burst_emits_concern_when_medium_confidence`             | unit        | Same                                                                                                                                                                                                                                             |
| `detect_cache_discontinuity_emits_warning_when_high_confidence`       | unit        | Same                                                                                                                                                                                                                                             |
| `detect_cache_discontinuity_emits_concern_when_medium_confidence`     | unit        | Same                                                                                                                                                                                                                                             |
| `detect_cross_pane_edit_cluster_emits_warning_when_high_confidence`   | unit        | Same                                                                                                                                                                                                                                             |
| `detect_cross_pane_edit_cluster_emits_concern_when_medium_confidence` | unit        | Same                                                                                                                                                                                                                                             |
| `promote_returns_empty_for_concern_only`                              | unit        | All-Concern input ⇒ `Vec::new()`                                                                                                                                                                                                                 |
| `promote_emits_recommendation_for_warning_signal`                     | unit        | Single Warning AnomalySignal ⇒ 1 Recommendation                                                                                                                                                                                                  |
| `promote_maps_each_kind_to_distinct_action`                           | unit        | One signal per kind (all Warning) ⇒ 4 distinct `action` strings (no collisions, all matching the matrix in § 5)                                                                                                                                  |
| `promote_recommendation_severity_matches_signal`                      | unit        | `promoted[0].severity == signal.severity` (Warning copies through; Risk would copy through too if a future detector emitted it)                                                                                                                  |
| `promote_recommendation_source_kind_matches_evidence`                 | unit        | `promoted[0].source_kind == signal.evidence[0].source_kind`                                                                                                                                                                                      |
| `event_loop_promotes_warning_anomalies_to_recommendations_and_notify` | integration | Synthetic high-confidence churn fixture ⇒ `PaneReport.recommendations` contains the promoted `"anomaly: identity churn detected"` Recommendation; `PaneReport.effects` contains `RequestedEffect::Notify`; m overlay ANOMALIES row still present |
| `event_loop_no_promotion_when_anomaly_disabled`                       | integration | `[anomaly] enabled = false` (default) ⇒ no promoted recommendations, no Notify effect, baseline v1.43.0 behavior preserved                                                                                                                       |

Existing 1081 lib + 54 event-loop integration tests must remain
green at the v1.44.0 release commit. New: ~13 unit (severity_for ×
2 + detector severity × 8 + promote × 5) + 2 integration. Final:
~1094 lib + 56 integration.

## 10. Validation gates (release ledger)

Phase 7 v2 promotion ships as v1.44.0 layered on v1.43.0. Required
gates at the release-ledger-sync commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — expected ~1094 lib tests (+13 vs
  v1.43.0 baseline of 1081); ~56 event-loop integration tests
  (+2 vs the v1.43.0 baseline of 54).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`
- `scripts/release/dry-run.sh` — full local mirror including SBOM
  diff guard.

## 11. v3 / future follow-ups (deliberately deferred)

- Four new detectors (`CostSlope`, `TokenSlope`, `MemoryGrowth`,
  `SubagentSideEffect`) — v1.45.0 scope (separate spec). They will
  automatically inherit the promotion path built here.
- Operator-tunable promotion thresholds
  (`[anomaly] promote_min_severity`,
  `[anomaly] promote_min_confidence`) — only if operator demand
  surfaces. The current natural pair (Warning + High) should suffice.
- Operator opt-out from Notify
  (`[anomaly] notify_on_promote = false`) — only if operator
  reports the desktop notifications too noisy. F-9b cost-budget
  alerts have no opt-out either, so consistency suggests holding.
- Risk-severity escalation — a future cycle may add per-detector
  rules that bump certain anomalies to Risk severity (e.g., very
  high cost slope). Phase 7 v2 caps promotion at Warning.
- Cross-session anomaly persistence (SQLite `anomaly_summaries`
  table) — only if support workflows demand it. v1's in-memory
  history + restart-rearms is acceptable for v2.
- Optional `n` overlay (anomaly tab) — only if the m overlay
  ANOMALIES row gets too dense for the operator to scan.

## 12. Open questions (none load-bearing for v1.44.0)

- Should the promoted Recommendation include a `suggested_command`
  pointer to a future `:anomaly hide <kind>` operator action? v3
  decision; v2 keeps `suggested_command = None`.
- Should the m overlay ANOMALIES row visually distinguish a
  promoted (Warning) signal from an observation-only (Concern)
  signal? The spec keeps the existing row format unchanged for
  v2; v3 may add a glyph or color cue.
- Should the `[anomaly]` Settings overlay Parameters tab gain a
  count of "promotions this session"? Plan-stage decision; not
  load-bearing.
