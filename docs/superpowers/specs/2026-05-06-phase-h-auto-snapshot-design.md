# 2026-05-06 — Phase H: opt-in auto-snapshot at reset boundary (design)

Status: design accepted, plan pending.
Audience: Qmonster TUI maintainers and the Claude/Codex/Gemini implementers.
Baseline: `main` at `21a4aa2` (post-v1.41.0 release ledger sync).

## 1. Goal

Close the last automation gap of the original token-optimization
five-layer model (`.docs/init/Qmonster_종합_기획_보고서_v0.4.0_2026-04-20.md`
§ 9 layer 3 "Archive + checkpoint"): when a 5h or weekly quota window
is about to reset and the F-7c rule already recommends a pre-reset
snapshot, an operator who opted in via `[reset] auto_snapshot = true`
gets the snapshot taken automatically — once per pane per reset
window. No new policy threshold, no new audit event variant, no
provider adapter change.

The snapshot itself is identical to the existing manual
pre-reset snapshot the operator gets today by acting on the F-7c
recommendation; only the actuation step is automated.

## 2. Non-goals

- New policy threshold. Phase H consumes the existing F-7d-tunable
  `[reset]` thresholds (`snapshot_pressure_threshold = 0.50`,
  `snapshot_eta_secs = 300`).
- New `AuditEventKind` variant. Auto-runs reuse the existing
  `SnapshotWritten` kind and add metadata fields for trigger
  attribution.
- Per-pane / per-provider opt-in. v1 ships a single global flag.
  Granularity can be revisited if multiple operators ask for it.
- Auto-`/compact`, auto-`/clear`, auto-prompt-send, or any
  destructive automation. Phase H only writes a snapshot file.
- Cross-session dedup persistence. Dedup is in-memory; a process
  restart inside a reset window may permit one extra snapshot, which
  is acceptable.
- Persisting auto-snapshot history to a new SQLite table. Existing
  audit log + filesystem listing under `<qmonster-root>/snapshots/`
  are sufficient.

## 3. Architecture overview

`policy/rules/reset.rs` already produces `Recommendation`s for
`snapshot_before_reset`. Phase H adds an actuation hook that
consumes those recommendations after the policy stage and, when
opted in, runs the snapshot writer once per `(pane_id, quota_kind,
window_id)` triple.

| Layer             | File                                                                                                       | Change                                                                                                                                                                                                                                                     |
| ----------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Config surface    | `src/app/config.rs`                                                                                        | `ResetConfig` gains `auto_snapshot: bool` (`#[serde(default)]`, default `false`). Existing `[reset]` section, no new section.                                                                                                                              |
| Policy gates      | `src/policy/gates.rs`                                                                                      | `PolicyGates` gains `reset_auto_snapshot: bool`, populated from `config.reset.auto_snapshot` in `from_inputs(PolicyGateInputs<'_>)` (existing pattern from F-7-config / F-7d).                                                                             |
| Actuation module  | `src/app/auto_snapshot.rs` (new)                                                                           | Single entry point `pub fn maybe_auto_snapshot(pane_id, recs, gates, dedup, snapshot_sink, audit, notice_bus, now) -> ()`. Pure orchestrator that filters recommendations, checks dedup, dispatches snapshot+audit+notice, updates dedup on success.       |
| Event-loop wiring | `src/app/event_loop.rs`                                                                                    | One call to `auto_snapshot::maybe_auto_snapshot` after `policy.evaluate(...)`, before `dashboard.push_recommendation`. The recommendation is still rendered in the dashboard recommendation panel — Phase H does not consume / hide / dedup the UI side.   |
| Dedup state       | `src/app/dashboard_state.rs` (or `dashboard_runtime.rs`)                                                   | `auto_snapshot_dedup: HashMap<PaneId, AutoSnapshotDedup>` where `AutoSnapshotDedup { quota_5h_window_id: Option<u64>, quota_weekly_window_id: Option<u64> }`. `window_id` = the provider-reported `quota_*_resets_at`. In-memory, cleared on process exit. |
| Audit metadata    | `src/store/audit.rs`                                                                                       | No new variant. Existing `SnapshotWritten` event payload extended with optional metadata keys: `trigger = "auto_reset_boundary"`, `quota_kind = "5h" \| "weekly"`, `recommendation_id = …`. Manual snapshots omit these keys.                              |
| Notice surface    | `src/app/system_notice.rs`                                                                                 | Concern-severity `SystemNotice` on every successful auto-snapshot ("auto-snapshot taken at 5h reset boundary, see snapshots/<file>"). Reuses the v1.41.0 `SystemNotice` push path.                                                                         |
| Settings overlay  | `src/app/settings_overlay.rs`                                                                              | `Parameters` tab: `[reset]` section gets a row for `auto_snapshot`. `Rules` tab: existing `wait_for_reset` / `snapshot_before_reset` rows gain a one-line note "(auto-actuated when `[reset] auto_snapshot = true`)".                                      |
| Docs              | `docs/ai/UI_MANUAL.md`, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml` | Add a "Phase H auto-snapshot" subsection to UI_MANUAL; ARCHITECTURE picks up a Phase H paragraph below the F-7d entry; VALIDATION extends the reset checklist; example config shows `auto_snapshot = false` as a comment block.                            |

Reused unchanged:

- `src/policy/rules/reset.rs` — emits the same `Recommendation` it has
  emitted since v1.34.0 (F-7c). Phase H does not modify the rule.
- `src/store/snapshots.rs` — the snapshot writer is invoked through
  the same `SnapshotSink` trait the manual path uses.
- `src/store/audit.rs::AuditEventKind::SnapshotWritten` — re-used
  with extended metadata.
- F-7d `[reset]` thresholds — Phase H reads via `PolicyGates`, never
  re-evaluates.

## 4. `[reset] auto_snapshot` config

```toml
[reset]
# (existing F-7d fields)
wait_pressure_threshold     = 0.85
wait_eta_secs               = 1800
snapshot_pressure_threshold = 0.50
snapshot_eta_secs           = 300

