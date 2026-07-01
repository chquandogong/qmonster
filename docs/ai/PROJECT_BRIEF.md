# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: **v3.1.3** (planning doc rev v0.4.0 — header tracks ledger)
- **Current reality (read first):** the phase inventory and per-tag table
  below are **historical**. In **v3.0.0 (narrow reduction)** Qmonster was cut
  back to its provider-neutral observe/recommend core — the Metrics / Anomaly
  Events / Token Insights / Pending-Actions / fx overlays, the Token Insights
  subsystem, the provider-profile recommender, the Codex app-server, Gemini
  `/stats`+`/model` enrichment, and the Settings Rules/Badges tabs were all
  removed. **v3.1.0–v3.1.3** then layered the uniform gauge-bar dashboard,
  `agy` (Antigravity) opt-in enrichment, compact non-selected panes, and the
  no-forced-selection pane list on top. Current operating state lives in
  `.mission/CURRENT_STATE.md`; canonical UI/architecture in
  `docs/ai/UI_MANUAL.md` + `ARCHITECTURE.md`.
- Date: 2026-04-20 (initial) → 2026-05-13 (v2.3.0) → 2026-05-21
  (v2.4.0 — `Provider::Antigravity` (Google `agy` CLI) ObserveOnly
  identification, Provider Setup 5th `agy` tab, tmux bundle pane 0.2
  → `agy:1:research`) → 2026-05-21 (current — v2.4.1 patch: Provider
  Setup tab header regression fix + `ProviderSetupTab::ALL` /
  `index()` single-source-of-truth refactor; v2.4.0 ledger-sync misses
  corrected (`README.md` Cargo crate version, `docs/ai/UI_MANUAL.md`
  "4개 탭" description)). Per-version prose lives in
  `mission-history.yaml` `change_sequence` (current tail entry 217).
  The summary table below replaces the prior multi-page inline
  narrative; consult the ledger for full per-tag detail.

  | Tag           | Date                    | One-line shape                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
  | ------------- | ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | v3.1.3        | 2026-07-01              | No forced pane selection — the dashboard launches with nothing selected (all panes compact); `↓`/`↑` enter at the first/last pane, stepping off either end returns to the clean state, and any selected pane (Qmonster monitor included) expands.                                                                                                                                                                                                                                                                                                    |
  | v3.1.0–v3.1.2 | 2026-06-30              | Uniform per-CLI gauge-bar dashboard (ctx/5h/7d as bars + at-a-glance status pills); `agy` footer/sidefile enrichment restored (opt-in, ObserveOnly held); Codex quota `% left` parse + `agy` version-probe fixes; agy idle stillness fallback on the common path; compact non-selected panes; dead-shortcut cleanup (`K`/`n`/`a`).                                                                                                                                                                                                                   |
  | v3.0.0        | 2026-06-30              | **Narrow reduction** — 10-slice TDD cut back to the provider-neutral observe/recommend core (removed the overlay/insights/profile/enrichment sprawl — see the "Current reality" banner above).                                                                                                                                                                                                                                                                                                                                                       |
  | v2.5.0–v2.8.0 | 2026-06-29              | Structured data-source migration (Claude sidefile-preferred pressures, Codex codex-tui rollout backstop); `agy` transcript/footer/sidefile enrichment (opt-in, ObserveOnly) + quota; `SignalSet.context_window_size`; Gemini cross-review leg retired → Codex + human.                                                                                                                                                                                                                                                                               |
  | v2.4.1        | 2026-05-21              | Patch on v2.4.0 — Provider Setup tab header regression fix; `ProviderSetupTab::ALL` const + `match`-based `index()` as single source of truth; `dashboard.rs` + `TAB_BY_INDEX` consume `ALL`; new `all_constant_is_in_sync_with_index_method_and_labels` regression test. Also corrects v2.4.0 ledger-sync misses in `README.md` (Cargo crate version row) and `docs/ai/UI_MANUAL.md` (Provider Setup tab description).                                                                                                                              |
  | v2.4.0        | 2026-05-21              | First feature minor on v2.x — `Provider::Antigravity` (Google `agy` CLI) ObserveOnly identification via Tasks-1-7 TDD bundle; six independent ObserveOnly gates keep agy panes off every analytic surface; Provider Setup overlay gains 5th `agy` tab; tmux bundle pane 0.2 → `agy:1:research`; `detect_provider_title` reorder (spinner-activity before keyword fallback); `process_memory` 2-tier descendant-walk priority. Gemini code/fixtures/mission-history untouched — Enterprise/Cloud/Standard Gemini CLI retains support past 2026-06-18. |
  | v2.3.0        | 2026-05-13              | Post-v2.2.0 stabilization + UX bundle: path-row worktree-role hint, sectioned pane panel tree (NOW/WHERE/PRESSURE/RUNTIME/RECOMMENDATIONS + `├/└/│`), alert related-context rail, footer chip mouse actions, copied-outcome lifecycle + done_when_refs 100 %, settings module split, dependabot bump (thiserror 2 / crossterm 0.29 / rusqlite 0.39 / toml 1.1 / toml_edit 0.25 / attest-build-provenance v4)                                                                                                                                         |
  | v2.2.0        | 2026-05-12              | Critical-eval P0/P1 bundle: dead-code purge, log_storm + quota dedup, threshold provenance tags, AnomalyKind::supported_providers gating, SubagentSideEffect honesty prose, RecommendationOutcome::Copied lifecycle                                                                                                                                                                                                                                                                                                                                  |
  | v2.1.0        | 2026-05-11              | r2 synthesis slices: attribution lock + pane-bucketed Insights + ROI loop + anomaly time-normalization + UI Now-strip / NBA / ETA chips                                                                                                                                                                                                                                                                                                                                                                                                              |
  | v2.0.0        | 2026-05-08              | Milestone marker — version surface alignment (1.60.0 → 2.0.0), no semver break, no API change                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
  | v1.55–v1.60   | 2026-05-08              | Polish bundles: light theme, n-overlay filter, hover_help expansion, provider-honesty cost chip, alerts filter, fx overlay polish                                                                                                                                                                                                                                                                                                                                                                                                                    |
  | v1.50.0       | 2026-05-08              | Insights overlay async load + spinner; CrossPaneEditCluster fold-back; provider-coverage matrix                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
  | v1.40s        | 2026-05-06              | Operator-controlled overlay geometry (m/a drag-to-move, separator drag); confirm_actions bypass discoverability                                                                                                                                                                                                                                                                                                                                                                                                                                      |
  | v1.43–v1.48   | 2026-05-07              | Phase 7 (anomaly subsystem) + Phase 8 (Token Insights)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
  | v1.36–v1.39   | 2026-04-30 → 2026-05-04 | Release-pipeline hardening + F-8/F-9/F-9b feature slices + UX polish wave                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
  | v1.0–v1.35    | 2026-04-20 → 2026-04-30 | Phases 1–5 + Phases B / C / D / E / F / G / H — see `mission-history.yaml`                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |

- Target env: Ubuntu + tmux + Rust TUI
- Phase: **All planned phases complete** _(historical — scope was reduced to the observe/recommend core in v3.0.0; see the "Current reality" banner)._
  - Foundational: Phases 1–5, Phase B, Phase C (C1/C2/C3), Phase D (D1/D2/D3), Phase E (E1/E2), Phase F (F-1…F-9b), Phase G (G-1/G-2), Phase H, Phase 6 Team Mode.
  - Analytical: Phase 7 (anomaly subsystem) v1 → v2 promotion → v2 detectors → v3 (a/b/c) — **anomaly DETECTION survives**; Phase 8 (Token Insights) v1 + v2 — **removed entirely in v3.0.0** (store, CLI subcommands, telemetry, overlay).
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
