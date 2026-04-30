# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-30 (current — v1.36.1 release-hygiene follow-up on the v1.36.0 PolicyGates input-bundle refactor + SBOM release provenance + dashboard screenshot; latest runtime-behavior sync remains v1.35.2/v1.35.x layered on v1.35.0 Phase F F-7d + v1.35.1 polish bundle)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 + Phase B + Phase C C1/C2/C3 + Phase D D1/D2/D3 + Phase E E1/E2 + Phase F F-1/F-2/F-3/F-4/F-4b/F-5/F-5b/F-6/F-7/F-7b/F-7c/F-7d/F-7-config + Phase G G-1/G-2 are complete.** Current v1.36.1 preserves v1.36.0 behavior and fixes the release/package/doc sync: npm tarballs include `docs/RELEASE_VERIFICATION.md`, the release workflow creates `dist/` before writing the SBOM, and canonical docs reflect the v1.36.0/v1.36.1 state. v1.36.0 closes the v1.35.x confirm-round constructor follow-up by replacing the 9-positional-arg `PolicyGates::from_config_and_identity` path with `PolicyGates::from_inputs(PolicyGateInputs<'_>)`; this is an internal policy API refactor with no operator-facing behavior change. Current v1.35.x runtime behavior keeps F-7d `[reset]` thresholds operator-tunable via `qmonster.toml`, shows reset values in Settings `Parameters`, reset activation conditions in Settings `Rules`, and source/metric explanations in Settings `Badges`. Expanded panes surface `token io` input/output and `cache io` read/create counts when provider data exists. Gemini `/stats session` surfaces `Tool Calls` as a `CALLS` runtime badge, and idle Gemini `/model` `Reset:` / `Resets:` rows surface the current status-table model family only (for example `gemini-3.1-pro-preview` -> `Pro`) as display-only `RESET` runtime facts plus a top metrics badge with provider-rendered time/remaining text. Policy-grade reset ETA still comes only from Claude sidefile / Codex app-server `quota_*_resets_at`; per-subagent token attribution remains deferred because providers do not expose structured per-subagent counters. Repository presentation now includes a README dashboard screenshot and tagged releases include an SPDX-JSON SBOM with release-verification guidance.

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
