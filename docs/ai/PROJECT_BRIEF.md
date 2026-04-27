# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-28 (current — v1.16.56 code-exploration advisory false-positive cleanup)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 + Phase B + Phase C C1/C2/C3 are complete.** Phase C C2 made `[tmux] source = "auto"` the default: auto attaches control-mode first, falls back to polling with a startup notice if attach fails, and keeps explicit `control_mode` strict. v1.16.51 updates observability semantics: Claude CTX is populated from `/context`; Claude quota is split from `/usage` Current session (5h) and Current week (all models); Codex quota is split from bottom-status `5h` and `weekly`; and the `S` settings overlay persists separate 5h/weekly quota thresholds for Claude and Codex. v1.16.52 makes the live TUI match that contract by parsing Claude's current `Context Usage` output, deepening Claude fullscreen runtime captures, waiting for render/pre-`Escape` settle, and caching Claude CTX + quota metrics per pane so they display together. v1.16.53 inverts Codex remaining-quota percentages into Qmonster pressure. v1.16.54 keeps runtime refresh captures parseable without hiding the live prompt cursor, restores reliable `c` system-notice clearing, and aligns npm package semver with the ledger tag. v1.16.55 completes C3 by adding `codex-review` and `gemini-policy-review` profile recommendations for healthy `Role::Review` panes. v1.16.56 narrows the `code-exploration` advisory: the bare `output_chars >= 1500` fallback was firing on every healthy Main pane in the 2026-04-28 live audit (same v1.13.0 anti-pattern that was already removed for log_storm and verbose_answer). The rule now requires `TaskType::CodeExploration` or a narrowed `verbose_answer` hedge phrase, locked by a new regression test.

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
