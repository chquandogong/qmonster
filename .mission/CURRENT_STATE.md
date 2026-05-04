# CURRENT_STATE

_Last updated: 2026-05-04 (Claude, v1.38.0 UX bundle release ledger sync)_

## Mission

- Title: Qmonster v1.38.0 - UX bundle: Metrics Overlay (m) with bounded vs unbounded visual split, pane card text wrap fix via wrap_aligned_field, Action Explainer modal for p/d/y with [ux] confirm_actions config, and toggle-on-entry-key + [x] standardization across S/P/t/m + target picker.
- Version surfaces: mission ledger target `1.38.0`; npm package metadata `qmonster@1.38.0`; latest local Git tag `v1.38.0`.
- Branch / worktree at handoff start: `main`, tag `v1.38.0`.
- Release publication state: code/tag/package metadata are staged for the v1.38.0 release; external GitHub/npm publication is CI-owned and will follow the same workflow as v1.37.0. v1.37.0 CI publication verification also remains pending.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, and the v1.38 UX bundle (F1/F2/F3/F4) are complete.

## v1.38.0 Feature State

Four backwards-compatible UX features are complete in the v1.38.0 source. None of them changes provider adapters, audit chain, policy gates, or `SignalSet` schema — they are pure UI/UX layered on existing data:

1. **F1 Metrics Overlay (`m`)**: `src/ui/metrics.rs` renders a modal that visually splits **bounded** values (context/quota/cache pressure as horizontal progress bars with tick marks at 60/75/85; reset windows as inverse-progress bars + textual ETA) from **unbounded** values (cumulative tokens, cost, RSS as 8-block sparklines + Δ vs prior sample). The modal layout is hottest banner, per-pane comparison row, and selected-pane detail (sparklines + reset bars + MEM trend placeholder). `src/app/metrics_overlay.rs` provides key/mouse dispatchers wired into `tui_loop`. Bound scroll, `[x]` close button click test included.
2. **F2 Pane card text wrap fix (`wrap_aligned_field`)**: `src/ui/panels.rs` introduces `wrap_aligned_field` and threads `wrap_width` through path/cmd/status pane card lines, the state row, severity reason rows, and expanded detail rows; pane card values now wrap with continuation indent on narrow splits instead of dropping silently off the right edge. `wrap_badge_line` reuse extends to the state row. `pane_index_at_row` regression test locks hit-testing under wrap.
3. **F3 Action Explainer modal + inline previews + `[ux] confirm_actions`**: `src/app/action_explainer.rs` introduces `ActionExplainModal` state + view builders + `should_open_explainer` helper. `tui_loop` intercepts `p`/`d`/`y` through the explainer, which renders fields, audit chain, and a mode warning before dispatch. An inline `proposal:` line on the selected pane card and a `copy:` line on the selected alert (aligned with sibling rows) preview the pending action. The new `[ux] confirm_actions` config knob in `src/app/config.rs` accepts `always` (default — modal on every p/d/y), `first_time` (modal only on the first per-session invocation, tracked via `should_open_explainer`), or `never` (preserves v1.37.0 silent-execute).
4. **Toggle-on-entry-key + `[x]` standardization**: `tui_loop` dispatch toggles `S` (Settings), `P` (Provider Setup), `t` (target picker), `m` (Metrics) on the entry key when the matching overlay is already open. `close_button_rect` renders an `[x]` glyph top-right on every overlay. Existing close keys (`Esc`, `q`) keep working. Redundant target picker mouse gate dropped.

## Post-tag polish (on main, untagged)

Two operator-facing iterations landed on main after v1.38.0 was tagged.
They will be picked up by the next tagged release (v1.38.1 or v1.39.0).

- `ff11717` — `format_resets_eta` uses `4d 6h` style for ≥24h etas
  instead of `102h00m`. Affects panes card metric_row, RESET 5H/7D
  badges, and metrics overlay reset rows.
- `6355f8c` — real MEM RSS / MEM-FILE trend arrows (▲/▼/─) wired
  through a per-poll `MemObservation` tracker in `tui_loop`. Replaces
  the placeholder `─` introduced in v1.38.0.

`git describe --tags --always` reports `v1.38.0-2-g6355f8c` as of this
update.

## Latest Release Notes

- `v1.38.0` is a minor UX-bundle release over the v1.37.0 feature line.
- Local tag: `v1.38.0` points at the v1.38.0 release commit (recorded in mission-history.yaml `related_commits`).
- Commit summary: `v1.38.0 UX bundle — metrics overlay + wrap fix + action explainer + toggle keys`.
- Reference spec: `docs/superpowers/specs/2026-05-04-v1.38-ux-bundle-design.md`.
- Reference plan: `docs/superpowers/plans/2026-05-04-v1.38-ux-bundle.md` (Tasks 1-23).

## Known External State

- CI publication for `v1.37.0` may still be running. Do not claim GitHub Release or npm publication success until the workflow and registry are checked.
- CI publication for `v1.38.0` will follow the same workflow once tag is pushed; do not claim publication success until verified.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Verify the CI-published `v1.37.0` GitHub Release and npm package after the release workflow completes.
2. v1.38.0 tag is created locally and not yet pushed; operator approval required before push.
3. Tag protection/ruleset activation remains a GitHub Settings task.
4. MF-2 remains deferred: `evolution_summary.current_phase` length cleanup.
5. Phase H remains unscoped: opt-in auto-snapshot at reset boundary.
6. Phase 7 planning is opened by `.docs/codex/phase7-anomaly-detection-spec.md` for anomaly detection and subagent analysis.
7. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.

## Validation Baseline

Most recent v1.38.0 validation reported in the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` (874 lib + 50 + 18 + 6 integration tests, all green)
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 still apply when the v1.38.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Operator-approve push of `v1.38.0` tag and verify the CI-published GitHub Release + npm package. Then verify the still-pending v1.37.0 publication assets, npm package, checksums, SBOM, and provenance, and update this state file with the concrete workflow run IDs and publication URLs.
