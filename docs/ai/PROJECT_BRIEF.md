# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-24 (current — P0-1 Slice 2 confirm-archive v1.12.2)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 complete + P0-1 (provider usage-hint parsing + observability field expansion) Slice 1 + Slice 2 shipped.** Slice 1 (v1.11.0..v1.11.3) closed the honest-gap audit that v1.10.9's TUI showed blanks for "who used how much + which model": new `PricingTable` module (operator-curated TOML, zero-rate-as-unset), `SignalSet.model_name` field, `ProviderParser::parse(.., pricing: &PricingTable)` trait extension, Codex status-line parser (context %, tokens, model, cost_usd via Estimated/0.7 pricing lookup), Claude `↓ Nk tokens` working-line parser, UI MODEL badge + K/M count suffix. Slice 2 (v1.12.0..v1.12.2) extended the surface with 3 new SignalSet fields (`git_branch`, `worktree_path`, `reasoning_effort`), Claude `model_name` via external `~/.claude/settings.json` through new `ClaudeSettings` module, `ProviderParser` migration from 3 positional args to `&ParserContext` struct, Codex `/status` box reasoning-effort parser anchored to `│` + `Model:` structure, and 2-row `metric_badge_line` TUI (`metrics:` primary row + `context:` row). All slices cross-checked end-to-end by Codex + Gemini through the confirm-archive pattern (mission-history change_sequence 44-51 for detail). 332 tests green; clippy + fmt clean; audit-vocab compile-time safety (v1.10.4/5) still in effect giving `AuditEventKind` a single-source-of-truth string form via `as_str` / `Display` / `AsRef<str>`. Phases 1–5 + P0-1 Slices 1–2 all gate-approved; no outstanding debt.

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
4. **Official vs heuristic**. Every number and every lever is labeled
   either `(official)` (citing the provider's own doc) or `(heuristic)` /
   `(estimated)` (from community tools or Qmonster inference).
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
