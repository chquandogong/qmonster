# Qmonster

Observe-first TUI for multi-CLI tmux development — watches Claude Code /
Codex / Gemini panes (plus itself), surfaces alerts, token-pressure
metrics, runtime facts, and recommendations. It does not touch observed
panes automatically; the operator can press `u` to cycle read-only
provider runtime slash commands on the selected pane.

- Version: v0.4.0 project phase. Runtime version is sourced from `git describe --tags --always --dirty` via `build.rs` and surfaced in the TUI footer (latest tag in this workspace: `v1.16.17`; current canonical ledger: `v1.16.17`). `Cargo.toml`'s `0.1.0` is not the operator-facing version.
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

| Phase              | Scope                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Status                                                                                                                                                                                                                                                                                                |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0                  | Planning — canonical docs, mission ledger, thin routers, r1 plan + r2 final synthesis                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | **Shipped**                                                                                                                                                                                                                                                                                           |
| 1                  | Observe-first MVP — tmux polling, identity resolver, adapters, alert rules, ratatui UI, desktop/bell notifications, safety precedence, version-drift detector                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | **Shipped**                                                                                                                                                                                                                                                                                           |
| 2                  | Archive + checkpoint + SQLite — `SqliteAuditSink` with type-level raw exclusion, `ArchiveWriter` preview/full split, `SnapshotWriter`, retention, persistent version drift                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | **Shipped**                                                                                                                                                                                                                                                                                           |
| 3                  | Policy engine A–G + concurrent-work warning + `suggested_command` + strong-rec `next_step` + shared render helper                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | **Shipped** (gate-approved v1.7.6)                                                                                                                                                                                                                                                                    |
| 4                  | Provider profile recommender — 3×2 provider/profile grid, structured payload render, side-effects surfacing, and auto-memory routing guidance                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | **Shipped** — Phase 4 complete; P4-8 wrap-up v1.8.12                                                                                                                                                                                                                                                  |
| 5                  | Manual prompt-send helper (safer actuation) — `PromptSendGate` two-stage (display + execution), `tmux send-keys` with `-l` literal split, 6 `PromptSend*` audit kinds, `p`/`d` TUI keys, stable `proposal_id`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | **Shipped** — P5-1 → P5-4 fully gate-approved; audit-vocab arc closed                                                                                                                                                                                                                                 |
| P0-1               | Provider usage-hint parsing + observability field expansion — `PricingTable` + `ClaudeSettings` operator-config readers, `ProviderParser` → `&ParserContext` struct, 7-metric Codex populate (context / tokens / model / cost / branch / path / reasoning effort), Claude `↓ Nk tokens` + `settings.json` model, 2-row TUI metric badge line, honesty regression tests locking tail-based absence                                                                                                                                                                                                                                                                                                                                                                                               | **Shipped** — Slice 1 (v1.11.0–v1.11.3) + Slice 2 (v1.12.0–v1.12.2) fully gate-approved                                                                                                                                                                                                               |
| v1.13.x            | Emergency false-positive suppression — `PERMISSION_PROMPT_MARKERS` / `WAITING_PROMPT_MARKERS` phrase-only contracts, `is_log_like` structural patterns, drop loose `verbose_answer` / `parse_context_pressure` / `ERROR_MARKERS` / `detect_task_type` substring fallbacks, real-tail regression suite                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | **Shipped** — v1.13.0 (4 markers) + v1.13.1 (error_hint + context_pressure); single-version pattern, confirm-archive deferred to Slice 4                                                                                                                                                              |
| Slice 4            | Halted/idle state detection — `IdleCause` hybrid classifier (marker → limit → cursor → stillness fallback), per-adapter `classify_idle` for Claude/Codex/Gemini/Qmonster, `PaneTailHistory` + `IdleTransitionTracker` per-pane caches, `eval_idle_transition` rule with transition-only firing, new `state` row on pane cards, `[idle] stillness_polls` config knob                                                                                                                                                                                                                                                                                                                                                                                                                             | **Shipped** — v1.14.0 (16-commit chain) + v1.14.1 cursor-fix (Codex bottom-status-line skip); rolled forward into v1.15.0                                                                                                                                                                             |
| Runtime facts      | Provider runtime fact display — manual `u` key cycles read-only slash commands with terminal submit (`C-m`, Enter-equivalent), one command per press: Claude `/status` → `/usage` → `/stats`, Codex `/status`, Gemini `/stats session` → `/stats model` → `/stats tools`. Claude `/status` output is captured before Qmonster sends `Escape`, then parsed once from an in-memory overlay so the pane is ready for the next slash command. Claude gets a defensive `Escape` before each cycled command to close any prior fullscreen runtime surface; Gemini does not. Adapter parsers surface permission/yolo/auto mode, sandbox, allowed dirs, loaded tools/skills/plugins, restricted tools, and Gemini status-table context/model/path fields when exposed by provider status/config sources | **Shipped** — v1.15.x; display-only facts with `SourceKind` (prose-derived → Heuristic, settings/box/table-validated → ProviderOfficial); unknown fields stay blank                                                                                                                                   |
| S3-5               | Identity resolver title fallback — Claude Code spinner-prefixed activity titles (`⠂ Analyze project ...`) and Gemini idle titles (`◇  Ready (project)`) resolve at `IdentityConfidence::Medium` when canonical pane titles are absent and the underlying command is `node`; bare glyph false positives stay `Unknown`                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | **Shipped** — v1.15.7; remaining Slice 3 backlog is S3-1, S3-3, S3-4 plus partial housekeeping (dead error variants already removed post-tag)                                                                                                                                                         |
| Pressure gradients | Operator-actionable metrics surface gradient advisories before they hit hard limits — `context_pressure_warning`/`_critical` (Phase 3), `quota_pressure_warning`/`_critical` (v1.15.11), and `cost_pressure_warning`/`_critical` (v1.15.14). All three use the `>=warning..<critical` / `>=critical` band shape, carry `SourceKind::Estimated` (thresholds are Qmonster picks), and respect the `IdentityConfidence` gate. Critical variants are `is_strong` and surface in the CHECKPOINT slot. The CTX / QUOTA / COST badges share a uniform severity-tinting contract (v1.15.15)                                                                                                                                                                                                             | **Shipped** — v1.15.8 quota metric → v1.15.10 LimitHit fix → v1.15.11 quota gradient → v1.15.12 SourceKind honesty → v1.15.13 input/output tokens (S3-1) → v1.15.14 cost gradient → v1.15.15 COST badge severity; alerts/panes split-pane resizer (`[`/`]`/`=` + drag) shipped alongside in `8d0764a` |