# Phase H (default false)
auto_snapshot               = false
```

Default false ⇒ a config that does not mention `auto_snapshot`
loads at `false`, so existing v1.41.0 operators see no behavior
change.

`auto_snapshot = true` ⇒ on every poll tick, after policy
evaluation, Phase H scans the freshly emitted recommendation set
for `snapshot_before_reset` items and acts on each one whose dedup
key has not been seen.

## 5. Dedup model

The dedup key is `(pane_id, quota_kind, window_id)`:

- `pane_id` — the existing pane identity scope.
- `quota_kind` — `"5h"` or `"weekly"`. The two windows are
  independent: a single tick can fire both `snapshot_before_reset`
  recommendations, and Phase H must run snapshots for both.
- `window_id` — the provider-reported reset boundary
  `quota_5h_resets_at` or `quota_weekly_resets_at` (already in
  `SignalSet` via F-5b / F-6), rounded to the nearest minute.
  This Unix timestamp is stable across every tick that lies inside
  the same window, so two ticks fired within one window agree on
  the same key. The `pressure` field is not used for the key
  because it slides every tick (floating-point drift); we only use
  it to detect that a recommendation fired at all.

State lives in
`AutoSnapshotDedup { quota_5h_window_id: Option<u64>,
quota_weekly_window_id: Option<u64> }` per pane. On a successful
snapshot, the corresponding `Option<u64>` is updated to the current
`window_id`. On the next tick, if the recommendation still fires
with the same `window_id`, dedup blocks the second snapshot. When
the provider rolls a new reset window, the new `window_id` differs
and dedup allows one snapshot for the new window.

Process restart inside a reset window resets the in-memory dedup,
permitting one extra snapshot for that window. This is acceptable —
the extra snapshot is harmless, and persisting dedup to disk is not
worth the schema change in v1.

## 6. Data flow (per poll tick)

```
poll
 → adapters → SignalSet (with quota_*_pressure, quota_*_resets_at)
 → policy.evaluate(...)
     → rules::reset::eval_reset(...)
         → [Recommendation::snapshot_before_reset, …]
 → auto_snapshot::maybe_auto_snapshot(...)
     ├─ if !gates.reset_auto_snapshot              → return
     ├─ for rec in recs where kind == snapshot_before_reset:
     │   ├─ window_id = signals.quota_<kind>_resets_at  (rounded to nearest minute)
     │   ├─ if dedup.matches(pane_id, quota_kind, window_id) → continue
     │   ├─ snapshot_sink.write(pane_id, reason="auto_reset_boundary")
     │   │     on Err(e):
     │   │       ├─ notice_bus.push(Warning, "auto-snapshot failed: …")
     │   │       └─ continue   (do NOT update dedup; allow retry next tick)
     │   ├─ audit.write(SnapshotWritten { metadata: {trigger, quota_kind, recommendation_id} })
     │   ├─ notice_bus.push(Concern, "auto-snapshot taken at <kind> reset boundary, see snapshots/<file>")
     │   └─ dedup.set(pane_id, quota_kind, window_id)
 → dashboard.push_recommendation(...)               [unchanged — recommendation still rendered]
 → ui render
