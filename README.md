# Qmonster

Observe-first TUI for multi-CLI tmux development — watches Claude Code /
Codex / Gemini panes (plus itself), surfaces alerts, token-pressure
metrics, and recommendations **without** touching the panes it observes.

- Version: v0.4.0 (Phase 1 shipped)
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

| Phase | Scope                                                                                                                                                         | Status         |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| 0     | Planning — canonical docs, mission ledger, thin routers, r1 plan + r2 final synthesis                                                                         | **Shipped**    |
| 1     | Observe-first MVP — tmux polling, identity resolver, adapters, alert rules, ratatui UI, desktop/bell notifications, safety precedence, version-drift detector | **Shipped**    |
| 2     | Archive + checkpoint + SQLite                                                                                                                                 | _pending gate_ |
| 3     | Policy engine A–G + concurrent-work warning                                                                                                                   | not started    |
| 4     | Provider profile recommender                                                                                                                                  | not started    |
| 5     | Manual prompt-send helper (safer actuation)                                                                                                                   | not started    |

## Quick start

```bash
# Build
cargo build --release

# Smoke test (one iteration; prints pane reports and version snapshot)
cargo run -- --once

# Launch the TUI
cargo run --release
#   q / Esc  — quit
#   r        — re-capture CLI versions; drift appears as a warning alert
#   c        — clear system notices
```

For a tmux layout matching Qmonster's pane-title convention, see
`tmux/qmonster.tmux.conf.example`. Default config is
`config/qmonster.example.toml`.

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
- Runtime writes stay inside `~/.qmonster/` (Phase 1 writes nothing).
- `audit.rs` writer cannot accept raw bytes — type-level separation.

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
  policy/      pure engine + alert rules
  store/       EventSink trait + NoopSink + InMemorySink
  ui/          ratatui dashboard, alerts, panels, theme
  notify/      desktop + terminal-bell + severity-aware rate limiter

docs/ai/       canonical docs (Git-tracked, stable rules)
config/        qmonster.example.toml
tmux/          qmonster.tmux.conf.example
tests/         integration tests
```

Local, single-user artefacts (gitignored): `.docs/`, `mission.yaml`,
`mission-history.yaml`, `.mission/`, `CLAUDE.local.md`. The project is
in the single-user phase of its gitignore policy; see
`docs/ai/WORKFLOWS.md` §7 for the team/CI flip path.

## Development

```bash
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build
```

Phase 1 ships with ~90 tests (lib + integration). The event-loop
integration tests use a fixture `PaneSource` so they do not require a
real tmux session.

## Documentation

- `docs/ai/PROJECT_BRIEF.md` — operating principles and scope
- `docs/ai/ARCHITECTURE.md` — module layout, pipeline, SourceKind, storage
- `docs/ai/VALIDATION.md` — phase-by-phase acceptance checks
- `docs/ai/WORKFLOWS.md` — planning loop, day-end routine, gitignore flip
- `docs/ai/REVIEW_GUIDE.md` — reviewer contract (Codex / Gemini / human)

## Status & scope

Qmonster is not a provider orchestrator, not a destructive automator,
and not a cloud service. It is a single-user, local-first operating
console. Default action mode is `recommend_only`; refresh policy is
`manual_only`; logging sensitivity is `balanced`. All four safety flags
can only move toward safer via env/CLI — attempted upward overrides
are rejected and audit-logged.
