# CURRENT_STATE

_Last updated: 2026-05-04 (Claude, v1.39.0 metrics overlay v2 + Action Explainer hardening + Codex 0.128 fix release ledger sync)_

## Mission

- Title: Qmonster v1.39.0 - metrics overlay v2 polish (per-pane cards, [/]/= resize, severity-colored bars, real per-poll MEM trend, day-format reset etas, Gemini quota + ModelReset surface, Codex split-resets-at regression cover) + Action Explainer hardening (mouse guard + PendingAction snapshot, confirm dispatches snapshot directly via handle_prompt_send_action_for_proposal, proposal_id threaded through prompt-send audit events) + Codex App Server v0.128 JSON-RPC compatibility (read_response skips unsolicited remoteControl/status/changed notifications between request and response).
- Version surfaces: mission ledger target `1.39.0`; npm package metadata `qmonster@1.39.0`; latest local Git tag `v1.39.0`.
- Branch / worktree at handoff start: `main`, tag `v1.39.0`.
- Release publication state: code/tag/package metadata are staged for the v1.39.0 release; external GitHub/npm publication is CI-owned and triggers off the v1.39.0 tag push via the `Release and Package Mirror` workflow. v1.37.0 / v1.38.0 CI publication verification also remains an external follow-up.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), and the v1.39 polish + correctness round are complete.

## v1.39.0 Feature State

Five backwards-compatible themes are complete in the v1.39.0 source. None of them changes provider adapters, policy gates, or the `SignalSet` schema; the audit summaries gain `proposal_id` but remain backwards-compatible.

1. **Metrics overlay v2**: `src/ui/metrics.rs` reflows the modal into a per-pane card layout — hottest banner on top, then a divider-separated card per pane with two columns (left: bounded values as severity-colored progress bars with tick marks at 60/75/85 + reset window inverse bars; right: unbounded values as 8-block sparklines + Δ vs prior sample). `src/app/metrics_overlay.rs` adds [`/`]/= resize keys + ↑/↓ body scroll + [x] close glyph. A `MemObservation` tracker in `tui_loop` drives real per-poll MEM RSS / MEM-FILE trend arrows (▲/▼/─), replacing the v1.38.0 placeholder. `format_resets_eta` uses `4d 6h` day format for ≥24h etas (panes card metric_row + RESET 5H/7D badges + metrics overlay reset rows). The m overlay surfaces Gemini quota + `ModelReset` advisories. Regression coverage locks the Codex split-resets-at rendering.
2. **Action Explainer hardening**: `src/app/action_explainer.rs` adds a mouse guard so the [x] click closes and other clicks don't pass through to the dashboard. `PendingAction` snapshots target identity (pane_id + slash_command + proposal_id, or alert command) at modal open time. The confirm path validates the snapshot against live reports and surfaces a "proposal vanished" notice instead of executing a drifted action; confirm dispatches the snapshot directly via `handle_prompt_send_action_for_proposal` so a newly polled lex-lower proposal can no longer divert the executed/dismissed command. `proposal_id` flows into `PromptSendAccepted` / `PromptSendCompleted` / `PromptSendFailed` / `PromptSendBlocked` / `PromptSendRejected` audit summaries end-to-end.
3. **Codex App Server v0.128 JSON-RPC compatibility**: `src/runtime/codex_app_server.rs::read_response` now skips JSON-RPC 2.0 notifications (id absent + method present, e.g. `remoteControl/status/changed` emitted by codex CLI ≥ 0.128 between request and response) until a real response arrives. Closes the silent rateLimits/read failure that hit on every poll tick when running against codex 0.128.
4. **Discoverability sync**: Settings Parameters tab adds `[ux] confirm_actions`, cost budget, profile_switch, cross-pane file knobs; Settings Rules tab adds cost-budget / profile-switch / cross-pane / explainer rule rows; Settings Badges legend lists MEM trend arrows + RESET 5H/7D day format. Help overlay adds m / Action Explainer / [x] / toggle-on-entry-key entries. Provider Setup gains F-7c reset advisory context + F-5b docstring fix. UI_MANUAL §8.5 rewritten for the v2 metrics overlay layout. `m metrics` chip added to dashboard footer. `confirm_actions` wording spells out always/first_time/never; advisory phrasing drops pseudo-IDs.
5. **Ledger sync**: `VERSION.md`, README release table, `docs/ai/PROJECT_BRIEF.md` Date+Phase, `docs/ai/ARCHITECTURE.md` Date line, `docs/ai/VALIDATION.md` v1.36.7 reference, `mission.yaml`, `mission-history.yaml`, and this state file all updated to v1.39.0.

## Latest Release Notes

- `v1.39.0` is a minor polish + correctness release over the v1.38.0 UX-bundle baseline.
- Local tag: `v1.39.0` points at the v1.39.0 release commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.39.0 metrics overlay v2 + action explainer hardening + codex 0.128 fix`.
- Reference spec: `docs/superpowers/specs/2026-05-04-v1.38-ux-bundle-design.md` (the v1.38 UX-bundle spec; v1.39 layers correctness on top of the same surfaces).

## Known External State

- CI publication for `v1.37.0` may still be running. Do not claim GitHub Release or npm publication success until the workflow and registry are checked.
- CI publication for `v1.38.0` follows the same workflow and remains pending verification until checked.
- CI publication for `v1.39.0` triggers off the v1.39.0 tag push via the `Release and Package Mirror` workflow; verify the workflow run, GitHub Release, and npm registry before claiming success.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Verify the CI-published `v1.39.0` GitHub Release and npm package after the release workflow completes.
2. Verify the still-pending CI publications for `v1.37.0` and `v1.38.0` (workflow runs, GitHub Release assets, npm registry) and update this state file with concrete run IDs and publication URLs.
3. Tag protection/ruleset activation remains a GitHub Settings task.
4. MF-2 remains deferred: `evolution_summary.current_phase` length cleanup.
5. Phase H remains unscoped: opt-in auto-snapshot at reset boundary.
6. Phase 7 planning is opened by `.docs/codex/phase7-anomaly-detection-spec.md` for anomaly detection and subagent analysis.
7. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.

## Validation Baseline

Most recent v1.39.0 validation reported in the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` (full lib + event-loop integration + false-positive regression + idle-state regression test suite green at HEAD `0c59a90`)
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 / v1.38.0 still apply when the v1.39.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Verify CI publication of `v1.39.0`: confirm the `Release and Package Mirror` workflow run completed successfully, the GitHub Release `v1.39.0` is visible with all expected assets (binary tarball, checksums, SBOM, attestations), and `qmonster@1.39.0` is published to npm. Then close out the still-pending v1.37.0 / v1.38.0 publication verification and update this state file with concrete workflow run IDs and publication URLs.
