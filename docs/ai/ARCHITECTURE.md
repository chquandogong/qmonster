# ARCHITECTURE

- Version: v0.4.0
- Date: 2026-04-20 (round r2 reconciled) / 2026-04-27 (implementation sync through v1.16.51 Claude context + split quota)
- Status: canonical architecture reference; phase notes below describe the historical rollout and current invariants.

## One-line shape (r2 canonical)

```
tmux::RawPaneSnapshot
   → domain::IdentityResolver
   → adapters::ProviderParser
   → domain::SignalSet
   → policy::Engine
   → app::EffectRunner
   ↘                     ↘
  ui::ViewModel       store::EventSink
```

Two non-negotiable rules:

1. **Identity resolution precedes provider dispatch.** `adapters/`
   never infers `provider` or `role` — it receives a resolved identity
   from `domain/` and parses tails into signals.
2. **Policy is pure.** `policy/` performs no IO. It returns
   `Recommendation`s (advisory) and `RequestedEffect`s (gated by the
   actuation allow-list in `app::EffectRunner`).

## Core loop

```
observe → classify → recommend → archive → checkpoint → limited actuation
```

No step silently skips another. `limited actuation` is restricted to
the allow-list in `mission.yaml` constraints and `config [actions]`.
Phase 1 kept only an in-memory `EventSink`; Phase 2 added archive,
checkpoint, retention, and durable audit storage.

## Module layout (Rust crate)

```
src/
  main.rs      # thin CLI/startup/--once/TUI-entry wrapper
  app/         # bootstrap/startup, config+safety-precedence, path resolution, event loop, tui-loop/dashboard-runtime/polling-tick/terminal-session/dashboard-render/keymap/target-picker/runtime-refresh/dashboard-state/modal/settings/operator-action/once-output/prompt-send/clipboard helpers, effect gate
  domain/      # pure types: identity, origin, signal, recommendation, audit, lifecycle
  tmux/        # PaneSource trait; auto source prefers control-mode with polling fallback
  adapters/    # per-provider tail parsers (no identity inference)
  policy/      # pure rules; Phase 1 = rules/alerts.rs;
               # Phase 3 adds rules/{advisories,concurrent}.rs;
               # Phase 4 adds rules/profiles.rs (provider-profile recommender)
  store/       # Phase 1: EventSink trait + NoopSink/InMemorySink
               # Phase 2: sqlite, archive_fs, audit (type-level raw split), snapshots, retention
  ui/          # ratatui widgets, alert queue, per-pane panels, theme
  notify/      # desktop / terminal bell; severity-aware rate limiting
```

