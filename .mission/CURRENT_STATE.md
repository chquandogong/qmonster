# CURRENT_STATE

_Last updated: 2026-05-07 (Claude, v1.43.0 release ledger sync)_

## Mission

- Title: Qmonster v1.43.0 - Phase 7 v1: anomaly observation surface (AnomalySignal + 4 detectors + eval_anomalies + m overlay ANOMALIES row).
- Version surfaces: mission ledger target `1.43.0`; npm package metadata `qmonster@1.43.0`; latest local Git tag `v1.43.0`.
- Branch / worktree at handoff start: `main`, tag `v1.43.0`.
- Release publication state: v1.42.0 is published. `Release and Package Mirror` workflow run `25472444159` (2026-05-07, 6m41s, success) created GitHub Release `v1.42.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.42.0` to npm + GitHub Packages mirror. v1.43.0 is locally tagged but not yet published — its `Release and Package Mirror` workflow will trigger on the v1.43.0 tag push. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`) publications also remain live — `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`, `1.41.0`, `1.42.0` with `dist-tags.latest = 1.42.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42}.0`.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, and Phase 7 v1 anomaly observation surface are complete.

## v1.43.0 Feature State

Phase 7 v1 adds the anomaly observation surface. It does not change provider adapters, audit chain, the `SignalSet` schema, or the SQLite schema. `AnomalyHistory` state is session-local and reset on restart. Anomaly detection is entirely opt-in (`[anomaly] enabled = false` by default).

1. **AnomalySignal / AnomalyKind / AnomalyConfidence / AnomalyEvidence** (`src/domain/anomaly.rs`): Core types. `AnomalyKind` variants: `ErrorBurst`, `IdentityChurn`, `CacheDiscontinuity`, `CrossPaneEditCluster`. `AnomalyConfidence` variants: `High`, `Medium`, `Low`. `AnomalyEvidence` carries the supporting metric values that triggered the anomaly.

2. **AnomalyConfig + `[anomaly]` section** (`src/app/config.rs`): `enabled` (bool, default false), `window_polls` (default 20), `min_confidence` (default `"medium"`), and per-detector threshold fields (`identity_churn_min_flips`, `error_burst_threshold`, `cache_discontinuity_drop`, `cross_pane_cluster_min_findings`). Serde defaults preserve v1.42.0 behavior when the section is absent.

3. **PolicyGates anomaly_\* fields + PolicyGateInputs.anomaly** (`src/policy/gates.rs`): `PolicyGates` gains `anomaly_enabled`, `anomaly_min_confidence`, and per-threshold fields wired from `AnomalyConfig`. `PolicyGateInputs` gains an `anomaly` field carrying the current `PaneReport` anomalies into the policy layer.

4. **AnomalyHistory + Context.anomaly_history / anomaly_dedup** (`src/policy/rules/anomaly.rs`, `src/app/bootstrap.rs`): `AnomalyHistory` (in `policy::rules::anomaly`) holds per-pane rolling-window observation history; `Context.anomaly_dedup: HashMap<(String, AnomalyKind), Option<u64>>` (in `bootstrap.rs`) holds per-`(pane_id, AnomalyKind)` edge-triggered dedup so each anomaly fires only on the rising edge (Some-after-None). `Context.anomaly_history: HashMap<String, AnomalyHistory>` holds the per-pane window. Both evicted on `BecameDead | Reappeared`.

5. **Four detectors** (`src/policy/rules/anomaly.rs`):
   - `detect_error_burst`: fires when recent error count exceeds the configured threshold within the observation window.
   - `detect_identity_churn`: fires when pane identity resolution flips repeatedly within the window.
   - `detect_cache_discontinuity`: fires when cache hit ratio drops sharply across recent samples.
   - `detect_cross_pane_edit_cluster`: fires when multiple panes edit overlapping file sets within the window.

6. **eval_anomalies orchestrator** (`src/policy/rules/anomaly.rs`): dispatches to the four detectors, applies `min_confidence` gating, and runs edge-triggered dedup via the caller-owned `Context.anomaly_dedup` map. Returns only rising-edge anomalies per poll tick.

7. **PaneReport.anomalies + event_loop wiring** (`src/app/event_loop.rs`): `PaneReport` gains an `anomalies: Vec<AnomalySignal>` field. `run_once_with_target` pushes per-tick observations (identity snapshot, error_hint, cache_hit_ratio, F-7b cache-drift fire flag, F-8 paths with one-tick lag) into `Context.anomaly_history`, then calls `eval_anomalies` to populate the field. `PolicyGateInputs.anomaly` carries the `[anomaly]` config (not the anomaly results) into `PolicyGates::from_inputs`.

8. **m overlay pane card ANOMALIES row** (`src/ui/metrics.rs`): when anomalies are present for a pane, the m overlay pane card shows an `ANOMALIES` row listing active `AnomalyKind` labels with confidence annotations.

9. **Settings Parameters / Rules / Badges rows** (`src/ui/settings.rs`): Parameters row documents `[anomaly] enabled` and threshold fields; Rules row describes edge-triggering and dedup; Badges row explains confidence labels. Behavior when off: zero behavior change from v1.42.0. Behavior when on: anomalies are detected, deduplicated, and surfaced in the m overlay pane card.

## Latest Release Notes

- `v1.43.0` is a minor feature release over the v1.42.0 Phase H baseline.
- Local tag: `v1.43.0` points at the v1.43.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.43.0 release ledger sync`.
- Reference plan: `docs/superpowers/plans/2026-05-06-phase7-v1-anomaly-overlay.md`.
- Reference spec: `docs/superpowers/specs/2026-05-06-phase7-v1-anomaly-overlay-design.md`.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.43.0 work has landed yet. The v1.43.0 release commit is the immediate parent of the v1.43.0 tag.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`, `1.41.0`, `1.42.0` with `dist-tags.latest = 1.42.0`.
- v1.43.0 is locally tagged but not yet published; the `Release and Package Mirror` workflow run will trigger on the v1.43.0 tag push.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 v2: promote high-confidence anomalies to Recommendations and Notify-level alerts; add new detector kinds (e.g. token_spike, stall_burst).
2. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
3. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.43.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1081 lib tests + 54 event-loop integration tests + 18 false-positive regression tests + 6 idle-state regression tests, all green at the v1.43.0 release commit (+32 lib tests vs the v1.42.0 baseline of ~1049; +2 event-loop integration tests vs the v1.42.0 baseline of ~52).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.42.0 still apply when the v1.43.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete item is planning Phase 7 v2 (promoting anomalies to Recommendations and Notify-level alerts). Phase 7 v1 is shipped. Tag protection and per-subagent token attribution both need either operator-side action (GitHub Settings) or provider-side support. v1.42.0 and v1.43.0 CI publication verification will trigger on the respective tag pushes and are separate verification follow-ups.
