# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-25 (current — v1.15.7 S3-5 identity-title fallback, builds on v1.15.6 selected-pane state-change visibility)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 + P0-1 Slice 1+2 + v1.13.x emergency suppression + Slice 4 (halted/idle state) shipped; v1.15.x runtime observability, state-change visibility, and S3-5 identity-title fallback applied.** P0-1 Slices 1+2 (v1.11.0..v1.12.2) closed the honest-gap audit for "who used how much + which model"; v1.13.x closed the production false-positive storm with phrase-only marker contracts and real-tail regressions; Slice 4 (v1.14.0 + v1.14.1) ships `IdleCause`, per-adapter idle classification, transition-only idle alerts, and the pane-card `state` row. The v1.15.x runtime follow-ups fix false-IDLE persistence after new requests, make Claude usage-limit banners beat IDLE, parse Gemini status-table `context`/model/path, add a Codex welcome-box `model:` fallback for model/reasoning before the bottom status line appears, and surface provider runtime facts in `modes`/`access`/`loaded`/`restrict` rows. The `u` key now cycles one runtime slash source per press with terminal submit (`C-m`): Claude `/status` → `/usage` → `/stats`, Codex `/status`, Gemini `/stats session` → `/stats model` → `/stats tools`. Claude `/status` output is captured into a one-shot parser overlay and closed with `Escape`; Claude also receives a defensive pre-command `Escape` before each cycled runtime command, while Gemini cycles without pre-`Escape`. The state-change visibility follow-ups add `PaneStateFlash`, a short `CHANGED` pulse on pane header/state rows, temporary `▶ ACTIVE`, and selected-pane flash text fallback: the selection marker expands to repeated `◆ CHANGED ◆` and the selected card title starts with `STATE CHANGED`, so selection styling cannot hide transitions. S3-5 now resolves Claude spinner-prefixed activity titles and Gemini `◇  Ready (...)` idle titles at `IdentityConfidence::Medium` when canonical pane titles are absent; bare glyph-only titles stay `Unknown`. 443 tests green; clippy + fmt clean; `mission-spec validate .` is blocked locally because `mission-spec` is not installed.

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
