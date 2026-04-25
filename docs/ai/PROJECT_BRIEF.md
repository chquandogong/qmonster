# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-25 (current — v1.14.1 baseline + local doc/idle consistency follow-up)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 + P0-1 Slice 1+2 + v1.13.x emergency suppression + Slice 4 (halted/idle state) shipped; local runtime observability follow-up applied.** P0-1 Slices 1+2 (v1.11.0..v1.12.2) closed the honest-gap audit that v1.10.9's TUI showed blanks for "who used how much + which model" via new `PricingTable` + `ClaudeSettings` operator-config readers, `&ParserContext` migration, Codex status-line parser, Claude `↓ Nk tokens` parser, 2-row TUI badge. v1.13.x (v1.13.0 + v1.13.1) closed ~250–330K daily false-positive alerts that production audit-DB measurement (2026-04-24) revealed: `PERMISSION_PROMPT_MARKERS` / `WAITING_PROMPT_MARKERS` phrase-only contracts; `is_log_like` structural patterns; dropped loose `verbose_answer` / `parse_context_pressure` / `ERROR_MARKERS` / `detect_task_type` substring fallbacks; real-tail regression suite. Slice 4 (v1.14.0 + v1.14.1) ships unified halt-state detection: `IdleCause` enum (PermissionWait / InputWait / LimitHit / WorkComplete / Stale), per-adapter `classify_idle` with 4-step priority (markers → limit → cursor → stillness fallback), `PaneTailHistory` + `IdleTransitionTracker` per-pane caches, `eval_idle_transition` rule fires alerts only on transitions, new `state` row on pane cards (`⏹ IDLE (done)` / `⏸ WAIT (input)` / `⚠ WAIT (approval)` / `⛔ USAGE LIMIT` / `⏸ IDLE (?)`), and `[idle] stillness_polls`. The 2026-04-25 local follow-up fixes the remaining false-IDLE persistence after new requests, broadens Claude usage-limit banners so LIMIT beats IDLE, adds Gemini status-table `context`/model/path parsing, and adds active-safe manual provider runtime refresh (`u` sends Claude `/status`, Codex `/status`, Gemini `/stats session` + `/stats model` + `/stats tools` while active or only heuristically stale; when Claude is explicitly idle/waiting/limited it sends `/status` + `/context` + `/config` + `/stats` + `/usage`) plus `modes`/`access`/`loaded`/`restrict` rows for permission/yolo/auto mode, sandbox, allowed directories, loaded tools/skills/plugins, and restricted tools when provider status/config sources expose them. 426 tests green; clippy + fmt clean; `mission-spec validate .` is blocked locally because `mission-spec` is not installed.

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
