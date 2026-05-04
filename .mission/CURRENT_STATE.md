# CURRENT_STATE

_Last updated: 2026-05-04 (Claude, v1.39.0 metrics overlay v2 + Action Explainer hardening + Codex 0.128 fix release ledger sync)_

## Mission

- Title: Qmonster v1.39.0 - metrics overlay v2 polish (per-pane cards, [/]/= resize, severity-colored bars, real per-poll MEM trend, day-format reset etas, Gemini quota + ModelReset surface, Codex split-resets-at regression cover) + Action Explainer hardening (mouse guard + PendingAction snapshot, confirm dispatches snapshot directly via handle_prompt_send_action_for_proposal, proposal_id threaded through prompt-send audit events) + Codex App Server v0.128 JSON-RPC compatibility (read_response skips unsolicited remoteControl/status/changed notifications between request and response).
- Version surfaces: mission ledger target `1.39.0`; npm package metadata `qmonster@1.39.0`; latest local Git tag `v1.39.0`.
- Branch / worktree at handoff start: `main`, tag `v1.39.0`.
- Release publication state: v1.39.0 is published. `Release and Package Mirror` workflow run `25311723861` (2026-05-04, 7m5s, success) created GitHub Release `v1.39.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums, attestations) and published `qmonster@1.39.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`) and v1.38.0 (`25305201597`) publications also verified post-hoc — both releases live, both packages present in `npm view qmonster versions`.
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

## Post-tag polish (on main, untagged)

Five commits landed on main after `v1.39.0` was tagged. They will be picked up by the next tagged release.

- `41cf54f` — post-publish CURRENT_STATE refresh (concrete workflow run IDs for v1.37/v1.38/v1.39).
- `540c414` — MF-2 closure: `evolution_summary.current_phase` / `next_phase` tightened.
- `d752204` — `★p` chip on pane card title when prompt-send proposal pending; `★y` chip on alert title when `suggested_command` present. Severity-colored.
- `e8e1808` — footer pending counters (`★p:N · ★y:M`) always-visible. Dim on 0; severity-colored when N > 0.
- `922bba9` — Pending Actions overlay (`a` key): unified modal listing every pane with a pending p/d proposal AND every alert with a y-copyable command. Enter jumps to the underlying item AND opens the Action Explainer modal so dispatch is one keypress away from anywhere on the dashboard. Help overlay + UI_MANUAL §8.7 document the new key.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39}.0`; `npm view qmonster versions` lists `1.37.0`, `1.38.0`, `1.39.0`.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Tag protection / ruleset activation remains a GitHub Settings task.
2. Phase H remains unscoped: opt-in auto-snapshot at reset boundary.
3. Phase 7 planning is opened by `.docs/codex/phase7-anomaly-detection-spec.md` for anomaly detection and subagent analysis.
4. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.

## Validation Baseline

Most recent v1.39.0 validation reported in the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` (full lib + event-loop integration + false-positive regression + idle-state regression test suite green at HEAD `0c59a90`)
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 / v1.38.0 still apply when the v1.39.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. The closest concrete items are MF-2 (mission-history.yaml `evolution_summary.current_phase` length cleanup) and Phase H scoping (opt-in auto-snapshot at reset boundary). Phase 7 anomaly detection and tag protection both need either operator input (Phase 7 scope) or operator-side action (GitHub Settings).
