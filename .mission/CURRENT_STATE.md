# CURRENT_STATE

_Last updated: 2026-05-06 (Claude, v1.42.0 release ledger sync)_

## Mission

- Title: Qmonster v1.42.0 - Phase H: opt-in auto-snapshot at reset boundary (`[reset] auto_snapshot`).
- Version surfaces: mission ledger target `1.42.0`; npm package metadata `qmonster@1.42.0`; latest local Git tag `v1.42.0`.
- Branch / worktree at handoff start: `main`, tag `v1.42.0`.
- Release publication state: v1.42.0 is locally tagged but not yet published. The `Release and Package Mirror` workflow run will trigger on the v1.42.0 tag push. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`) are all live — `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`, `1.41.0` with `dist-tags.latest = 1.41.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41}.0`.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, and Phase H opt-in auto-snapshot are complete.

## v1.42.0 Feature State

One backwards-compatible feature is complete in the v1.42.0 source. It does not change provider adapters, audit chain (snapshots route through the existing `ArchiveWritten` path), policy gates set, or the `SignalSet` schema. The `auto_snapshot_dedup` HashMap is session-local state reset on restart.

1. **Opt-in auto-snapshot at reset boundary (`[reset] auto_snapshot`)**: When `auto_snapshot = true` (default `false`), Qmonster automatically archives a snapshot at each reset boundary before the reset fires. Files touched: `src/domain/config.rs` gains `ResetConfig.auto_snapshot: bool`; `src/policy/gates.rs` gains `PolicyGates.reset_auto_snapshot` wired from `ResetConfig`; new module `src/app/auto_snapshot.rs` adds `AutoSnapshotDedup` (HashMap keyed by `(target, QuotaKind)` with `floor_to_minute` dedup window), `QuotaKind` enum (`FiveHour` / `Weekly`), `extract_quota_kind` (maps `ResetKind` → `QuotaKind` with drift guard), and `maybe_auto_snapshot` orchestrator (gate → dedup → archive); `src/app/context.rs` gains `Context.auto_snapshot_dedup`; `src/app/event_loop.rs::run_once_with_target` wires `maybe_auto_snapshot` before the reset fires; `src/ui/settings.rs` gains Parameters and Rules rows. Behavior when off: zero behavior change from v1.41.0. Behavior when on: a pre-reset snapshot is archived and deduplicated within the same `floor_to_minute` window per `(target, QuotaKind)` pair.

## Latest Release Notes

- `v1.42.0` is a minor release over the v1.41.0 a-overlay polish baseline.
- Local tag: `v1.42.0` points at the v1.42.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.42.0 release ledger sync`.
- Reference spec: `docs/superpowers/specs/2026-05-06-phase7-v1-anomaly-overlay-design.md` (Phase 7 v1 design, committed but unimplemented; Phase H had no spec — it was planned in the phase H plan commit `bbab2bf`).

## Post-tag polish (on main, untagged)

No genuinely-post-v1.42.0 work has landed yet. The v1.42.0 release commit is the immediate parent of the v1.42.0 tag.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`, `1.41.0` with `dist-tags.latest = 1.41.0`.
- v1.42.0 is locally tagged but not yet published; the `Release and Package Mirror` workflow run will trigger on the v1.42.0 tag push.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 v1 anomaly overlay: spec committed at `docs/superpowers/specs/2026-05-06-phase7-v1-anomaly-overlay-design.md`; implementation plan needs to be written next cycle.
2. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
3. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.42.0 validation reported in the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — ~1049 lib tests + ~52 event-loop integration tests + 18 false-positive regression tests + 6 idle-state regression tests, all green at the v1.42.0 release commit (+21 lib tests vs the v1.41.0 baseline of 1028; +2 event-loop integration tests vs the v1.41.0 baseline of 50).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 still apply when the v1.42.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete item is writing the Phase 7 v1 implementation plan (the spec is already committed at `docs/superpowers/specs/2026-05-06-phase7-v1-anomaly-overlay-design.md`). Phase H is shipped. Tag protection and per-subagent token attribution both need either operator-side action (GitHub Settings) or provider-side support. v1.42.0 CI publication verification will trigger on the v1.42.0 tag push and is then a separate verification follow-up.