Recent post-Phase-4 TUI follow-ups are already shipped in-tree:
scrollable alerts/panes/help/target picker, mouse interaction, severity
bulk hide, session/window filtering, and a bottom-right version badge
that opens a Git status overlay. The Slice 4 `state` row appears on
each pane card when the pane is halted, with glyph + cause label +
elapsed time (`⏹ IDLE (done)` / `⏸ WAIT (input)` / `⚠ WAIT (approval)`
/ `⛔ USAGE LIMIT` / `⏸ IDLE (?)` for the stillness fallback). State
transitions now add a short `CHANGED` pulse on the pane header and
`state` row; a return to active briefly renders `▶ ACTIVE` before the
row disappears again. If the changed pane is selected, the selection
marker expands to `◆ CHANGED ◆` on every selected line and the card
header starts with `STATE CHANGED` during the flash.
The Phase-B visibility slice adds a `cmd` row sourced from tmux
`pane_current_command` on pane cards/detail/`--once`, plus selected-pane
Codex input/output token breakdown when the bottom status line exposes
ProviderOfficial `in`/`out` counts. The security posture slice keeps
YOLO / bypass / no-sandbox facts badge-only by default; set
`[security] posture_advisories = true` to promote them into passive
Concern recommendations.
Concurrent-work warnings are also quieter after v1.15.23: same path
alone is no longer enough; panes must expose the same git branch too.
The selected alert's `run:` command is copyable with `y` when Alerts are
focused; clipboard failure or a no-command selection is reported as a
visible system notice. The `c` key is reserved for system-notice clear.
Phase C has started: v1.16.0 extracts dashboard focus, selection, and
list hit-test helpers from `src/main.rs` into `src/app/keymap.rs`;
v1.16.1 moves the session/window target picker model, preview, and
choice logic into `src/app/target_picker.rs`; v1.16.2 moves provider
runtime-refresh command selection, cycling, send/capture, and notice
label helpers into `src/app/runtime_refresh.rs`; v1.16.3 moves alert
selection/hide/double-click and pane-state flash synchronization into
`src/app/dashboard_state.rs`; v1.16.6 moves shared git/help scroll modal
open/close/scroll state and key/mouse handlers into
`src/app/modal_state.rs`; v1.16.7 moves settings overlay key and mouse
dispatch into `src/app/settings_overlay.rs`; v1.16.8 moves operator
version-refresh and snapshot-write helpers into
`src/app/operator_actions.rs`; v1.16.9 moves `--once` report formatting
into `src/app/once_report.rs`; v1.16.10 moves prompt-send accept/dismiss
handling into `src/app/prompt_send_actions.rs`; v1.16.11 moves
runtime-refresh action orchestration into `src/app/runtime_refresh.rs`;
v1.16.12 moves selected-alert command copy notices into
`src/app/clipboard_actions.rs`; v1.16.13 moves target-picker
open/key/mouse dispatch into `src/app/target_picker.rs`; v1.16.14
moves dashboard Alerts/Panes selection key dispatch into
`src/app/dashboard_state.rs`; v1.16.15 moves dashboard mouse dispatch
into `src/app/dashboard_state.rs`; v1.16.16 moves default config-path
resolution into `src/app/path_resolution.rs`; v1.16.17 moves initial target
selection into `src/app/target_picker.rs`. The next C1 slices should keep
thinning event-loop orchestration before control-mode adapter work.

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

## Status & scope

Qmonster is not a provider orchestrator, not a destructive automator,
and not a cloud service. It is a single-user, local-first operating
console. Default action mode is `recommend_only`; refresh policy is
`manual_only`; logging sensitivity is `balanced`. All four safety flags
can only move toward safer via env/CLI — attempted upward overrides
are rejected and audit-logged.