The long-term target is still a thinner `src/main.rs`. v1.16.0 begins
that work by moving dashboard focus, selection, and list hit-testing
helpers into `app::keymap`; v1.16.1 moves the session/window target
picker model, preview, choice application, and row hit-test logic into
`app::target_picker`; v1.16.2 moves runtime-refresh command selection,
cycling, send/capture, and operator-facing label helpers into
`app::runtime_refresh`; v1.16.3 moves alert selection, hide/defer,
double-click tracking, and pane-state flash synchronization into
`app::dashboard_state`. The current implementation still keeps the live
TUI event loop in `main.rs`.
v1.16.4 fixed Git overlay title consistency by using the same
`QMONSTER_GIT_VERSION` value as the footer badge. v1.16.5 restores `c`
to its original system-notice clear role; alert command copy remains on
`y`. v1.16.6 moves shared git/help scroll modal open/close/scroll state
and key/mouse handling into `app::modal_state`. v1.16.7 moves settings
overlay key and mouse dispatch into `app::settings_overlay`. v1.16.8
moves operator version-refresh and snapshot-write helpers into
`app::operator_actions`. v1.16.9 moves `--once` report formatting into
`app::once_report`. v1.16.10 moves prompt-send accept/dismiss handling
into `app::prompt_send_actions`. v1.16.11 moves runtime-refresh action
orchestration into `app::runtime_refresh`. v1.16.12 moves selected-alert
command copy notices into `app::clipboard_actions`. v1.16.13 moves
target-picker open/key/mouse dispatch into `app::target_picker`.
v1.16.14 moves dashboard Alerts/Panes selection key dispatch into
`app::dashboard_state`. v1.16.15 moves dashboard mouse dispatch into
`app::dashboard_state`. v1.16.16 moves default config-path resolution and
its tests into `app::path_resolution`. v1.16.17 moves initial target
selection and its tests into `app::target_picker`. v1.16.18 moves the
dashboard frame and overlay render composition into `app::dashboard_render`.
v1.16.19 moves raw-mode, alternate-screen, and mouse-capture terminal
lifecycle helpers into `app::terminal_session`. v1.16.20 moves one poll
tick's success/failure notice routing and pane-state flash updates into
`app::polling_tick`. v1.16.21 moves dashboard notices/reports,
list-selection, and alert freshness resync bookkeeping into
`app::dashboard_runtime`. v1.16.22 moves startup config/root, audit
sink, pricing, Claude settings, retention, and version snapshot assembly
into `app::startup`. v1.16.23 moves target-picker runtime state
ownership into `app::target_picker`. v1.16.24 moves the live TUI event
loop into `app::tui_loop`, leaving `main.rs` as the thin CLI/startup/
`--once`/TUI-entry wrapper.
v1.16.25 starts Phase C C2 by adding `tmux::ControlModeSource`, an
opt-in `[tmux] source = "control_mode"` transport that runs the same raw
tmux commands behind the existing `PaneSource` contract while keeping
`polling` available as an explicit transport.
v1.16.26 adds one-shot reconnect on control-mode transport lifecycle
errors (`%exit`, EOF, broken pipe) and explicitly keeps command-level tmux
errors as caller-visible failures.
v1.16.27 extracts `tmux::commands` so polling and control-mode share the
same list-panes, list-windows, current-target, capture-tail, and
send-keys argument builders.
v1.16.28 adds `tmux::parity`, the `qmonster-tmux-parity` helper binary,
and `scripts/check-tmux-source-parity.sh` so the active tmux session can
be checked for polling-vs-control-mode target, pane, metadata, and
optional strict-tail parity before any default-source switch.
v1.16.29 adds target-scoped parity mode (`--all-targets`) so each
discovered tmux window can be compared independently instead of relying
only on all-session aggregation.
v1.16.30 adds `tmux::snapshots` so polling and control-mode share
list-panes row parsing and tail hydration.
v1.16.31 adds `tmux::targets` so current/available window target parsing
and sorting/dedup rules are shared by both transports.
v1.16.32 adds repeated live parity runs (`--repeat`, optional
`--delay-ms`) so the same control-mode client is exercised across
consecutive polling/control-mode comparisons before any default-source
switch.
v1.16.33 extracts `tmux::control_protocol` so control-mode response
block parsing, command-line quoting, and transport-error classification
are testable without the process-owning client.
v1.16.34 splits parity title differences from structural metadata
differences. Live title drift is a warning by default because animated
pane titles can change between sequential polling/control-mode captures;
`--strict-title` restores failure semantics when needed.
v1.16.35 extracts the reconnect decision into a scripted-testable helper
inside `tmux::control_mode`, locking the contract that lifecycle errors
retry once after reconnect while command-level tmux errors do not.
v1.16.36 extracts `tmux::polling_process` so polling shells out to tmux
through one process boundary with centralized stdout/stderr/error mapping.
v1.16.37 renames poll-tick source failure/recovery notices to
`tmux source` so polling and control-mode share the same
operator-facing failure vocabulary.
v1.16.38 adds `scripts/run-qmonster-control-mode-once.sh`, a
temporary-config `--once` launcher for operator control-mode trials that
does not mutate the standard config file.
v1.16.39 prints the active tmux source mode in `--once` startup output,
using the same `polling` / `control_mode` spelling accepted by config.
v1.16.40 moves the `TmuxSource` dispatch enum into `src/tmux/source.rs`,
leaving `tmux/mod.rs` as module wiring and re-exports.
v1.16.41 moves startup tmux source construction into
`src/app/tmux_source.rs`, keeping `startup.rs` focused on runtime assembly.
v1.16.42 moves `tmux -C attach-session` process ownership into
`src/tmux/control_process.rs`, making control-mode's process boundary
match the earlier polling CLI boundary split.
v1.16.43 locks that attach command's argv contract with a unit test so
future refactors cannot silently drift from tmux control-mode attach.
v1.16.44 moves the control-mode command client and reconnect helper into
`src/tmux/control_client.rs`, so `control_mode.rs` owns the PaneSource
adapter shape while process and client responsibilities remain separate.
v1.16.45 moves `TMUX_PANE` current-pane normalization into
`src/tmux/targets.rs`, keeping polling and control-mode aligned on the
same current-target environment gate.
v1.16.46 tightens the temporary-config control-mode once helper so it owns
`--config`/`--once` explicitly and only passes through `--root`/`--set`.
v1.16.47 tightens the control-mode attach argv to
`tmux -C attach-session -f ignore-size,no-output`, keeping the hidden
control client from influencing pane sizing or receiving unused pane output.
v1.16.48 decorates initial control-mode attach failures with exited child
status and stderr when available, improving diagnosis for unsupported
client flags, missing sessions, and startup diagnostics.
v1.16.49 adds a legacy `tmux -C attach-session` fallback when the
preferred `ignore-size,no-output` client flags are rejected, keeping the
non-invasive attach path first while preserving opt-in control-mode
compatibility on older tmux versions.
v1.16.50 completes Phase C C2 by making `[tmux] source = "auto"` the
default. Auto mode attaches control-mode first and falls back to polling
only when startup attach fails; explicit `control_mode` remains strict.
v1.16.51 updates provider pressure semantics without changing the
PaneSource contract: Claude CTX is read from `/context`, Claude quota is
split from `/usage` Current session (5h) and Current week (all models),
Codex quota is split from bottom-status `5h` and `weekly`, and the
settings overlay persists separate 5h/weekly quota thresholds for Claude
and Codex.
The invariant that matters is boundary purity: provider parsing stays in
`adapters/`, policy stays pure, storage stays out of `ui/`, and tmux
stays unaware of provider semantics.

