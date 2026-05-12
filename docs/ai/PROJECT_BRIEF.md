# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: **v2.2.0** (planning doc rev v0.4.0 — header tracks ledger)
- Date: 2026-04-20 (initial) → 2026-05-12 (current — v2.2.0). Per-version
  prose lives in `mission-history.yaml` `change_sequence` (current tail
  ≈ entry 210+). The summary table below replaces the prior multi-page
  inline narrative; consult the ledger for full per-tag detail.

  | Tag         | Date                    | One-line shape                                                                                                                                                                                                      |
  | ----------- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | v2.2.0      | 2026-05-12              | Critical-eval P0/P1 bundle: dead-code purge, log_storm + quota dedup, threshold provenance tags, AnomalyKind::supported_providers gating, SubagentSideEffect honesty prose, RecommendationOutcome::Copied lifecycle |
  | v2.1.0      | 2026-05-11              | r2 synthesis slices: attribution lock + pane-bucketed Insights + ROI loop + anomaly time-normalization + UI Now-strip / NBA / ETA chips                                                                             |
  | v2.0.0      | 2026-05-08              | Milestone marker — version surface alignment (1.60.0 → 2.0.0), no semver break, no API change                                                                                                                       |
  | v1.55–v1.60 | 2026-05-08              | Polish bundles: light theme, n-overlay filter, hover_help expansion, provider-honesty cost chip, alerts filter, fx overlay polish                                                                                   |
  | v1.50.0     | 2026-05-08              | Insights overlay async load + spinner; CrossPaneEditCluster fold-back; provider-coverage matrix                                                                                                                     |
  | v1.40s      | 2026-05-06              | Operator-controlled overlay geometry (m/a drag-to-move, separator drag); confirm_actions bypass discoverability                                                                                                     |
  | v1.43–v1.48 | 2026-05-07              | Phase 7 (anomaly subsystem) + Phase 8 (Token Insights)                                                                                                                                                              |
  | v1.36–v1.39 | 2026-04-30 → 2026-05-04 | Release-pipeline hardening + F-8/F-9/F-9b feature slices + UX polish wave                                                                                                                                           |
  | v1.0–v1.35  | 2026-04-20 → 2026-04-30 | Phases 1–5 + Phases B / C / D / E / F / G / H — see `mission-history.yaml`                                                                                                                                          |

- Target env: Ubuntu + tmux + Rust TUI
- Phase: **All planned phases complete.**
  - Foundational: Phases 1–5, Phase B, Phase C (C1/C2/C3), Phase D (D1/D2/D3), Phase E (E1/E2), Phase F (F-1…F-9b), Phase G (G-1/G-2), Phase H, Phase 6 Team Mode.
  - Analytical: Phase 7 (anomaly subsystem) v1 → v2 promotion → v2 detectors → v3 (a/b/c), Phase 8 (Token Insights) v1 + v2.
  - Post-phase line (v1.36+): release-pipeline hardening, operator-UX polish, overlay geometry, attribution-lock + insights synthesis, critical-eval-driven dead-code purge and honesty improvements.
  - Per-tag deltas are in `mission-history.yaml`. The current operating
    state is best understood via `.mission/CURRENT_STATE.md` (latest 2–3
    versions only) plus the `MDR-DRAFT-v2.2.0-*` files for pending
    operator decisions.

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
