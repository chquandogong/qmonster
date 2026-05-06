# CURRENT_STATE

_Last updated: 2026-05-06 (Claude, v1.40.0 release ledger sync)_

## Mission

- Title: Qmonster v1.40.0 - operator-controlled overlay geometry: m drag-to-move + a overlay live-explainer-split + multi-select bulk dispatch + size/position/list-ratio adjustability across both overlays.
- Version surfaces: mission ledger target `1.40.0`; npm package metadata `qmonster@1.40.0`; latest local Git tag `v1.40.0`.
- Branch / worktree at handoff start: `main`, tag `v1.40.0`.
- Release publication state: v1.40.0 has been pushed to origin/main + tag pushed. The `Release and Package Mirror` workflow run `25421376056` was triggered on the v1.40.0 tag push and is in progress (queued / running at the time of this commit; v1.39.0's run took 7m5s for reference). GitHub Release page + npm publish will be verified post-success in a follow-up CURRENT_STATE polish commit. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), and v1.39.0 (`25311723861`) are all live — GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39}.0` and `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, and the v1.40 operator-controlled overlay geometry round are complete.

## v1.40.0 Feature State

Seven backwards-compatible themes are complete in the v1.40.0 source. None of them changes provider adapters, audit chain, policy gates, or the `SignalSet` schema; bulk dispatch routes per-item through the existing `confirm_pending_action(...)` path so the audit event chain (`PromptSendAccepted`/`Rejected`/`Completed`/`Blocked` + `AlertHidden` via `alert_hide_deadlines.insert`) is unchanged.

1. **m overlay drag-to-move**: `src/ui/metrics.rs` `MetricsOverlay` gains `offset_x` / `offset_y` / `drag_anchor`; `metrics_modal_rects` becomes a 5-arg helper that calls `apply_clamped_offset` enforcing left/top hard bounds + right/bottom soft bounds (≥4 cells horizontal / ≥1 row vertical visible). `src/app/metrics_overlay.rs::handle_metrics_overlay_mouse` routes Down on the title row (excluding `[x]`) into drag-anchor capture, Drag into per-frame offset application, Up into release. `=` resets size + offset together. UI_MANUAL §8.5 + help overlay updated.
2. **a overlay redesign — split layout + live Action Explainer**: `src/ui/pending_actions.rs` body splits into `{ area, list, explainer, hint }` via `pending_actions_modal_rects` (vertical when body width < 80, horizontal otherwise); the list pane is 60% of body clamped to `[44, 64]`. `render_modal` renders the cursor item's explainer view inline using `build_accept_view` (proposals) / `build_copy_view` (alerts) so the explainer pane is a live preview, not a separate modal; Enter is silently swallowed inside the overlay.
3. **a overlay redesign — multi-select**: `src/app/pending_actions_overlay.rs::PendingActionsOverlay` gains `multi_selected: BTreeSet<String>` tracking stable keys (proposal_id for proposals, alert_id for alerts). Auto-prune at every render drops selections without a live row. New keys: Space (toggle cursor row), P/Y/A (group toggle: P=all proposals, Y=all alerts, A=everything), c (clear all selections). Mouse: cols 0–3 toggle multi-select; cols 4+ move the cursor.
4. **a overlay redesign — bulk dispatch**: `PendingActionsOutcome` enum carries `AcceptItems` / `ClearItems` / `CopyItem`; p/d/y dispatch keys construct the outcome from `multi_selected` (priority) or the cursor row (fallback). `src/app/tui_loop.rs` routes each item per-call through `confirm_pending_action(...)` so no new audit event types are introduced. Notices push through `dashboard.push_notice(notice, now)` for top-of-queue + `fresh_alerts` / `alert_times` / `resync` parity with single-item dispatch. Bulk-clear iterates highest-index-first to avoid index-shift bugs.
5. **a overlay TX-A — modal size + position adjustability**: `PendingActionsOverlay` gains `width_pct` / `height_pct` (defaults 80 / 65, range `[50, 99]`) and `offset_x` / `offset_y` / `drag_anchor`. `[`/`]` shrink/grow by 5%; `=` resets size + offset + ratio together; title-row drag (excluding `[x]`) moves the modal with the same asymmetric clamp as the m overlay; close+reopen preserves geometry, restart resets.
6. **a overlay TX-B — list/explainer ratio adjustability**: `PendingActionsOverlay` gains `list_width_override`. `,` narrows / `.` widens the list pane in 2-cell steps; the list/explainer separator (3-cell hit zone, body rows only) drag resizes the split; override range stays inside the auto-formula's `[44, 64]` clamp so the explainer always has ≥8 cells; close+reopen preserves the override.
7. **Ledger sync**: `package.json`, `VERSION.md`, README release table, `docs/ai/PROJECT_BRIEF.md` Date+Phase, `docs/ai/ARCHITECTURE.md` Date line, `docs/ai/VALIDATION.md` v1.40.0 reference, `mission.yaml`, `mission-history.yaml`, and this state file all updated to v1.40.0.

## Latest Release Notes

- `v1.40.0` is a minor release over the v1.39.0 polish + correctness baseline.
- Local tag: `v1.40.0` points at the v1.40.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.40.0 release ledger sync`.
- Reference spec: `docs/superpowers/specs/2026-05-06-mouse-drag-and-bulk-overlays-design.md`.
- Reference plan: `docs/superpowers/plans/2026-05-06-mouse-drag-and-bulk-overlays.md`.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.40.0 work has landed yet. The v1.40.0 release commit is the immediate parent of the `v1.40.0` tag.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`.
- v1.40.0 is locally tagged but not yet published; the `Release and Package Mirror` workflow run will trigger on the v1.40.0 tag push.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Tag protection / ruleset activation remains a GitHub Settings task.
2. Phase H remains unscoped: opt-in auto-snapshot at reset boundary.
3. Phase 7 planning is opened by `.docs/codex/phase7-anomaly-detection-spec.md` for anomaly detection and subagent analysis.
4. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.

## Validation Baseline

Most recent v1.40.0 validation reported in the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1022 lib tests + 50 event-loop integration tests + 18 false-positive regression tests + 6 idle-state regression tests, all green.
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 / v1.38.0 / v1.39.0 still apply when the v1.40.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete items are Phase H scoping (opt-in auto-snapshot at reset boundary) and the v1.40.0 CI publication verification once the `v1.40.0` tag is pushed and the `Release and Package Mirror` workflow run completes. Phase 7 anomaly detection and tag protection both need either operator input (Phase 7 scope) or operator-side action (GitHub Settings).
