# Qmonster

Observe-first TUI for multi-CLI tmux development — watches Claude Code /
Codex / Gemini panes (plus itself), surfaces alerts, token-pressure
metrics, and recommendations **without** touching the panes it observes.

- Version: v0.4.0 project phase. Runtime version is sourced from `git describe --tags --always --dirty` via `build.rs` and surfaced in the TUI footer (latest tag: `v1.10.8`). `Cargo.toml`'s `0.1.0` is not the operator-facing version.
- Target env: Ubuntu + tmux + Rust 1.85+
- Name origin: Dr. QUAN's Q + monitoring / master

## Why

Running Claude main, Codex review, and Gemini research side-by-side in
tmux is powerful but hard to babysit: you lose track of which pane is
waiting for approval, which one is bleeding tokens, and which alerted a
security concern two minutes ago.

Qmonster sits in its own tmux pane, polls the others, and shows the
operator-facing signals a human can't keep up with on their own. Its
three guiding principles are:

1. **Observe-first.** Read state before offering any action.
2. **Alert-first.** The loudest surface is the queue of things that
   need attention.
3. **Recommendation-first.** Qmonster recommends; humans (or explicit
   approval gates) act. **No destructive automation by default.**

See `docs/ai/PROJECT_BRIEF.md` for the full statement of intent.

## Phase status

| Phase | Scope                                                                                                                                                                                                         | Status                                                                                      |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| 0     | Planning — canonical docs, mission ledger, thin routers, r1 plan + r2 final synthesis                                                                                                                         | **Shipped**                                                                                 |
| 1     | Observe-first MVP — tmux polling, identity resolver, adapters, alert rules, ratatui UI, desktop/bell notifications, safety precedence, version-drift detector                                                 | **Shipped**                                                                                 |
| 2     | Archive + checkpoint + SQLite — `SqliteAuditSink` with type-level raw exclusion, `ArchiveWriter` preview/full split, `SnapshotWriter`, retention, persistent version drift                                    | **Shipped**                                                                                 |
| 3     | Policy engine A–G + concurrent-work warning + `suggested_command` + strong-rec `next_step` + shared render helper                                                                                             | **Shipped** (gate-approved v1.7.6)                                                          |
| 4     | Provider profile recommender — 3×2 provider/profile grid, structured payload render, side-effects surfacing, and auto-memory routing guidance                                                                 | **Shipped** — Phase 4 complete; P4-8 wrap-up v1.8.12                                        |
| 5     | Manual prompt-send helper (safer actuation) — `PromptSendGate` two-stage (display + execution), `tmux send-keys` with `-l` literal split, 6 `PromptSend*` audit kinds, `p`/`d` TUI keys, stable `proposal_id` | **Shipped** — P5-1 → P5-4 fully gate-approved; audit-vocab arc closed; latest tag `v1.10.8` |

Recent post-Phase-4 TUI follow-ups are already shipped in-tree:
scrollable alerts/panes/help/target picker, mouse interaction, severity
bulk hide, session/window filtering, and a bottom-right version badge
that opens a Git status overlay.

## Quick start

```bash
# Build
cargo build --release

# Smoke test (one iteration; prints pane reports, version snapshot,
# and writes to ~/.qmonster/)
cargo run -- --once

# Launch the TUI
cargo run --release
#   q / Esc  — quit
#   Tab      — switch focus between alerts and pane list
#   ↑ / ↓    — scroll the focused list
#   PgUp/PgDn, Home/End — faster list navigation
#   Enter/Space — toggle auto-hide on the selected alert
#   t        — choose target (session -> window)
#   Enter    — move to window list / confirm window selection
#   Left / Backspace — back to session list
#   ?        — open help / legend overlay
#   r        — re-capture CLI versions; drift appears as a warning alert
#   s        — write a runtime snapshot to ~/.qmonster/snapshots/
#   c        — clear system notices
#   p        — accept pending prompt-send proposal on the selected pane (P5-3
#               safer-actuation; audit: PromptSendAccepted → Completed/Failed,
#               or PromptSendBlocked on observe_only / auto-send-off)
#   d        — dismiss pending prompt-send proposal (audit: PromptSendRejected)
#   Mouse    — wheel scroll, click select, double-click alert hide
#   Footer version badge — click bottom-right to open Git status

# Override the storage root (useful for tests / sandbox runs)
QMONSTER_ROOT=/tmp/q cargo run -- --once
cargo run -- --root /tmp/q --once
```

