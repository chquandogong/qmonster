# Qmonster

Observe-first TUI for multi-CLI tmux development — watches Claude Code /
Codex / Gemini panes (plus itself), surfaces alerts, token-pressure
metrics, runtime facts, and recommendations. It does not touch observed
panes automatically; the operator can press `u` to cycle read-only
provider runtime slash commands on the selected pane.

- Version: npm package `0.5.0`; current mission ledger `v1.16.51`. Runtime version is sourced from `git describe --tags --always --dirty` via `build.rs` and surfaced in the TUI footer. `Cargo.toml`'s `0.1.0` is internal crate metadata, not the operator-facing version.
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

Current line: `v1.16.51` updates provider pressure semantics and prepares the
first npm package (`qmonster@0.5.0`). Phase C C2 is already complete:
`[tmux] source = "auto"` prefers control-mode and falls back to polling at
startup when attach is unavailable.

| Area | Status | Notes |
| --- | --- | --- |
| Phases 0-5 | Shipped | Planning, observe-first MVP, SQLite/archive/checkpoints, policy engine, provider profiles, and safer prompt-send are complete. |
| Runtime observability | Shipped | Pane state, command row, provider facts, copyable `run:` commands, security posture badges, cost/context/quota gradients, and settings overlay are live. |
| Phase B visibility | Complete | Standard config/pricing paths, command row, Codex in/out token detail, opt-in security advisories, quieter concurrent-work warnings, and Phase-B docs consistency are closed. |
| Phase C C1 | Complete | `src/main.rs` was split into app modules through `src/app/tui_loop.rs`; main is now a thin CLI/startup/TUI wrapper. |
| Phase C C2 | Complete | `PaneSource` supports polling and control-mode; auto source now tries control-mode first with polling fallback. |
| Phase C C3 | Next | Review-tier profiles (`codex-review`, `gemini-policy-review`) remain the next architecture-debt item. |

### Current Metric Contracts

| Metric | Claude | Codex | Gemini |
| --- | --- | --- | --- |
| CTX | `/context` | bottom status line | status table `context` |
| QUOTA 5H | `/usage` Current session | bottom status `5h` | n/a |
| QUOTA WEEK | `/usage` Current week (all models) | bottom status `weekly` | n/a |
| QUOTA | n/a | n/a | single quota surface |
| COST | unset until provider exposes/price config supports it | pricing table + token usage | unset today |

The `S` settings overlay mirrors that contract: cost/context keep
default + provider rows, while quota has default, `claude 5h`,
`claude weekly`, `codex 5h`, `codex weekly`, and `gemini` rows.

Detailed rollout history is kept in `mission-history.yaml` and the
canonical docs under `docs/ai/`; the README tracks the current operator
shape rather than every patch-level slice.

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
#   u        — cycle provider runtime slash sources for the selected pane
#   y        — copy the selected alert's run command when Alerts are focused
#   c        — clear system notices
#   p        — accept pending prompt-send proposal on the selected pane (P5-3
#               safer-actuation; audit: PromptSendAccepted → Completed/Failed,
#               or PromptSendBlocked on observe_only / auto-send-off)
#   d        — dismiss pending prompt-send proposal (audit: PromptSendRejected)
#   S        — open cost/context/quota settings overlay
#   Mouse    — wheel scroll, click select, double-click alert hide
#   Footer version badge — click bottom-right to open Git status

# Standard operator launch. Creates ~/.qmonster/config/qmonster.toml and
# ~/.qmonster/config/pricing.toml from templates when missing, then starts
# Qmonster with --config so the settings overlay can persist edits.
./scripts/run-qmonster.sh

# C2 validation: compare polling and control-mode against the active tmux session.
./scripts/check-tmux-source-parity.sh
./scripts/check-tmux-source-parity.sh --all-targets
./scripts/check-tmux-source-parity.sh --all-targets --repeat 3 --delay-ms 100
./scripts/check-tmux-source-parity.sh --all-targets --strict-title

# Forced control-mode smoke without editing ~/.qmonster/config/qmonster.toml.
# The helper owns --config/--once and accepts only optional --root/--set passthroughs.
./scripts/run-qmonster-control-mode-once.sh

# Override the storage root (useful for tests / sandbox runs)
QMONSTER_ROOT=/tmp/q cargo run -- --once
cargo run -- --root /tmp/q --once
```

For a tmux layout matching Qmonster's pane-title convention, see
`tmux/qmonster.tmux.conf.example`. Runtime-consumed config keys are
documented in `config/qmonster.example.toml`; operator pricing rates
live in `~/.qmonster/config/pricing.toml` using
`config/pricing.example.toml` as the template.

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
  bin/         qmonster-tmux-parity live tmux-source parity checker
  app/         bootstrap, config + safety-precedence, event loop, effect
               runner, version-drift detector, system notices, safety
               audit logging
  domain/      pure types: identity, origin (SourceKind), signal,
               recommendation, audit, lifecycle
  tmux/        PaneSource trait + polling/control-mode sources, shared
               tmux command, target parsing, snapshot hydration,
               polling process boundary, control-mode process flags,
               attach diagnostics/fallback, protocol, and parity helpers
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
npm/           npm bin wrapper for source-based package install
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
./scripts/check-tmux-source-parity.sh
./scripts/check-tmux-source-parity.sh --all-targets
./scripts/check-tmux-source-parity.sh --all-targets --repeat 3
./scripts/run-qmonster-control-mode-once.sh --root /tmp/qmonster-control-mode-smoke
npm pack --dry-run
mission-spec validate .
mission-spec eval --shared .
MISSION_SPEC_CLI=/abs/path/to/mission-spec.js ./scripts/verify-shared.sh
```

If `mission-spec` is not installed, `scripts/verify-shared.sh` still
runs cargo build/test/clippy and falls back to a lite ledger-structure
check with install guidance.

The event-loop integration tests use a fixture `PaneSource` so they do
not require a real tmux session.

## Documentation

- `docs/ai/PROJECT_BRIEF.md` — operating principles and scope
- `docs/ai/ARCHITECTURE.md` — module layout, pipeline, SourceKind, storage
- `docs/ai/VALIDATION.md` — phase-by-phase acceptance checks
- `docs/ai/WORKFLOWS.md` — planning loop, day-end routine, gitignore flip
- `docs/ai/REVIEW_GUIDE.md` — reviewer contract (Codex / Gemini / human)
- `docs/ai/UI_MANUAL.md` — user manual for TUI badges, severity letters, and metrics
- `VERSION.md` — version surface map (ledger tag, npm package, Cargo crate)
- `CONTRIBUTING.md` — local development, documentation, and release rules

## Status & scope

Qmonster is not a provider orchestrator, not a destructive automator,
and not a cloud service. It is a single-user, local-first operating
console. Default action mode is `recommend_only`; refresh policy is
`manual_only`; logging sensitivity is `balanced`. All four safety flags
can only move toward safer via env/CLI — attempted upward overrides
are rejected and audit-logged.
