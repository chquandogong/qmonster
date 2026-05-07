# CURRENT_STATE

_Last updated: 2026-05-07 (Claude, v1.47.0 publication verified)_

## Mission

- Title: Qmonster v1.47.0 - Phase 7 v3 (c): SQLite persistence for anomaly events + history
- Version surfaces: mission ledger target `1.47.0`; npm package metadata `qmonster@1.47.0`; latest local Git tag `v1.47.0`.
- Branch / worktree at handoff start: `main`, tag `v1.47.0`.
- Release publication state: v1.47.0 is published. `Release and Package Mirror` workflow run `25490555532` (2026-05-07, 7m16s, success) created GitHub Release `v1.47.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.47.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`), v1.42.0 (`25472444159`), v1.43.0 (`25474748447`), v1.44.0 (`25476534645`), v1.45.0 (`25478893257`), v1.46.0 (`25485133895`) publications also remain live — `npm view qmonster versions` lists `1.37.0` through `1.47.0` with `dist-tags.latest = 1.47.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47}.0`.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, Phase 7 v1 anomaly observation surface, Phase 7 v2 promotion, Phase 7 v2 detectors, Phase 7 v3 (a+b), and Phase 7 v3 (c) are complete.

## v1.47.0 Feature State

Phase 7 v3 (c) adds SQLite persistence for anomaly events and history snapshots on top of the v1.46.0 Phase 7 v3 (a+b) baseline. Two new tables (anomaly_events, anomaly_history_snapshots) added to audit.db via SqliteAnomalySink (mirrors SqliteTokenUsageSink/SqliteCostUsageSink pattern).

1. **AnomalyKind/Confidence/Severity label/try_from_label helpers** (`src/domain/anomaly.rs`, `src/domain/recommendation.rs`): New `label()` and `try_from_label()` methods on `AnomalyKind`, `Confidence`, and `Severity` for round-trip serialization to/from SQLite text columns.

2. **AnomalyHistorySnapshot struct + capture_tick + push_snapshot_into_history** (`src/store/anomaly_history.rs`): New plain-data struct capturing a full AnomalyHistory deque state at a point in time. `capture_tick` snapshots the current deque; `push_snapshot_into_history` replays a stored snapshot back into an AnomalyHistory on startup.

3. **SqliteAnomalySink** (`src/store/anomaly_sink.rs`): open/insert/upsert/with_transaction/fetches/run_retention; two new schemas in AuditDb::open. `anomaly_events` table stores per-tick anomaly events; `anomaly_history_snapshots` table stores per-pane deque snapshots. Retention: time-based (`retention_days`, default 30) + 100K-row emergency cap on events; snapshots auto-purge at 4× window.

4. **AnomalyConfig.retention_days = 30 default** (`src/app/config.rs`): New `retention_days` field on `AnomalyConfig` with default 30. Controls time-based retention for both `anomaly_events` and `anomaly_history_snapshots` tables.

5. **Context.anomaly_sink + anomaly_history_replayed + with_anomaly_sink builder** (`src/app/bootstrap.rs`): `Context` gains `anomaly_sink: Option<SqliteAnomalySink>` and `anomaly_history_replayed: HashSet<PaneId>` fields. `with_anomaly_sink` builder wires the sink into the context.

6. **event_loop per-tick transaction** (`src/app/event_loop.rs`): `run_once_with_target` wraps anomaly_events INSERT + snapshot UPSERT in a single fsync transaction per tick. Batches all visible signal inserts + one UPSERT per pane with anomaly activity.

7. **event_loop lazy AnomalyHistory replay on first-pane-observation** (`src/app/event_loop.rs`): On the first observation of a pane per session, replays stored AnomalyHistory from disk if the stored snapshot is not stale (staleness gate: ≤ 2× window × tick).

8. **AnomalyOverlayView enum + view/history_cache fields + toggle_view + close_resets** (`src/ui/anomaly_overlay.rs`): `AnomalyOverlay` gains a `view: AnomalyOverlayView` enum (Ring | History) and `history_cache: Vec<AnomalyEvent>`. `toggle_view` switches between Ring and History views; `close` resets back to Ring view and clears the cache.

9. **AnomalyOverlay render branch for History view** (`src/ui/anomaly_overlay.rs`): History view renders the stored AnomalyHistory snapshots from disk in the overlay modal. Shows per-pane deque entries newest-first with timestamps.

10. **tui_loop wires SqliteAnomalySink + 'h' key history-view dispatch** (`src/app/startup.rs`, `src/app/tui_loop.rs`): `startup.rs` constructs `SqliteAnomalySink` and wires it into the context via `with_anomaly_sink`. `tui_loop.rs` dispatches the `h` key to `toggle_view` on the `AnomalyOverlay` when the overlay is open.

11. **Settings UI Parameters tab — anomaly retention_days row** (`src/ui/settings.rs`): Parameters tab `[anomaly]` section gains a `retention_days` row showing the current configured value.

12. **Phase 7 v3 closes**: (a+b) shipped in v1.46.0; (c) ships in v1.47.0. The only outstanding Phase 7 follow-up is D3 per-subagent token attribution (provider-blocked).

## Latest Release Notes

- `v1.47.0` is a minor feature release over the v1.46.0 Phase 7 v3 (a+b) baseline.
- Local tag: `v1.47.0` points at the v1.47.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.47.0 release ledger sync`.
- Reference plan: `docs/superpowers/plans/2026-05-07-phase7-v3-c-sqlite-persistence.md`.
- Reference spec: `docs/superpowers/specs/2026-05-07-phase7-v3-c-sqlite-persistence-design.md`.

## Post-tag polish (on main, untagged)

- `d699694 v1.47.0 review fixup: CURRENT_STATE.md Feature Item 8 accuracy` — corrected AnomalyOverlayView variants ("Live | History" → "Ring | History") and history_cache type ("Vec<AnomalyHistorySnapshot>" → "Vec<AnomalyEvent>") in the Feature State description. Docs-only; matches the v1.42-v1.46 ledger-fixup post-tag pattern (no re-tag needed).

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 / v1.46.0 / v1.47.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` / `25478893257` / `25485133895` / `25490555532` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47}.0`; `npm view qmonster versions` lists `1.37.0` through `1.47.0` with `dist-tags.latest = 1.47.0`.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters (Phase 7's only remaining follow-up).
2. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.47.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — ~1200 lib tests + 60 integration tests, all green (lib up from 1162 at v1.46.0; integration up from 58).
- `cargo clippy --lib --tests -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.46.0 still apply when the v1.47.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Phase 7 D3 is provider-blocked; pick from upstream backlog.