## Module responsibilities

### `app/`

Owns startup, config load (with safety-precedence enforcement — see
"Safety precedence" below), polling cadence, event loop, shutdown.
Wires the other modules. Holds the top-level recommend-vs-actuate gate
in `effects.rs`. Does NOT contain business rules.

### `domain/`

Pure types, no IO:

- `PaneIdentity = { provider, instance, role, pane_id }`
- `IdentityConfidence = High | Medium | Low | Unknown`
- `ResolvedIdentity = { identity, confidence }`
- `IdentityResolver` — maps `RawPaneSnapshot` → `ResolvedIdentity`;
  canonical `{provider}:{instance}:{role}` titles win, with medium-
  confidence fallbacks for provider title/command hints and the S3-5
  Claude spinner / Gemini `◇  Ready (...)` title patterns. v1.15.20
  also treats structurally anchored provider status surfaces (Codex
  status/welcome box, Gemini status table, Claude status screen,
  Qmonster dashboard tail) as Medium-confidence provider evidence and
  assigns default fallback roles (`Claude|Codex|Gemini => main`,
  `Qmonster => monitor`). Prose-only tail hints remain Low/Unknown.
- `SourceKind = ProviderOfficial | ProjectCanonical | Heuristic | Estimated`
- `MetricValue<T> = { value, source_kind, confidence, provider }`
- `Signal`, `SignalSet`, `TaskType`, `Severity`,
  `Recommendation`, `RequestedEffect`, `AuditEvent`, `Finding`.
- `PaneLifecycle` — transitions (`pane_dead`, session re-attach) that
  drain alerts + reset per-pane pressure state.

### `tmux/`

A single trait `PaneSource` with a polling implementation first.
Control-mode implementation must be drop-in substitutable. Returns
`RawPaneSnapshot`, NOT `PaneSnapshot` — identity is resolved downstream.
Format strings for `list-panes` and `capture-pane` live here. Knows
nothing about providers, roles, or signal semantics.
The `tmux::parity` helper compares two `PaneSource` implementations using
only raw tmux fields, keeping validation inside the same boundary.
The `tmux::snapshots` helper hydrates raw pane rows for both transports
so parsing and tail-fill behavior cannot drift independently.
The `tmux::targets` helper centralizes available-window sorting/dedup
and the current-target first-row contract.
The `tmux::control_protocol` helper owns protocol-only parsing/quoting
for the control-mode client.
The `tmux::control_process` helper owns the control-mode attach argv,
including the `ignore-size,no-output` client flags, legacy attach fallback
support, and attach-failure diagnostic decoration.
The startup tmux source factory owns `auto` mode: preferred control-mode
attach first, polling fallback with an operator-visible startup notice.
The parity helper can repeat checks against one control-mode client to
expose lifecycle regressions after transport changes.
It also keeps volatile pane-title drift out of the default failure path
while leaving strict title checks opt-in.
`[ProviderOfficial: tmux wiki / Formats / Control Mode]` informs the
format strings and lifecycle assumptions.

