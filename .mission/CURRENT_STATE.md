# CURRENT_STATE

_Last updated: 2026-05-07 (Claude, v1.45.0 release ledger sync)_

## Mission

- Title: Qmonster v1.45.0 - Phase 7 v2 detectors: 4 new AnomalyKind variants (CostSlope/TokenSlope/MemoryGrowth/SubagentSideEffect) on top of v1.44.0 promotion infrastructure.
- Version surfaces: mission ledger target `1.45.0`; npm package metadata `qmonster@1.45.0`; latest local Git tag `v1.45.0`.
- Branch / worktree at handoff start: `main`, tag `v1.45.0`.
- Release publication state: v1.44.0 is published. `Release and Package Mirror` workflow run `25476534645` (2026-05-07, 6m48s, success) created GitHub Release `v1.44.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.44.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`), v1.42.0 (`25472444159`), v1.43.0 (`25474748447`) publications also remain live — `npm view qmonster versions` lists `1.37.0` through `1.44.0` with `dist-tags.latest = 1.44.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44}.0`. v1.45.0 CI publication is a pending external follow-up.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, Phase 7 v1 anomaly observation surface, Phase 7 v2 promotion, and Phase 7 v2 detectors are complete.

## v1.45.0 Feature State

Phase 7 v2 detectors adds 4 new `AnomalyKind` variants on top of the v1.44.0 promotion infrastructure (`severity_for` + `promote_anomalies_to_recommendations` + event_loop wiring). It does not change provider adapters, audit chain, the `SignalSet` schema, the SQLite schema, or the existing v1.44.0 detector/promotion logic. All 4 new detectors use the same edge-triggered dedup and `AnomalyHistory` pattern established in Phase 7 v1.

1. **AnomalyKind::CostSlope / detect_cost_slope** (`src/policy/rules/anomaly.rs`): Detects USD/hour cost rate spikes. Uses `AnomalyHistory` cost deque samples; fires `AnomalyKind::CostSlope` when the rolling rate exceeds `AnomalyConfig.cost_slope_usd_per_hour`. Confidence-mapped through `severity_for`; High → Warning (promotes to Recommendation + Notify), Medium/Low → Concern (passive observation only).

2. **AnomalyKind::TokenSlope / detect_token_slope** (`src/policy/rules/anomaly.rs`): Detects `input_tokens/poll` rate spikes. Uses `AnomalyHistory` token slope deque; fires `AnomalyKind::TokenSlope` when the rolling rate exceeds `AnomalyConfig.token_slope_per_poll`.

3. **AnomalyKind::MemoryGrowth / detect_memory_growth** (`src/policy/rules/anomaly.rs`): Detects RSS process MB growth. Uses `AnomalyHistory` memory MB deque; fires `AnomalyKind::MemoryGrowth` when growth exceeds `AnomalyConfig.memory_growth_mb`.

4. **AnomalyKind::SubagentSideEffect / detect_subagent_side_effect** (`src/policy/rules/anomaly.rs`): Detects orchestrator-hop side-effect correlation. Tracks orchestrator hop state in `AnomalyHistory`; fires `AnomalyKind::SubagentSideEffect` on correlated side-effect patterns.

5. **AnomalyHistory extension** (`src/policy/rules/anomaly.rs`): Extended with 6 new deques to support the 4 new detectors; `trim` extended accordingly. No SQLite schema changes — history remains in-memory per session.

6. **AnomalyConfig thresholds** (`src/policy/rules/anomaly.rs`): 3 new operator-tunable thresholds: `cost_slope_usd_per_hour`, `token_slope_per_poll`, `memory_growth_mb`. Defaults in `config/qmonster.example.toml`.

7. **PolicyGates** (`src/policy/gates.rs`): 3 new `anomaly_*` fields: `anomaly_cost_slope`, `anomaly_token_slope`, `anomaly_memory_growth`. Same gate pattern as the original 4 anomaly gate fields from Phase 7 v1.

8. **event_loop wiring** (`src/app/event_loop.rs`): History push +6 lines wiring the 4 new detectors into the `eval_anomalies` call path. The existing `promote_anomalies_to_recommendations` call already handles all 8 `AnomalyKind` variants via the extended promote matrix.

9. **promote matrix extension** (`src/policy/rules/anomaly.rs`): `promote_anomalies_to_recommendations` promote matrix extended with 4 new branches for `CostSlope`, `TokenSlope`, `MemoryGrowth`, and `SubagentSideEffect`. Same `(action, reason, next_step)` pattern as the original 4.

10. **Settings** (`src/ui/settings.rs`): Parameters tab +3 new threshold rows (`cost_slope_usd_per_hour`, `token_slope_per_poll`, `memory_growth_mb`). Rules tab +4 new anomaly detector rows for the 4 new `AnomalyKind` variants, each including the `'promotes to Recommendation when confidence=High'` annotation.

11. **Behavior when off**: when `[anomaly] enabled = false` (the default), all 8 detectors (original 4 + new 4) return empty slices; `promote_anomalies_to_recommendations` receives an empty slice and returns an empty vec; zero behavior change from v1.43.0 baseline applies. Behavior when on: `High`-confidence anomalies for any of the 8 `AnomalyKind` variants produce `Warning`-severity recommendations and desktop notifications. `Medium`/`Low`-confidence anomalies remain visible only in the m overlay ANOMALIES row as passive observation.

## Latest Release Notes

- `v1.45.0` is a minor feature release over the v1.44.0 Phase 7 v2 promotion baseline.
- Local tag: `v1.45.0` points at the v1.45.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.45.0 release ledger sync`.
- Reference plan: `docs/superpowers/plans/2026-05-07-phase7-v2-detectors.md`.
- Reference spec: `docs/superpowers/specs/2026-05-07-phase7-v2-detectors-design.md`.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.45.0 work has landed yet. The v1.45.0 release commit is the immediate parent of the v1.45.0 tag.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`, `1.41.0`, `1.42.0`, `1.43.0`, `1.44.0` with `dist-tags.latest = 1.44.0`.
- v1.45.0 CI publication (GitHub Release + npm package) is a pending external follow-up.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 v3 — operator-tunable promotion thresholds, optional `n` overlay, SQLite persistence for anomaly history.
2. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
3. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.45.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1123 lib tests + 57 integration tests, all green (lib count up from 1097 at v1.44.0; integration count up from 56 at v1.44.0).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.44.0 still apply when the v1.45.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete item is planning Phase 7 v3 — operator-tunable promotion thresholds, optional `n` overlay for anomaly history visualization, and SQLite persistence for anomaly history across sessions. Phase 7 v1 (anomaly observation), v2 promotion, and v2 detectors are all shipped. Tag protection and per-subagent token attribution both need either operator-side action (GitHub Settings) or provider-side support.
