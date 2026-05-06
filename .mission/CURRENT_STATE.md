# CURRENT_STATE

_Last updated: 2026-05-06 (Claude, v1.41.0 release ledger sync)_

## Mission

- Title: Qmonster v1.41.0 - v1.40 a-overlay polish: cursor staleness fix + ,/. first-press direction fix + confirm_actions bypass surfacing (UI_MANUAL ⚠ + hint chip + first-open SystemNotice) + doc/git consistency audit.
- Version surfaces: mission ledger target `1.41.0`; npm package metadata `qmonster@1.41.0`; latest local Git tag `v1.41.0`.
- Branch / worktree at handoff start: `main`, tag `v1.41.0`.
- Release publication state: v1.41.0 is locally tagged but not yet published. The `Release and Package Mirror` workflow run will trigger on the v1.41.0 tag push. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`) are all live — `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40}.0`.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, and the v1.41 a-overlay polish round are complete.

## v1.41.0 Feature State

Four backwards-compatible themes are complete in the v1.41.0 source. None of them changes provider adapters, audit chain, policy gates, or the `SignalSet` schema; the new `SystemNotice` flows through the existing `dashboard.push_notice` path, and the `seen_first_open` flag is overlay-local state reset on restart.

1. **Cursor staleness fix**: `src/app/pending_actions_overlay.rs::prune_to()` now clamps `selected` against `items.len()` so polling-driven shrinks no longer leave the cursor stale. The proposal preview, dispatch, and explainer all read the clamped index, so the operator never sees a stale highlight on a removed row.
2. **`,` / `.` first-press direction fix**: `src/app/pending_actions_overlay.rs::handle_pending_actions_overlay_key` signature gains a `viewport: Rect` param so `narrow_list` / `widen_list` can take the current effective list width as an argument and step in the correct direction from the auto-formula baseline. Previously the first press could move the wrong direction when the auto-formula list width exceeded the no-override default.
3. **`confirm_actions` bypass surfacing (3 surfaces)**: The a overlay's intentional `[ux] confirm_actions` bypass becomes discoverable through three operator-facing surfaces. (a) `docs/ai/UI_MANUAL.md` §8.7's "confirm_actions 무시" subsection is rewritten as a ⚠ caution that contrasts the dashboard direct-key path (which still respects the setting) with the a overlay path (which doesn't). (b) `src/ui/pending_actions.rs` in-modal hint row gains a `(confirm_actions bypass)` chip surfacing the bypass at the moment of action. (c) `PendingActionsOverlay` gains a `seen_first_open: bool` flag that, on the first `a` press per session, fires a Concern-severity `SystemNotice` summarising the bypass.
4. **Doc/git consistency audit + ledger sync**: At the v1.40.0 baseline — stale "Enter jumps + opens Action Explainer" module-doc cleanup, spec status (`docs/superpowers/specs/2026-05-06-mouse-drag-and-bulk-overlays-design.md`) flipped to "implementation shipped", plan checkboxes (`docs/superpowers/plans/2026-05-06-mouse-drag-and-bulk-overlays.md`) flipped to complete, CURRENT_STATE post-tag polish list refreshed. Plus `package.json`, `VERSION.md`, README release table, `docs/ai/PROJECT_BRIEF.md` Date+Phase, `docs/ai/ARCHITECTURE.md` Date line, `docs/ai/VALIDATION.md` v1.41.0 reference, `mission.yaml`, `mission-history.yaml`, and this state file all updated to v1.41.0.

## Latest Release Notes

- `v1.41.0` is a minor release over the v1.40.0 operator-controlled overlay geometry baseline.
- Local tag: `v1.41.0` points at the v1.41.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.41.0 release ledger sync`.
- Reference spec: `docs/superpowers/specs/2026-05-06-mouse-drag-and-bulk-overlays-design.md` (the v1.40 spec; v1.41 is a polish release without its own spec).
- Reference plan: `docs/superpowers/plans/2026-05-06-mouse-drag-and-bulk-overlays.md` (the v1.40 plan; v1.41 is a polish release without its own plan).

## Post-tag polish (on main, untagged)

No genuinely-post-v1.41.0 work has landed yet. The v1.41.0 release commit is the immediate parent of the v1.41.0 tag.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`, `1.40.0` with `dist-tags.latest = 1.40.0`.
- v1.41.0 is locally tagged but not yet published; the `Release and Package Mirror` workflow run will trigger on the v1.41.0 tag push.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Tag protection / ruleset activation remains a GitHub Settings task.
2. Phase H remains unscoped: opt-in auto-snapshot at reset boundary.
3. Phase 7 planning is opened by `.docs/codex/phase7-anomaly-detection-spec.md` for anomaly detection and subagent analysis.
4. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.

## Validation Baseline

Most recent v1.41.0 validation reported in the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1028 lib tests + 50 event-loop integration tests + 18 false-positive regression tests + 6 idle-state regression tests, all green at the v1.41.0 release commit (+6 lib tests vs the v1.40.0 baseline of 1022).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 still apply when the v1.41.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete item is Phase H scoping (opt-in auto-snapshot at reset boundary). Phase 7 anomaly detection and tag protection both need either operator input (Phase 7 scope) or operator-side action (GitHub Settings). v1.41.0 CI publication verification will trigger on the v1.41.0 tag push and is then a separate verification follow-up.
