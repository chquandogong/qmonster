# CURRENT_STATE

_Last updated: 2026-05-07 (Claude, v1.44.0 release ledger sync)_

## Mission

- Title: Qmonster v1.44.0 - Phase 7 v2 promotion: anomaly → Recommendation + Notify (severity_for + promote_anomalies_to_recommendations + event_loop wiring).
- Version surfaces: mission ledger target `1.44.0`; npm package metadata `qmonster@1.44.0`; latest local Git tag `v1.44.0`.
- Branch / worktree at handoff start: `main`, tag `v1.44.0`.
- Release publication state: v1.44.0 is published. `Release and Package Mirror` workflow run `25476534645` (2026-05-07, 6m48s, success) created GitHub Release `v1.44.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.44.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`), v1.42.0 (`25472444159`), v1.43.0 (`25474748447`) publications also remain live — `npm view qmonster versions` lists `1.37.0` through `1.44.0` with `dist-tags.latest = 1.44.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44}.0`.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, Phase 7 v1 anomaly observation surface, and Phase 7 v2 promotion are complete.

## v1.44.0 Feature State

Phase 7 v2 promotion wires anomaly signals into the Recommendation + Notify path. It does not change provider adapters, audit chain, the `SignalSet` schema, the SQLite schema, or the existing detector logic. The promotion is controlled by `AnomalyConfidence` — only `High`-confidence anomalies produce `Warning`-severity recommendations that trigger desktop notification.

1. **severity_for** (`src/policy/rules/anomaly.rs`): Pure helper that maps `AnomalyConfidence::High` → `Severity::Warning` and `AnomalyConfidence::Medium` / `AnomalyConfidence::Low` → `Severity::Concern`. Used by all four detectors when constructing the `Recommendation` payload.

2. **promote_anomalies_to_recommendations** (`src/policy/rules/anomaly.rs`): Converts a `&[AnomalySignal]` slice into a `Vec<Recommendation>`. Filters out signals with `severity < Warning` (Concern-severity signals are skipped entirely; severity was already stamped on each signal by the detector via `severity_for`). For surviving Warning+ signals, copies `sig.severity` and `evidence[0].source_kind` directly to each emitted `Recommendation` and builds kind-specific `(action, reason, next_step)` from the matrix in the spec. Pure (no IO, no state mutation).

3. **event_loop wiring** (`src/app/event_loop.rs`): `run_once_with_target` calls `eval_anomalies` and then `promote_anomalies_to_recommendations` BEFORE `deliver_effects` and the audit `alert_event` loop — mirroring the F-9b cost-budget pattern at lines 345-362. The promote block explicitly pushes `RequestedEffect::Notify` when any promoted recommendation has `severity >= Warning`. Promoted recommendations land in `out.recommendations` and from there flow through the same audit and Notify-dispatch paths every other rule's recommendations use.

4. **Settings annotation** (`src/ui/settings.rs`): The Rules tab row for each of the four anomaly detectors (`detect_error_burst`, `detect_identity_churn`, `detect_cache_discontinuity`, `detect_cross_pane_edit_cluster`) now includes the text "promotes to Recommendation when confidence=High", making the promotion behavior discoverable without reading source code.

5. **Behavior when off**: when `[anomaly] enabled = false` (the default), `eval_anomalies` returns an empty vec, `promote_anomalies_to_recommendations` receives an empty slice and returns an empty vec, and zero behavior change from v1.43.0 applies. Behavior when on: `High`-confidence anomalies now trigger `Warning`-severity recommendations and desktop notifications. `Medium`/`Low`-confidence anomalies produce NO recommendations (filtered out by `promote_anomalies_to_recommendations`); they remain visible only in the m overlay ANOMALIES row as passive observation, with no Recommendation and no Notify.

## Latest Release Notes

- `v1.44.0` is a minor feature release over the v1.43.0 Phase 7 v1 baseline.
- Local tag: `v1.44.0` points at the v1.44.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.44.0 release ledger sync`.
- Reference plan: `docs/superpowers/plans/2026-05-07-phase7-v2-promotion.md`.
- Reference spec: `docs/superpowers/specs/2026-05-07-phase7-v2-promotion-design.md`.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.44.0 work has landed yet. The v1.44.0 release commit is the immediate parent of the v1.44.0 tag.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`, `1.41.0`, `1.42.0`, `1.43.0`, `1.44.0` with `dist-tags.latest = 1.44.0`.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 v2 detectors (CostSlope/TokenSlope/MemoryGrowth/SubagentSideEffect): add new `AnomalyKind` variants using the same `severity_for` + `promote_anomalies_to_recommendations` pattern.
2. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
3. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.44.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1097 lib tests + 56 integration tests, all green (lib count up from 1081 at v1.43.0; integration count up from 54 at v1.43.0).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.43.0 still apply when the v1.44.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete item is planning Phase 7 v2 detectors (adding new `AnomalyKind` variants — `CostSlope`, `TokenSlope`, `MemoryGrowth`, `SubagentSideEffect` — using the `severity_for` + `promote_anomalies_to_recommendations` pattern already in place). Phase 7 v1 and v2 promotion are both shipped and v1.43.0 is published. Tag protection and per-subagent token attribution both need either operator-side action (GitHub Settings) or provider-side support.