### `adapters/`

Per-provider tail parsers: `claude.rs`, `codex.rs`, `gemini.rs`,
`qmonster.rs`. Each takes a `ResolvedIdentity` and the raw tail and
returns domain `Signal`s. No identity inference. No cross-provider
logic. No SQLite. No ratatui.

### `policy/`

Pure: consumes `(ResolvedIdentity, SignalSet)` and emits
`(Recommendation | RequestedEffect)[]`. Reads thresholds from config.
Gates `aggressive_mode` behind the `quota_tight` flag AND the
`IdentityConfidence` gate (low-confidence panes suppress
provider-specific recommendations). Every rule attaches a
`SourceKind` to its output. Phase 1 ships `rules/alerts.rs` only;
the A–G canonical situations (log storm / code exploration / context
pressure / verbose output / permission wait / quota-tight /
repeated output) land in Phase 3. Phase 4 adds `rules/profiles.rs` —
a provider-profile recommender that bundles ProviderOfficial CLI
flags / settings / env vars into named `ProjectCanonical` profiles
(e.g. `claude-default`) with per-lever citations, consumed via
`Engine::evaluate` alongside alerts and advisories.
v1.15.22 adds a separate opt-in security posture gate:
`[security] posture_advisories = true` promotes permissive runtime
facts (YOLO, bypass permissions, Full Access, `danger-full-access`,
`no sandbox`) into passive `Severity::Concern` recommendations. The
default is false, so runtime facts remain badge-only unless the
operator explicitly asks for policy surfacing.

### `store/`

- **Phase 1:** `EventSink` trait + `NoopSink` + `InMemorySink`. No
  durable storage, no SQLite, no archive writer.
- **Phase 2:**
  - `sqlite.rs` — audit DB (metadata only).
  - `archive_fs.rs` — raw tail archive with preview/full split.
  - `audit.rs` — audit writer **whose type signature cannot accept
    raw bytes**. Type-level isolation prevents raw tail from bleeding
    into the audit log (Codex CSF-2 + Gemini G-8).
  - `snapshots.rs` — runtime checkpoint writer to
    `~/.qmonster/snapshots/`. Never writes `.mission/CURRENT_STATE.md`.
  - `retention.rs` — retention job (default 14 days, config-driven).

### `ui/`

Ratatui widgets. Current operator surfaces:

1. Severity-first alert queue with timestamps, `NEW` highlighting,
   per-alert auto-hide toggles, and severity bulk-hide chips.
2. Resizable Alerts/Panes dashboard split. Operators can drag the
   divider with the mouse or use `[` / `]`, `/` cycle, and `=` reset
   from the keyboard; the footer shows the current Alerts percentage.
3. Per-pane list with inline expansion for the selected pane's
   recommendations, provider-profile payload, metrics, and runtime
   facts (`modes`, `access`, `loaded`, `restrict`).
4. Alert command ergonomics: recommendation and cross-pane alert
   `suggested_command` values render as `run:` lines; when Alerts are
   focused, `y` copies the selected alert's command to the system
   clipboard and reports missing-command/backend-failure cases as system
   notices. `c` clears system notices.
5. Overlays for target selection (session -> window), help/legend, and
   Git status from the bottom-right version badge.
6. Source labels rendered in long form (`[Official]`, `[Qmonster]`,
   `[Heur]`, `[Estimate]`) rather than two-letter abbreviations.

