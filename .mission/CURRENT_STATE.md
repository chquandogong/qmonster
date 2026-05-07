# CURRENT_STATE

_Last updated: 2026-05-07 (Claude, v1.46.0 release ledger sync)_

## Mission

- Title: Qmonster v1.46.0 - Phase 7 v3 (a+b): per-kind promotion gate + n overlay (anomaly events ring buffer)
- Version surfaces: mission ledger target `1.46.0`; npm package metadata `qmonster@1.46.0`; latest local Git tag `v1.46.0`.
- Branch / worktree at handoff start: `main`, tag `v1.46.0`.
- Release publication state: v1.45.0 is published. `Release and Package Mirror` workflow run `25478893257` (2026-05-07, 7m02s, success) created GitHub Release `v1.45.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.45.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`), v1.42.0 (`25472444159`), v1.43.0 (`25474748447`), v1.44.0 (`25476534645`) publications also remain live — `npm view qmonster versions` lists `1.37.0` through `1.45.0` with `dist-tags.latest = 1.45.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45}.0`. v1.46.0 CI publication is a pending external follow-up.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, Phase 7 v1 anomaly observation surface, Phase 7 v2 promotion, Phase 7 v2 detectors, and Phase 7 v3 (a+b) are complete.

## v1.46.0 Feature State

Phase 7 v3 (a+b) adds per-kind promotion gate and the `n` overlay for anomaly events history on top of the v1.45.0 Phase 7 v2 detectors baseline. It does not change provider adapters, audit chain, the `SignalSet` schema, or the SQLite schema.

1. **AnomalyEvent record** (`src/domain/anomaly.rs`): New plain-data struct capturing `kind`, `confidence`, `reason`, `timestamp` (u64 Unix seconds) for a single detected anomaly. Feeds the ring buffer.

2. **AnomalyPromoteConfig** (`src/app/config.rs`): New `[anomaly.promote]` config section with 8 fields — one per `AnomalyKind`. Defaults: 7 × `"high"`, `subagent_side_effect = "medium"`, set by `impl Default for AnomalyPromoteConfig`; `#[serde(default)]` on the sub-struct lets pre-v1.46 toml files load cleanly. The `subagent_side_effect = "medium"` default fixes a v1.45.0 latent dead-code branch (the SubagentSideEffect promote matrix branch was previously unreachable because the old hardcoded `severity ≥ Warning` filter required `High` confidence, but SubagentSideEffect emits `Medium`-only).

3. **AnomalyEventsRing** (`src/app/anomaly_events_ring.rs`): In-memory ring buffer, capacity 100, session-only. Stores `AnomalyEvent` entries in insertion order; `push` evicts oldest when at capacity. `iter()` yields oldest-first; overlay uses `DoubleEndedIterator` for newest-first display.

4. **PolicyGates +8 anomaly_promote_* fields + promote_min_confidence(kind) helper** (`src/policy/gates.rs`): 8 new `anomaly_promote_*` fields (one per `AnomalyKind`) populated from `AnomalyPromoteConfig` in `from_inputs`. `promote_min_confidence(kind: AnomalyKind) -> Confidence` helper returns the per-kind threshold. Replaces the old hardcoded `severity ≥ Warning` filter.

5. **promote_anomalies_to_recommendations: signature change + per-kind gate predicate** (`src/policy/rules/anomaly.rs`): Function now takes `gates: &PolicyGates`; promotion predicate replaced from `severity_for(confidence) >= Severity::Warning` to `confidence >= gates.promote_min_confidence(kind)`. All 8 `AnomalyKind` branches updated. 6 new unit tests.

6. **Context.anomaly_events_ring + push site in run_once_with_target** (`src/app/bootstrap.rs`, `src/app/event_loop.rs`): `Context` gains an `anomaly_events_ring: AnomalyEventsRing` field. `run_once_with_target` pushes each detected anomaly (pre-promotion) into the ring after `eval_anomalies` returns.

7. **AnomalyOverlay state + key handler** (`src/ui/anomaly_overlay.rs`, `src/app/anomaly_overlay.rs`): `AnomalyOverlay` struct with `open: bool`, `scroll: u16`. Key handler: `n` toggles open/closed; `Esc`/`q` closes; `j`/`↓` scroll down, `k`/`↑` scroll up; all keys consumed while open. 11 unit tests.

8. **AnomalyOverlay mouse handler** (`src/app/anomaly_overlay.rs`): Mouse scroll events (`ScrollUp`/`ScrollDown`) adjust `scroll` while overlay is open. 5 unit tests.

9. **AnomalyOverlay render** (`src/ui/anomaly_overlay.rs`): Renders a centered modal over the dashboard. Newest-first list of `AnomalyEvent` entries with truncated `reason` (ellipsize helper). Empty state shown when ring is empty. 3 unit tests.

10. **tui_loop.rs + dashboard_render.rs wiring** (`src/app/tui_loop.rs`, `src/app/dashboard_render.rs`): `anomaly_overlay` is a `let mut` local in `tui_loop.rs::run` (mirroring `metrics_overlay`); `n` key toggles open/closed; mouse events forwarded; overlay rendered on top of the dashboard when open. The render-context struct gains `anomaly_overlay: &AnomalyOverlay` and `anomaly_events_ring: &AnomalyEventsRing` fields. (No `AppState` struct in this codebase — UI overlay state is a `let mut` local; policy state lives on `Context<P, N>`.)

11. **Settings UI: Parameters [anomaly.promote] +8 rows + Rules tab annotation reads from config** (`src/ui/settings.rs`): Parameters tab `[anomaly.promote]` section adds 8 rows (one per kind showing current threshold). Rules tab anomaly detector rows updated to read `min_confidence` from config rather than showing a hardcoded annotation.

12. **(latent v1.45.0 dead-code fix)**: `SubagentSideEffect` default `min_confidence = "medium"` makes its previously-unreachable promote-matrix branch reachable. When `subagent_hint` co-occurs with another anomaly, a `Concern`-severity Recommendation is now promoted as documented (`correlation, not attribution`).

## Latest Release Notes

- `v1.46.0` is a minor feature release over the v1.45.0 Phase 7 v2 detectors baseline.
- Local tag: `v1.46.0` points at the v1.46.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.46.0 release ledger sync`.
- Reference plan: `docs/superpowers/plans/2026-05-07-phase7-v3-promote-and-overlay.md`.
- Reference spec: `docs/superpowers/specs/2026-05-07-phase7-v3-promote-and-overlay-design.md`.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.46.0 work has landed yet.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` / `25478893257` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`, `1.41.0`, `1.42.0`, `1.43.0`, `1.44.0`, `1.45.0` with `dist-tags.latest = 1.45.0`.
- v1.46.0 CI publication is pending (external follow-up after local tag push).
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 v3 (c) — SQLite persistence for anomaly history (deferred from v1.46.0 — audit-grade / replay / retention design).
2. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
3. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.46.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1162 lib tests + 58 integration tests, all green (lib up from 1123 at v1.45.0; integration up from 57 at v1.45.0).
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.45.0 still apply when the v1.46.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete item is Phase 7 v3 (c) — SQLite persistence for anomaly history (audit-grade / replay / retention design). Phase 7 v1 (anomaly observation), v2 promotion, v2 detectors, and v3 (a+b) are all shipped. Tag protection and per-subagent token attribution both need either operator-side action (GitHub Settings) or provider-side support.
