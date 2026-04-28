# Qmonster

Observe-first TUI for multi-CLI tmux development — watches Claude Code /
Codex / Gemini panes (plus itself), surfaces alerts, token-pressure
metrics, runtime facts, and recommendations. It does not touch observed
panes automatically; the operator can press `u` to cycle read-only
provider runtime slash commands on selected non-Claude panes.

- Version: npm package `1.21.3`; current mission ledger `v1.22.0`. Runtime version is sourced from `git describe --tags --always --dirty` via `build.rs` and surfaced in the TUI footer. `Cargo.toml`'s `0.1.0` is internal crate metadata, not the operator-facing version.
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

Current release: `v1.22.0` / npm `1.21.3` (npm publish deferred).

`v1.22.0` opens Phase F with F-1: process RSS is surfaced as `memory <N> MB [Heur]`
on Claude/Codex pane cards. tmux `#{pane_pid}` is captured as the 9th
`PANE_LIST_FORMAT` field; a new `adapters::process_memory` helper walks
`/proc/<pid>/task/<pid>/children` recursively (depth ≤ 5, visited-set) to
find the highest-RSS descendant, preferring `claude/codex/gemini/node/python`
comm names. Gemini's status-table `[Official]` MEM path is preserved
untouched — the `/proc` fill only applies when the provider adapter left
`process_memory_mb` as `None`. 608 lib + 67 integration tests green.

| Area                  | Status   | Operator-visible result                                                                                                                                  |
| --------------------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Phases 0-5            | Shipped  | Planning, observe-first MVP, SQLite/archive/checkpoints, policy engine, provider profiles, and safer prompt-send are complete.                           |
| Runtime observability | Shipped  | Pane state, command row, provider facts, copyable `run:` commands, security posture badges, cost/context/quota gradients, and settings overlay are live. |
| Phase B visibility    | Complete | Standard config/pricing paths, Codex token in/out, opt-in security advisories, branch-aware concurrent-work warnings, and copyable commands are closed.  |
| Phase C C1            | Complete | `src/main.rs` is a thin CLI/startup wrapper; the live TUI loop and helpers live under `src/app/`.                                                        |
| Phase C C2            | Complete | `[tmux] source = "auto"` tries control-mode first and falls back to polling when attach is unavailable.                                                  |
| Phase C C3            | Complete | Review-tier profiles (`codex-review`, `gemini-policy-review`) fire on healthy `Role::Review` panes with source-labeled payloads.                         |
| Phase D D1            | Shipped  | Opt-in cross-window concurrent-work findings exist for same path + branch panes across tmux windows.                                                     |
| Phase D D2            | Shipped  | Opt-in identity-drift findings catch provider or worktree changes in the same pane, with per-session dedup.                                              |
| Phase D D3            | Closed   | Claude subagent detection is refined; per-subagent token attribution stays permanently deferred because providers do not expose counters.                |
| Phase E E1            | Shipped  | Gemini status-table memory is parsed into `MEM` badges and `--once` metrics.                                                                             |
| Phase E E2            | Shipped  | Settings overlay writes preserve existing TOML comments, unrelated sections, and key order.                                                              |
| Phase F F-1           | Shipped  | Process RSS surfaced as `memory <N> MB [Heur]` on Claude/Codex pane cards via /proc descendant walk; Gemini status-table `[Official]` path untouched.    |

Recent release notes:

- `v1.21.3`: Claude `/clear` `CTX —` becomes `CTX 0%`; package metadata
  and repo license switch to MIT.
- `v1.21.2`: Claude statusline percent parsing is bounded per column so
  placeholders do not steal adjacent values.
- `v1.21.1`: Codex `/clear` keeps visible CTX/quota/model/path/token
  fields even when the total token field is absent.
- `v1.21.0`: Gemini `memory` status-table column surfaces as `MEM`.
- `v1.20.0`: settings overlay saves only threshold keys without
  rewriting the rest of `qmonster.toml`.

### Current Metric Contracts

| Metric     | Claude                                                | Codex                                                  | Gemini                 |
| ---------- | ----------------------------------------------------- | ------------------------------------------------------ | ---------------------- |
| CTX        | statusline `CTX`; `CTX —` after `/clear` renders `0%` | bottom status line                                     | status table `context` |
| QUOTA 5H   | statusline `5h`                                       | bottom status `5h` remaining, inverted to pressure     | n/a                    |
| QUOTA WEEK | statusline `7d`                                       | bottom status `weekly` remaining, inverted to pressure | n/a                    |
| QUOTA      | n/a                                                   | n/a                                                    | single quota surface   |
| COST       | unset until provider exposes/price config supports it | pricing table + token usage                            | unset today            |

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
#   u        — force-poll Claude statusline; cycle runtime slash sources for Codex/Gemini
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
- `LICENSE` — MIT license

## Status & scope

Qmonster is not a provider orchestrator, not a destructive automator,
and not a cloud service. It is a single-user, local-first operating
console. Default action mode is `recommend_only`; refresh policy is
`manual_only`; logging sensitivity is `balanced`. All four safety flags
can only move toward safer via env/CLI — attempted upward overrides
are rejected and audit-logged.

## License

MIT. See `LICENSE`.
