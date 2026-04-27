# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-27 (current — v1.15.21 Phase-B pane visibility)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 + v1.15.x runtime observability/settings/cost line shipped; v1.15.20 activates the shipped surfaces in the default operator workflow; v1.15.21 starts Phase B visibility.** v1.15.18 added the in-TUI settings overlay for cost/context/quota thresholds and v1.15.19 added its `[x]` close button. v1.15.20 standardizes `~/.qmonster/config/qmonster.toml` as the default writable config path, loads it automatically when present, and lets the settings overlay save there even when Qmonster started without `--config`. `scripts/run-qmonster.sh` creates both config and pricing templates under `~/.qmonster/config/` and launches with `--config`; `~/.qmonster/config/pricing.toml` remains operator-filled so COST badges/advisories are enabled without fetching provider prices. Non-canonical provider/status panes can now resolve to `Role::Main` at Medium confidence when the provider is structurally identified, waking baseline profile recommendations previously blocked by `role=Unknown` / Low confidence. v1.15.21 threads tmux `pane_current_command` into pane cards/detail/`--once` and shows selected-pane Codex input/output token breakdown when ProviderOfficial token data exists. Verification: 508 tests green; clippy + fmt clean; `scripts/verify-shared.sh` passes with a lite ledger fallback because `mission-spec` is not installed locally.

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