Palette: low-saturation, grey/navy/blue. Color only on state
transitions, always paired with a numeric % or severity letter.
UI consumes already-classified signals; it never re-parses tails.
Pane state transitions include text-backed visibility cues (`CHANGED`,
temporary `▶ ACTIVE`, and `STATE CHANGED`) so selection styling or
terminal color themes cannot hide a transition. Selected and unselected
pane cards use the same state-change content; selection highlight itself
does not encode state-change semantics, and it does not override state
badge foreground/background colors. Selection itself is only the first
line marker (`▶`), not a full-item underline/background pass across
every expanded row. Current idle/wait/limit states also carry persistent
high-contrast title-prefix badges (`IDLE DONE`, `WAIT INPUT`,
`WAIT APPROVAL`, `USAGE LIMIT`, etc.) and persistent state-row markers
(`COMPLETE`, `INPUT NEEDED`, `ACTION REQUIRED`, etc.) so the operator
does not have to catch the 3-second transition pulse to notice a pane
that still needs attention.
Provider runtime facts are produced by adapter-local parsers from
provider status/slash output and readable provider config sources. The
TUI key `u` sends the selected provider's read-only runtime slash
commands with terminal submit (`C-m`, Enter-equivalent), one command per
press when a provider exposes multiple runtime surfaces. Claude cycles
`/status`, `/context`, `/usage`, `/stats`; Codex sends `/status`;
Gemini cycles `/stats session`, `/stats model`, `/stats tools`. Claude
fullscreen surfaces (`/status`, `/context`, `/usage`) are captured
before Qmonster sends `Escape` to close them; the captured tail is
consumed once as an in-memory parser overlay on the next poll. Claude
also gets a defensive `Escape` before each cycled runtime command so any
prior fullscreen surface is closed before the next slash command is
submitted. Gemini stats surfaces are cycled without a pre-`Escape`.
Claude `/btw` is not used as a runtime fact source because it has no
tool or internal-state access. Unknown or unexposed fields stay absent
rather than inferred.

Pressure metrics intentionally mirror provider surfaces:

- `context_pressure`: Claude `/context`, Codex bottom status, Gemini
  status table `context`.
- `quota_pressure`: provider exposes only one quota surface, currently
  Gemini.
- `quota_5h_pressure`: Claude `/usage` Current session; Codex bottom
  status `5h`.
- `quota_weekly_pressure`: Claude `/usage` Current week (all models);
  Codex bottom status `weekly`.

The policy layer keeps these windows distinct so settings and advisory
actions can pace a rolling 5-hour budget without hiding an exhausted
weekly budget.

### `notify/`

Desktop notification (`notify-send` or equivalent) + optional terminal
bell. Severity-aware rate limiting so log storms do not spam.

## Cross-cutting rules

### Actuation policy (enforced at `app::EffectRunner`)

- `observe_only` — no outbound actions at all.
- `recommend_only` — **default**. Allowed auto actions: notifications,
  runtime-local archive writes under the resolved Qmonster root, and
  display-layer prompt-send proposals. Disallowed: unconfirmed prompt
  send, `/compact`, `/clear`, memory mutation, provider
  reconfiguration, any destructive mutation.
- `safe_auto` — accepted as a non-`observe_only` mode, but it does not
  create autonomous destructive behavior. Real prompt send still
  requires the operator `p` confirmation path plus
  `allow_auto_prompt_send = true`.

### Safety precedence (resolves the r1 contradiction)

Current runtime config loading is explicit via `--config PATH`, or via
the standard default path `~/.qmonster/config/qmonster.toml` when that
file exists. If neither exists, in-memory defaults are used while the
settings overlay still writes to the standard path. Safer-only runtime
overrides may be passed with `--set KEY=VALUE`. Storage-root resolution has its own implemented
precedence: `QMONSTER_ROOT > --root > config.storage.root > default
(~/.qmonster/)`.

Asymmetry for these four flags — env/CLI may only move them TOWARD
safer behavior; any attempt to move them toward more permissive is
ignored and logged as a `risk`-severity audit event:

- `actions.mode` (safer: `observe_only` > `recommend_only` > `safe_auto`)
- `allow_auto_prompt_send` (safer: `false`)
- `allow_destructive_actions` (safer: `false`)
- `refresh.policy` (safer: `manual_only`)

Runtime code does NOT toggle these flags; they are set at startup by
config + safer-only env/CLI overrides.

### Data-shape rule

- Config is TOML (static, stable). The runtime-parsed subset is the one
  defined in `src/app/config.rs`; `config/qmonster.example.toml`
  documents only those live keys.
- Runtime state is in-memory structs + (Phase 2+) SQLite. UI must
  never treat runtime state as config or vice versa.

### Storage split

- `qmonster.db` = `<qmonster-root>/qmonster.db` (Phase 2+; audit
  metadata - indices + summaries). **Never contains raw tail bytes.**
- `archive_dir = <qmonster-root>/archive/` (Phase 2+; raw tails with
  preview/full split).
- `snapshot_dir = <qmonster-root>/snapshots/` (Phase 2+; runtime
  checkpoints before a user-requested compact). **Never overlaps with
  `.mission/CURRENT_STATE.md`.**