```

The dashboard recommendation flow is left untouched. Operators who
dislike the now-redundant pre-reset snapshot recommendation can
either accept it (no-op because dedup blocks) or hide it through
the existing dashboard hide path.

## 7. Error handling

| Failure                                                                    | Response                                                                                                                                                     |
| -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `snapshot_sink.write` returns `Err`                                        | `notice_bus` Warning + dedup not updated (next tick retries). No audit event. The Recommendation still renders so the operator sees the original suggestion. |
| `audit.write` returns `Err`                                                | Log via existing audit-sink failure path; snapshot file is already written. Dedup is still updated. (Matches existing `SnapshotWritten` failure semantics.)  |
| `[reset]` config has wrong type                                            | `serde(default)` swallows; `auto_snapshot = false`. No regression for v1.41.0 operators with malformed config.                                               |
| Policy emits 0 `snapshot_before_reset` recommendations                     | `maybe_auto_snapshot` is a no-op.                                                                                                                            |
| Two `snapshot_before_reset` recommendations on the same tick (5h + weekly) | Both are evaluated independently; up to two snapshots per tick per pane.                                                                                     |

## 8. UI / docs surfaces

- **Settings overlay (`S`) Parameters tab**: under the existing
  `[reset]` group, add a row `auto_snapshot   false (default)` /
  `auto_snapshot   true  (auto-actuates snapshot_before_reset)`.
- **Settings overlay Rules tab**: the existing
  `snapshot_before_reset` rule row gets a small annotation
  `(auto when [reset] auto_snapshot = true)`.
- **Help overlay (`h`)**: one new line under the `[reset]` group
  explaining the opt-in.
- **`UI_MANUAL.md`**: new subsection in the "Reset awareness"
  chapter — when does it fire, where the snapshot lands, dedup
  guarantee.
- **`ARCHITECTURE.md`**: paragraph below the existing F-7d entry
  documenting the `policy → actuation` separation and dedup.
- **`VALIDATION.md`**: extend the reset-related manual checks with
  one verification step for `auto_snapshot = true`.
- **`config/qmonster.example.toml`**: comment block under
  `[reset]` showing `# auto_snapshot = false` with one-line
  description.

## 9. Test plan

| Test                                      | Kind        | Asserts                                                                                                                                                 |
| ----------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `auto_snapshot_disabled_by_default`       | unit        | `ResetConfig::default().auto_snapshot == false`. Config without `[reset]` block also yields `false`.                                                    |
| `auto_snapshot_no_op_when_gate_off`       | unit        | `gates.reset_auto_snapshot = false` ⇒ `maybe_auto_snapshot` invokes `snapshot_sink` 0 times even with a positive recommendation set.                    |
| `auto_snapshot_fires_once_per_window`     | unit        | Two consecutive ticks with the same recommendation + same `window_id` ⇒ snapshot_sink.write called once; dedup blocks the second.                       |
| `auto_snapshot_5h_and_weekly_independent` | unit        | One tick with both `snapshot_before_reset` recs ⇒ two writes, two audit events, two notices. Dedup tracks both quota_kinds.                             |
| `auto_snapshot_failure_does_not_dedup`    | unit        | `snapshot_sink.write` returns `Err` ⇒ Warning notice, dedup NOT updated, no audit event. Next tick retries.                                             |
| `auto_snapshot_new_window_resets_dedup`   | unit        | First window snapshots, then `window_id` changes ⇒ second snapshot fires successfully.                                                                  |
| `event_loop_auto_snapshot_integration`    | integration | Synthetic SignalSet with `quota_5h_pressure = 0.95, quota_5h_resets_at = now+60` and `auto_snapshot = true` ⇒ snapshot file present, audit row present. |
| `auto_snapshot_off_baseline_regression`   | integration | Same input with `auto_snapshot = false` (default) ⇒ no snapshot file, recommendation still in `dashboard.recommendations`.                              |

Existing 1028 lib + 50 event-loop integration + 18 false-positive
regression + 6 idle-state regression tests must remain green at the
v1.42.0 release commit.

## 10. Validation gates (release ledger)

Phase H ships as v1.42.0 layered on v1.41.0. Required gates at the
release-ledger-sync commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — expected ~1034 lib tests (+6 vs
  v1.41.0 baseline of 1028).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`
- `scripts/release/dry-run.sh` — full local mirror including SBOM
  diff guard.

## 11. Open questions (none load-bearing for v1)

- Should the auto-snapshot reuse the existing recommendation's
  `id` field as `recommendation_id` metadata, or generate a fresh
  UUID? Plan-stage decision.
- Should the Concern notice include the snapshot filename or just
  the directory? Defaulting to "filename" for grep-ability.
- Is there a clean way to expose the dedup state in Settings for
  debugging? Probably not in v1; revisit if support questions
  arrive.
