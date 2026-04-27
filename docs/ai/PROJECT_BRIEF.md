# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-27 (current — v1.16.17 initial target helper extraction)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 + v1.15.x runtime observability/settings/cost line shipped; Phase B is complete; Phase C C1 is active.** v1.15.20 standardizes `~/.qmonster/config/qmonster.toml` and `~/.qmonster/config/pricing.toml`; v1.15.21 threads tmux `pane_current_command` into pane cards/detail/`--once` and shows selected-pane Codex input/output token breakdown. v1.15.22 adds `[security] posture_advisories = false`; when enabled, YOLO / bypass permissions / Full Access / danger-full-access / no sandbox runtime facts produce passive Concern recommendations. v1.15.23 removes path-only concurrent warnings; panes need matching `current_path` and `git_branch`. v1.15.24 adds `y copy` in the TUI: selected Alerts with a `run:` command copy their `suggested_command` to the system clipboard, with success/failure notices. v1.15.25 is a docs/ledger consistency checkpoint that closes Phase B's active backlog. v1.16.0 starts Phase C C1 by extracting dashboard focus, list selection, and list hit-test helpers into `src/app/keymap.rs`; v1.16.1 moves the target picker model/preview/choice helpers into `src/app/target_picker.rs`; v1.16.2 moves provider runtime-refresh command selection, cycling, send/capture, and notice label helpers into `src/app/runtime_refresh.rs`; v1.16.3 moves alert selection/hide/double-click and pane-state flash synchronization into `src/app/dashboard_state.rs`. v1.16.4 fixed the Git overlay title to use the same git-described version string as the footer badge. v1.16.5 restores `c` to its original system-notice clear role; selected-alert command copy remains on `y`. v1.16.6 moves shared git/help scroll modal open/close/scroll state plus key/mouse handling into `src/app/modal_state.rs`. v1.16.7 moves settings overlay key and mouse dispatch into `src/app/settings_overlay.rs`. v1.16.8 moves operator version-refresh and snapshot-write helpers into `src/app/operator_actions.rs`. v1.16.9 moves `--once` report formatting into `src/app/once_report.rs`. v1.16.10 moves prompt-send accept/dismiss handling into `src/app/prompt_send_actions.rs`. v1.16.11 moves runtime-refresh action orchestration into `src/app/runtime_refresh.rs`. v1.16.12 moves selected-alert command copy notices into `src/app/clipboard_actions.rs`. v1.16.13 moves target-picker open/key/mouse dispatch into `src/app/target_picker.rs`. v1.16.14 moves dashboard Alerts/Panes selection key dispatch into `src/app/dashboard_state.rs`. v1.16.15 moves dashboard mouse dispatch into `src/app/dashboard_state.rs`. v1.16.16 moves default config-path resolution into `src/app/path_resolution.rs`. v1.16.17 moves initial target selection into `src/app/target_picker.rs`. `src/main.rs` still owns the live event loop but has shed reusable interaction/modal/target-picker/runtime-refresh/dashboard-state/path-resolution/operator-action/once-output/prompt-send/clipboard support code. Verification: 565 tests green; clippy + fmt clean; `scripts/verify-shared.sh` passes with a lite ledger fallback because `mission-spec` is not installed locally.

## What Qmonster is

A Rust TUI that sits in one tmux pane and observes the other panes of
the same project window — typically Claude Code (main), Codex (review /
cross-check), Gemini (research / policy / safety), plus the Qmonster
monitor itself. It surfaces the signals a human operator cannot keep up
with on their own: alerts, token / context pressure, recommended actions,
security and audit events, and task observability.

## What Qmonster is NOT

- Not a provider orchestrator. It does not silently route work between
  Claude, Codex, or Gemini.
- Not a destructive automator. It does not run `/compact`, `/clear`,
  prompt injection, or provider reconfiguration on its own.
- Not a replacement for mission-spec, canonical docs, or auto memory.
  It reads state; it does not own the contract layer.
- Not a cloud service. Single-user, local-first in this phase.

## Operating principles

1. **Observe-first**. Read state before offering any action.
2. **Alert-first**. The loudest surface on the screen is the queue of
   things that need attention — stop / stopfail / input wait /
   permission wait / context pressure / security concern.
3. **Recommendation-first**. Automation is kept narrow and auditable.
   Qmonster recommends; the human (or another explicit approval gate)
   acts.
4. **Source-labeled evidence**. Every number and every lever carries
   a `SourceKind`: `ProviderOfficial`, `ProjectCanonical`,
   `Heuristic`, or `Estimated`.
5. **Token optimization is architectural**. It is a design axis, not a
   feature. It shapes the observe → classify → recommend → archive →
   checkpoint loop.

## Operating model

- **One project = one tmux window = four panes.**
- **Multiple projects = multiple windows.**
- **Repeated providers allowed** inside one project (e.g.,
  `claude:1:main`, `claude:2:review`, `claude:3:research`).
- **Pane identity** = `{provider, instance, role, pane_id}`.
- **Pane title convention** = `{provider}:{instance}:{role}`.
- Default roles: `main`, `review`, `research`, `monitor`.
- Default assignment: Claude main, Codex review, Gemini research,
  Qmonster monitor.

## Who runs this

- Single operator, local machine, balanced logging.
- Quota-tight mode is opt-in and unlocks more aggressive token-saving
  recommendations.
- Team / CI / review-ready phase is anticipated but not active: the
  gitignore flip path is documented in `WORKFLOWS.md`.

## Doc layer summary (authority order)

1. `mission.yaml` — current mission contract (goal, done_when,
   constraints, approvals).
2. `mission-history.yaml` — scope-change ledger.
3. `.mission/CURRENT_STATE.md` — today's state / handoff.
4. `.mission/decisions/` — MDRs.
5. `.mission/evals/` — human or LLM verdicts.
6. `docs/ai/*` — canonical shared docs (this file and its siblings).
7. `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` — thin routers only.
8. `.docs/*` — raw working docs, gitignored.
9. Auto memory (Claude / Codex / Gemini) — local convenience only.

Authority goes **top → bottom**: if a memory entry disagrees with
`mission.yaml`, `mission.yaml` wins.