- `.mission/CURRENT_STATE.md` is a **day-end handoff document** written
  by the human day-end routine (`docs/ai/WORKFLOWS.md` §3). Qmonster
  runtime never writes it.

### Filesystem write boundary

Qmonster runtime writes only within the resolved Qmonster root
(default `~/.qmonster/`, or the configured `QMONSTER_ROOT` / `--root`
/ `storage.root`). Project-directory files (`CURRENT_STATE.md`,
`mission.yaml`, `mission-history.yaml`, `docs/ai/*`, provider config
files, source files) are read-only to Qmonster across Phases 1–5. Any
runtime write elsewhere is a bug.

### Abstraction boundaries (do not violate)

- `tmux/` knows nothing about providers, roles, or signals.
- `domain/` is pure, no IO.
- `adapters/` know nothing about SQLite, ratatui, or identity
  inference.
- `policy/` is pure; returns values only. No IO.
- `store/` knows nothing about ratatui; `ui/` knows nothing about
  storage shape.
- `audit.rs` cannot accept raw bytes — type-level enforcement, not
  comment-level.

## SourceKind taxonomy

Every metric, threshold, and recommendation carries a `SourceKind`:

| SourceKind         | Meaning                                                                                                               |
| ------------------ | --------------------------------------------------------------------------------------------------------------------- |
| `ProviderOfficial` | Cited to a provider/tool's own documentation (Anthropic / OpenAI / Google / tmux).                                    |
| `ProjectCanonical` | Qmonster project-local rule in `docs/ai/` or `config/qmonster.example.toml`.                                          |
| `Heuristic`        | Community-tool derived (RTK, Context Mode, Token Savior, Caveman, claude-token-efficient, token-optimizer-mcp, etc.). |
| `Estimated`        | Qmonster-derived inference or default chosen without external citation.                                               |

Promotion rule: nothing promotes to `ProviderOfficial` without a direct
provider-doc citation. Heuristic thresholds stay `Estimated` until
Phase-1 fixture measurement justifies re-labeling.

Display rule: every UI surface shows the `SourceKind` next to the
value. Color is never used alone.

## Token-optimization architecture (five layers)

1. **Provider-native profile** (Phase 4 implementation). Pick the right
   CLI flags/settings per `(provider, role, situation)` using
   `ProviderOfficial` levers (Claude/Codex/Gemini). Profile names are
   project-local (`ProjectCanonical`).
2. **Observation** (Phase 1). Polling pane tail, provider parsers,
   repeated-output / log-storm / verbose-answer / context-pressure /
   token / cost signal extraction, plus provider runtime facts for
   permission mode, auto/yolo mode, sandbox, allowed directories,
   loaded tools/skills/plugins, and restricted tools when exposed by
   provider status/config sources. **Phase 1 surfaces these as
   display-only metrics/facts (each with `SourceKind`), not as gating
   signals.**
3. **Archive + checkpoint** (Phase 2). Raw tails → archive;
   preview/full split on screen; runtime snapshots pre-compact →
   `<qmonster-root>/snapshots/`. Never `.mission/CURRENT_STATE.md`.
4. **Policy + recommendation** (Phase 1 = alerts only; Phase 3 =
   A–G situations; Phase 4 = profile recommendations). Aggressive mode
   gated by `quota_tight` flag.
5. **Limited actuation** (see "Actuation policy"). Destructive code
   paths are not created until the phase that owns them is approved.

## MVP reference code — warning

`.docs/init/qmonster_adaptive_token_optimizer_mvp.rs` is a **signal
catalog** (useful) and a **prototype-level reference** (NOT an
architecture template). In particular:

- its `collect_snapshots()` fills `provider`/`role` inside the
  tmux-facing code — violates "`tmux/` knows no providers".
- its `recommend()` mixes provider-specific profile logic into the
  rules — violates "`policy/` is pure".

Copy the markers and parse functions when needed. Do NOT mirror its
module shape.

## Deferred for later phases

- Control-mode tmux adapter.
- Subagent token accounting (Phase 1 ships a detection-only warning).
- Cross-window / cross-project correlation.
- Anomaly detection on pane identity drift (Phase 1 logs transitions).
- Concurrent-work warning across panes (v1.15.23 requires
  `same current_path + same git_branch`; file-level detection remains
  deferred until providers expose a trustworthy active-file signal).
- Review-tier profiles to restore the intended 3×3 provider/profile
  grid.