For a tmux layout matching Qmonster's pane-title convention, see
`tmux/qmonster.tmux.conf.example`. Runtime-consumed config keys are
documented in `config/qmonster.example.toml`.

## Architecture at a glance

```
tmux::RawPaneSnapshot
   ↓
domain::IdentityResolver              (provider + instance + role + IdentityConfidence)
   ↓
adapters::ProviderParser              (one per provider; no identity inference)
   ↓
domain::SignalSet                     (typed signals, each with SourceKind)
   ↓
policy::Engine                        (pure: signals → (Recommendation | RequestedEffect)[])
   ↓
app::EffectRunner                     (allow-list; `recommend_only` by default)
   ↓  ↘
ui::ViewModel           store::EventSink   (Phase 1: NoopSink / InMemorySink)
```

Non-negotiable boundaries:

- Identity resolution **before** provider dispatch.
- `policy/` performs no IO.
- Runtime writes stay inside `~/.qmonster/` (Phase 2 writes `qmonster.db`,
  `archive/YYYY-MM-DD/<pane>/*.log`, `snapshots/*.json`, and
  `versions.json` — never touches project-dir files).
- `audit.rs` writer cannot accept raw bytes — type-level separation.
  The SQLite schema has no raw_tail column; raw tails only live in
  `archive_fs.rs`.

See `docs/ai/ARCHITECTURE.md` for module responsibilities and the full
SourceKind taxonomy (`ProviderOfficial | ProjectCanonical | Heuristic |
Estimated`).

## Repository layout

```
src/
  app/         bootstrap, config + safety-precedence, event loop, effect
               runner, version-drift detector, system notices, safety
               audit logging
  domain/      pure types: identity, origin (SourceKind), signal,
               recommendation, audit, lifecycle
  tmux/        PaneSource trait + polling implementation
  adapters/    claude / codex / gemini / qmonster tail parsers
  policy/      pure engine + rules (alert + advisory + concurrent + profile)
  store/       paths, sink (EventSink + NoopSink + InMemorySink),
               audit (SqliteAuditSink), sqlite (low-level adapter),
               archive_fs (raw tail preview/full split),
               snapshots (operator-requested JSON checkpoints),
               retention (age-based sweep)
  ui/          ratatui dashboard, alerts, panels, theme
  notify/      desktop + terminal-bell + severity-aware rate limiter

docs/ai/       canonical docs (Git-tracked, stable rules)
config/        qmonster.example.toml
tmux/          qmonster.tmux.conf.example
tests/         integration tests
```

Local-only artefacts (gitignored): `.docs/`,
`.mission/CURRENT_STATE.md`, `.mission/snapshots/`,
`.mission/templates/`, `CLAUDE.local.md`. Shared repo ledger artefacts:
`mission.yaml`, `mission-history.yaml`, `docs/ai/*`, `.mission/evals/`.
See `docs/ai/WORKFLOWS.md` §7 for the exact tracking split.

## Development

```bash
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build
mission-spec validate .
mission-spec eval --shared .
MISSION_SPEC_CLI=/abs/path/to/mission-spec.js ./scripts/verify-shared.sh
```

The event-loop integration tests use a fixture `PaneSource` so they do
not require a real tmux session.

## Documentation

- `docs/ai/PROJECT_BRIEF.md` — operating principles and scope
- `docs/ai/ARCHITECTURE.md` — module layout, pipeline, SourceKind, storage
- `docs/ai/VALIDATION.md` — phase-by-phase acceptance checks
- `docs/ai/WORKFLOWS.md` — planning loop, day-end routine, gitignore flip
- `docs/ai/REVIEW_GUIDE.md` — reviewer contract (Codex / Gemini / human)
- `docs/ai/UI_MANUAL.md` — user manual for TUI badges, severity letters, and metrics

## Status & scope

Qmonster is not a provider orchestrator, not a destructive automator,
and not a cloud service. It is a single-user, local-first operating
console. Default action mode is `recommend_only`; refresh policy is
`manual_only`; logging sensitivity is `balanced`. All four safety flags
can only move toward safer via env/CLI — attempted upward overrides
are rejected and audit-logged.
