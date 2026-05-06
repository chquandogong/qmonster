# ARCHITECTURE

- Version: v0.4.0
- Date: 2026-04-20 (round r2 reconciled) / 2026-04-30 (implementation sync through v1.37.0: F-8 file-level multi-pane orchestration, F-9 opt-in dynamic profile switching, F-9b SQLite USD cost budget tracking with 80% / 100% alerts, and Phase 6 Team Mode layered configuration. v1.36.x remains the release-pipeline hardening base: scripts/release extraction, SBOM SPDX root-doc filter, cargo audit RUSTSEC gates, npm publish --dry-run, and sbom-diff.js --count / --count-purl modes) / 2026-05-04 (v1.37.0 introduces F-8 / F-9 / F-9b and Phase 6 Team Mode layered configuration. v1.38.0 ships the operator-facing UX bundle (Metrics overlay, pane card wrap fix, Action Explainer modal, toggle-on-entry-key + [x]). v1.39.0 polishes the metrics overlay into per-pane cards with severity-colored bars, real per-poll MEM trend tracking, eta day formatting, plus an Action Explainer hardening pass (mouse guard, snapshot dispatch, proposal_id audit) and a Codex App Server v0.128 JSON-RPC compatibility fix.) / 2026-05-06 (v1.40.0 lifts the m and a overlays from fixed-geometry surfaces to operator-controlled. m overlay gains drag-to-move on the title row with asymmetric clamp (left/top hard, right/bottom ≥ 4 cells horizontal / ≥ 1 row vertical soft) and `=` reset of size + position. The a overlay (Pending Actions) is rebuilt as a left/right split with a live Action Explainer panel for the cursor item, multi-select via Space/P/Y/A/c with `BTreeSet<String>` stable-key tracking and auto-prune, in-place p/d/y bulk dispatch routed per-item through the existing `confirm_pending_action(...)` path (no new audit event types), notice-routing parity via `dashboard.push_notice(notice, now)`, and full geometry controls — `[/]/=`, `,/.`, title-drag, separator-drag — mirroring the m overlay. UI_MANUAL §8.5 + §8.7 + help overlay updated.) / 2026-05-06 (current — v1.41.0 is a minor polish release on top of v1.40.0 that hardens two operator-reported bugs in the v1.40 a overlay and elevates the `confirm_actions` bypass to a first-class operator-facing surface. `prune_to()` now clamps `selected` against `items.len()` so polling-driven shrinks no longer leave the cursor stale; `,` / `.` list-width keys take the current effective list width as an argument so first-press direction is correct (`handle_pending_actions_overlay_key` gains a `viewport: Rect` param). Three discoverability surfaces warn about the a overlay's intentional `confirm_actions` bypass: `UI_MANUAL` §8.7 ⚠ caution, in-modal `(confirm_actions bypass)` hint chip, and a `PendingActionsOverlay::seen_first_open` flag that fires a Concern-severity `SystemNotice` on the first `a` press per session. Plus a doc/git consistency audit at the v1.40.0 baseline.)
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
  app/         # bootstrap/startup, layered config+safety-precedence, path resolution, event loop, tui-loop/dashboard-runtime/polling-tick/terminal-session/dashboard-render/keymap/target-picker/runtime-refresh/dashboard-state/modal/settings/operator-action/once-output/prompt-send/clipboard helpers, effect gate
  domain/      # pure types: identity, origin, signal, recommendation, audit, lifecycle
  tmux/        # PaneSource trait; auto source prefers control-mode with polling fallback
  adapters/    # per-provider tail parsers (no identity inference)
  policy/      # pure rules; Phase 1 = rules/alerts.rs;
               # Phase 3 adds rules/{advisories,concurrent}.rs;
               # Phase 4 adds rules/profiles.rs (provider-profile recommender)
  store/       # Phase 1: EventSink trait + NoopSink/InMemorySink
               # Phase 2+: sqlite, archive_fs, audit (type-level raw split), snapshots, retention
               # Phase F/6+: token_usage_samples and cost_usage_events/cost_budget_alerts
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
v1.16.51 originally split provider pressure semantics into context,
5-hour quota, and weekly quota windows. Codex still sources its split
quota values from bottom-status `5h` and `weekly`; v1.17.1 makes Claude
source the live values from statusline `CTX`, `5h`, and `7d` instead of
slash-command runtime captures. v1.21.3 treats Claude statusline
`CTX —` after `/clear` as a visible 0% reset so the pressure cache does
not replay the pre-clear CTX.
v1.16.55 completes Phase C C3 by adding `codex-review` and
`gemini-policy-review` profile recommendations in `policy/rules/profiles.rs`.
They fire only for healthy `Role::Review` panes at medium-or-higher
identity confidence, carry structured `ProviderProfile` payloads with
source-labeled levers, and stay independent of the main-pane
quota_tight baseline/aggressive switch.
v1.16.56 narrows the `code-exploration` advisory so output volume alone
does not trigger the graph/symbol recommendation on healthy Main panes.
v1.16.57 protects active provider work: Gemini `thinking...` progress
markers suppress stale-IDLE fallback, and Claude active-pane runtime
refresh stops sending defensive pre-`Escape` into in-flight Claude work.
v1.16.58 keeps Gemini live-prompt panes active while recent tail history
is still changing. v1.17.1 replaces Claude slash-command refresh with
statusline parsing: Claude `CTX`, `5h`, `7d`, model, effort, path, and
permission mode are read from the live tail, and `u` no longer sends
Claude provider input.
v1.18.0 closes the long-deferred Phase D D2 identity-drift detection.
`policy/rules/identity_drift.rs` is a pure rule that compares each
pane's stored `IdentitySnapshot { provider, current_path }` against
the just-resolved identity and returns up to two `DriftFinding`s
(provider drift, worktree drift) with stable dedup keys. The event
loop (`app/event_loop.rs`) owns the per-session history map
(`Context.identity_history`) and the dedup hashset
(`Context.reported_drifts`); both are evicted on
`PaneLifecycleEvent::{BecameDead, Reappeared}` so a re-spawned pane
can fire its own drifts. The rule is gated behind
`PolicyGates.identity_drift_findings` (operator opts in via
`[security] identity_drift_findings = true`); default off.
v1.19.0 closes Phase D D3 honestly: subagent detection adds Claude
Code's `● Task(` tail signature while leaving per-subagent token
attribution permanently deferred until providers expose structured
per-subagent counters. The same slice hardens Codex statusline parsing
for current `model-with-reasoning` output: if effort appears only in a
trailing item such as `gpt-5.5 xhigh`, the Codex adapter still
populates `model_name = gpt-5.5` and `reasoning_effort = xhigh`.
The invariant that matters is boundary purity: provider parsing stays in
`adapters/`, policy stays pure, storage stays out of `ui/`, and tmux
stays unaware of provider semantics.
v1.22.0 opens **Phase F** with **F-1 process RAM**: tmux `#{pane_pid}` is
captured as the 9th `PANE_LIST_FORMAT` field, and a new
`adapters::process_memory` helper walks `/proc/<pid>/task/<pid>/children`
recursively (depth-capped at 5 with a visited-set against
diamond/cycle re-reads) to surface the highest-RSS descendant's RSS as
`SignalSet.process_memory_mb` for Claude/Codex panes via a new
`#[doc(hidden)] pub` `parse_for_with_proc_root` test seam in the adapter
dispatcher. Gemini's status-table `[Official]` path is preserved
untouched (the dispatcher fills `/proc` descendant RSS only when the
provider adapter left `process_memory_mb` `None` and `pane_pid.is_some()`).
SourceKind for the new path is `Heuristic` because `/proc` RSS is an OS
observation, not a provider-published number.
v1.23.0 continues **Phase F** with **F-2 agent memory file scan + bloat advisory**:
new `adapters::agent_memory::read_agent_memory_bytes_with_filesystem` discovers
provider-specific memory files (per-provider candidate lists; Claude includes
`~/.claude/projects/<encoded>/memory/*.md`), sums byte sizes capped per file at
1 MiB, and returns `Option<u64>`. `ParserContext.current_path: &str` is threaded
through; `parse_for_with_proc_root` is preserved as a backward-compat shim that
calls a new `parse_for_with_environment(ctx, proc_root, home_dir)` seam — single
dispatch responsible for both F-1 process RAM and F-2 agent memory fills.
`SignalSet.agent_memory_bytes: Option<MetricValue<u64>>` carries the result,
always `Heuristic` (file existence ≠ load confirmation). UI renders
`MEM-FILE <KB|MB> [Heur]` after the F-1 MEM badge; sub-1 KiB renders as
`<1 KB`. New policy rule `policy::rules::agent_memory::eval_agent_memory` fires
`Severity::Concern` `ProjectCanonical` advisory above 50_000 bytes
(`BLOAT_THRESHOLD_BYTES`) with `IdentityConfidence ≥ Medium` and no input /
permission wait; next-step routes the operator to per-task skill / override
files. Threshold is hard-coded for v1; operator-configurable threshold
deferred to a later slice.

v1.24.0 continues **Phase F** with **F-3 token usage time series + sparkline UI**:
a new `token_usage_samples` table is added to the shared `qmonster.db`
(alongside `audit_events`); `AuditDb::open` applies both schemas at
construction. `store::token_usage::SqliteTokenUsageSink` provides
`record_sample` (write one row per pane per poll when any token field
is populated) and `recent_samples(pane_id, limit)` (newest first via
`idx_token_usage_pane_ts`). The sink carries an `error_count: AtomicU64`
mirroring `SqliteAuditSink` for durability observability. The event
loop wires the new sink alongside the existing audit sink at startup;
`PaneReport.recent_token_samples: Vec<TokenSample>` is populated for
every live pane every iteration (one indexed `SELECT LIMIT 20` per
pane per poll, sub-ms cost). UI renders `TOKENS ▁▂▃▄▅▆▇█` from
`input_tokens` deltas on the expanded (selected) pane card; fewer
than 2 samples → no sparkline. Retention sweep extends with
`DELETE FROM token_usage_samples WHERE ts_unix_ms < (now -
max_age_days*86_400_000)`; zero retention is a no-op (consistent
with archive sweep). Provider native cache_read fields will populate
the sparkline once F-4 / F-5 / F-6 land; F-3 ships the persistence +
render infrastructure today.

v1.25.0 continues **Phase F** with **F-4 Codex cached_input_tokens + CACHE hit ratio badge**:
new `parse_codex_cached_input_tokens` in `src/adapters/codex.rs` extracts the
`(+ N cached)` token from the Codex `/status` welcome panel's `Token usage:`
line and populates `SignalSet.cached_input_tokens: Option<MetricValue<u64>>`
with `SourceKind::ProviderOfficial`. The `token_usage_samples` table gains a
nullable `cached_input_tokens INTEGER` column; `AuditDb::open` runs an
idempotent `ALTER TABLE … ADD COLUMN` migration that swallows "duplicate
column" errors so v1.24.0 DBs upgrade in place. `TokenSample.cached_input_tokens`
round-trips through INSERT/SELECT. The event-loop sampling predicate now
records cache-only rows. UI renders `cache_hit_ratio = cached / (input +
cached)` as a `cache <%>` text field or `CACHE <%>` badge with one-decimal
precision; honesty rule preserved when `cached_input_tokens` is None.
Provider coverage today: Codex (welcome panel, ProviderOfficial). Deferred
siblings: F-4b (Gemini /stats parsing), F-5 (Claude statusLine command),
F-6 (Codex App Server resetsAt), F-7 (cache-aware policy rules using the
time series).

v1.26.0 continues **Phase F** with **F-7 cache-aware advisory rules**:
two new rules in `src/policy/rules/cache.rs` turn F-4's
`cached_input_tokens` data into actionable `/compact` decisions.
`recommend_cache_hot_compact_warning` (Severity::Concern,
SourceKind::ProjectCanonical) fires when `cache_hit_ratio > 60%`
AND `context_pressure < 70%`, advising the operator NOT to
compact (compact resets cache; let context fill further).
`recommend_compact_when_cache_cold` (Severity::Good,
SourceKind::ProjectCanonical, suggested_command: Some("/compact"))
fires when `cache_hit_ratio < 30%` AND `context_pressure > 60%`,
advising a snapshot-first `/compact` (cache rebuild cost is already
paid on every turn). Both gate on `IdentityConfidence ≥ Medium`
and suppress on input/permission wait. Mutual exclusion is
structural: hot needs ratio > 0.6, cold needs ratio < 0.3 —
strictly disjoint regions, intermediate 30-60% band triggers
neither. `Engine::evaluate` dispatches `eval_cache` after
`eval_agent_memory`. Thresholds hard-coded for v1; operator-tunable
thresholds deferred. Deferred siblings: F-4b (Gemini /stats),
F-5 (Claude statusLine), F-6 (Codex App Server resetsAt),
F-7b (cache_drift_detected via recent_token_samples), F-7c
(wait_for_reset / snapshot_before_reset — depend on F-5/F-6
reset_eta).

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
commands with terminal submit (`C-m`, Enter-equivalent) for Codex and
Gemini only. Claude is statusline-only: pressing `u` on a Claude pane
forces the next poll but sends no slash command and no `Escape`. Codex
sends `/status`; Gemini cycles `/model`, `/stats session`,
`/stats model` only when the pane is idle/stale/limit-hit. While
Gemini is active, Qmonster skips `/model` and cycles only
`/stats session`, `/stats model`.
Gemini `/stats ...` surfaces are cycled without a pre-`Escape`; Gemini
`/model` is treated as a picker surface, so Qmonster captures the tail
and then sends one `Escape` to close it before the next `u` cycle
command. Gemini v0.41 model usage rows render `Resets:` rather than
`Reset:`, and both forms are parsed. When the live status table exposes
the current `/model` value, Qmonster filters picker reset rows to that
model family only (`gemini-3.1-pro-preview` maps to the `Pro` row, not
`Flash` or `Flash Lite`). Unlike one-shot informational runtime
overlays, Gemini `/model` reset captures persist until the next `/model`
capture or pane lifecycle reset because closing the picker removes the
reset rows from live scrollback. `/stats tools` is not sent because no
current Gemini parser consumes it.
Gemini
`thinking...` progress markers and recently changing live-prompt tails
suppress idle classification so a genuinely working Gemini pane does not
display as `IDLE`.
Claude `/btw` is not used as a runtime fact source because it has no
tool or internal-state access. Unknown or unexposed fields stay absent
rather than inferred.

Pressure metrics intentionally mirror provider surfaces:

- `context_pressure`: Claude statusline `CTX`, Codex bottom status,
  Gemini status table `context`. Historical Claude `/context` capture
  parsing remains for old overlays, but the live path is statusline.
- `quota_pressure`: provider exposes only one quota surface, currently
  Gemini.
- `quota_5h_pressure`: Claude statusline `5h`; Codex bottom status `5h`
  converted from remaining quota to pressure.
- `quota_weekly_pressure`: Claude statusline `7d`; Codex bottom status
  `weekly` converted from remaining quota to pressure.

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
file exists. If a sibling `qmonster.local.toml` exists next to the
loaded file, Phase 6 Team Mode recursively merges that local override
after the shared `qmonster.toml`. If neither exists, in-memory defaults
are used while the settings overlay still writes to the standard path.
Safer-only runtime overrides may be passed with `--set KEY=VALUE`.
Storage-root resolution has its own implemented precedence:
`QMONSTER_ROOT > --root > config.storage.root > default (~/.qmonster/)`.

Asymmetry for these four flags — env/CLI may only move them TOWARD
safer behavior; any attempt to move them toward more permissive is
ignored and logged as a `risk`-severity audit event:

- `actions.mode` (safer: `observe_only` > `recommend_only` > `safe_auto`)
- `allow_auto_prompt_send` (safer: `false`)
- `allow_destructive_actions` (safer: `false`)
- `refresh.policy` (safer: `manual_only`)

Runtime code does NOT toggle these flags; they are set at startup by
config + safer-only env/CLI overrides.

### Layered configuration (Phase 6 Team Mode)

The configuration model is now a three-layer TOML merge:

1. Serialized `QmonsterConfig::defaults()` provides a complete base
   shape for every live config section.
2. The shared config file (`--config PATH`, or
   `~/.qmonster/config/qmonster.toml` when present) overlays those
   defaults.
3. A sibling `qmonster.local.toml` overlays the shared config when it
   exists.

`app::config::load_layered_from_paths(base, local)` owns the recursive
merge. Table values merge key-by-key, so local files can be sparse:

```toml
[cost]
budget_usd = 200.0

[cost.claude]
warning_usd = 15.0
```

Scalar or array values replace the lower layer. The local file path is
derived with `local_override_path_for(path)` by replacing the base file
name with `qmonster.local.toml`; Qmonster does not search parent
directories or arbitrary include paths. `load_with_local_override(path)`
is the startup-facing helper. If the local file exists, settings writes
target that local path so machine-local choices do not overwrite shared
team defaults.

The git contract is explicit: shared `qmonster.toml` may be tracked;
`qmonster.local.toml` and `config/qmonster.local.toml` are ignored.
Local overrides are still subject to the same startup safety-precedence
rule as the merged config: env/CLI can only move the four safety flags
toward safer behavior.

### Data-shape rule

- Config is TOML (static, stable). The runtime-parsed subset is the one
  defined in `src/app/config.rs`; `config/qmonster.example.toml`
  documents only those live keys.
- Runtime state is in-memory structs + (Phase 2+) SQLite. UI must
  never treat runtime state as config or vice versa.

### Storage split

- `qmonster.db` = `<qmonster-root>/qmonster.db` (Phase 2+; audit
  metadata - indices + summaries). **Never contains raw tail bytes.**
  Phase F stores token samples in `token_usage_samples`; Phase 6 cost
  tracking stores normalized USD deltas in `cost_usage_events` and
  one-shot 80% / 100% budget claims in `cost_budget_alerts`; all are
  metadata-only.
- `archive_dir = <qmonster-root>/archive/` (Phase 2+; raw tails with
  preview/full split).
- `snapshot_dir = <qmonster-root>/snapshots/` (Phase 2+; runtime
  checkpoints before a user-requested compact). **Never overlaps with
  `.mission/CURRENT_STATE.md`.**
- `.mission/CURRENT_STATE.md` is a **day-end handoff document** written
  by the human day-end routine (`docs/ai/WORKFLOWS.md` §3). Qmonster
  runtime never writes it.

### Cost tracking and budget alerts (F-9b)

Provider cost signals are cumulative session values, not deltas.
`SignalSet.cost_usd` therefore has two storage paths with different
contracts:

- `token_usage_samples.cost_usd` keeps the raw per-poll cumulative value
  next to token samples for graphing and historical inspection.
- `cost_usage_events.cost_usd_delta` stores only the positive delta that
  should count against a budget.

`store::cost_usage::SqliteCostUsageSink` is the only writer for the
budget ledger. For each `CostObservation { pane_id, provider,
cumulative_cost_usd }`, it looks up the previous cumulative value for
that pane ordered by `(ts_unix_ms DESC, id DESC)`:

- first sample: count the cumulative value as the initial session spend;
- higher sample: count `current - previous`;
- equal sample or microscopic delta: record nothing to avoid
  double-counting repeated polls;
- lower sample: treat it as a provider/session reset and count the new
  cumulative value as a fresh session.

Budget claims are separate from usage rows. `claim_budget_alerts(cost,
now)` sums `cost_usage_events`, compares the total against
`[cost] budget_usd`, and inserts into `cost_budget_alerts` with primary
key `(threshold_bps, budget_cents)`. The key intentionally makes 80%
and 100% alerts one-shot for a specific configured budget value while
allowing a changed budget to establish a new alert ledger. The live
thresholds are:

- 80% (`threshold_bps = 8000`) -> Warning recommendation;
- 100% (`threshold_bps = 10000`) -> Risk recommendation and strong
  budget-exhausted signal.

`CostConfig::budget_usd` defaults to `200.0`, matching the operator
budget for v1.37.0. `budget_enabled()` requires a finite positive value;
`budget_usd = 0.0` disables budget alerts without disabling the existing
per-poll token/cost samples. Event-loop errors in the cost sink are
logged to stderr and do not crash polling, matching the durable-storage
best-effort pattern used by the audit and token sinks.

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

- Subagent token attribution (per-subagent input/output token split).
  v1.19.0 (Phase D D3-A) refines the _detection_ surface — `● task(`
  joins the existing phrase markers so Claude Code's Task-tool tail
  signature fires `subagent_hint` while ordinary tool calls
  (`● Bash(...)`, `● Read(...)`) and TODO-list prose
  (`Task 1 — capture fixtures`) stay silent. Per-subagent **token
  attribution** stays permanently deferred (Phase D D3-C honesty
  note): none of Claude / Codex / Gemini exposes per-subagent
  input/output token counters today. The cumulative session token
  totals already include subagent work, so any "Sub" split would be
  a delta-window guess that conflates with main-agent activity. The
  same anti-pattern v1.13.0 already removed for log_storm /
  verbose_answer fallbacks. Re-open if a future provider release
  lands a structured per-subagent counter.
- Cross-project correlation across tmux windows that hold genuinely
  different projects (v1.17.0 ships within-project cross-window
  detection only — same `current_path` + `git_branch` across 2+ tmux
  windows fires `CrossWindowConcurrentWork` behind the
  `[security] cross_window_findings` opt-in gate).
- Concurrent-work warning across panes (v1.15.23 requires
  `same current_path + same git_branch`; file-level detection remains
  deferred until providers expose a trustworthy active-file signal).

v1.35.0 ships **Phase F F-7d operator-tunable `[reset]`
thresholds** for the F-7c reset-aware advisories. Mirrors the
F-7-config (cache thresholds) refactor pattern from v1.28.0: lift
hardcoded constants out of the rule module into a new config
struct, thread the values through `PolicyGates`, and have the
rule read from gates instead of module constants. New
`ResetConfig` struct in `src/app/config.rs` with four
`#[serde(default)]` fields and per-field defaults matching the
v1.34.0 (F-7c) hardcoded constants exactly:
`wait_pressure_threshold = 0.85`, `wait_eta_secs = 1800`,
`snapshot_pressure_threshold = 0.50`, `snapshot_eta_secs = 300`.
`QmonsterConfig` adds `reset: ResetConfig` (also
`#[serde(default)]`) so existing configs without a `[reset]`
section keep working unchanged. `PolicyGates` gains four
`reset_*` fields populated from `ResetConfig` in
`from_config_and_identity` (now 9th positional param;
`#[allow(clippy::too_many_arguments)]` applied narrowly the same
way F-7-config did at the 8th param boundary). `eval_reset` and
its two helpers (`recommend_wait_for_reset`,
`recommend_snapshot_before_reset`) take `&PolicyGates` and read
thresholds from `gates.reset_*` instead of the removed module
constants; reason strings interpolate
`gates.reset_wait_pressure` so an operator who sets
`wait_pressure_threshold = 0.75` sees the recommendation mention
75% rather than the original 85%. `event_loop::run_once_with_target`
passes `&ctx.config.reset` to `from_config_and_identity`; the 7
existing `gates.rs` test call sites updated with the new arg via
`&crate::app::config::ResetConfig::default()`. Why threshold
tuning matters: operator workflows differ — some operators prefer
earlier wait advice when their provider quota is precious; others
want a longer snapshot lead-time so the runtime snapshot completes
before the actual reset boundary. F-7d makes those preferences
first-class config surface without changing default behavior, so
operators who never edit `qmonster.toml` see no v1.34.0
regression. With F-7d shipped, the entire F-stack (F-1 → F-7d) is
now under operator control where appropriate (cache thresholds via
`[cache]`, reset thresholds via `[reset]`); the rest is
observation/wiring without operator knobs. Settings overlay (`S`)
shows `[reset]` values in `Parameters` and the reset rule conditions
in `Rules`; editing remains direct `qmonster.toml` for the threshold
sections. The `Badges` tab documents source labels and metric meanings
so the operator can decode `CTX`, `COST`, `TOKENS`, `CACHE`, `RESET`,
and `CALLS` without leaving the TUI. Tests grew to 771+ lib + 68
integration green in the v1.35.x line.

v1.34.0 ships **Phase F F-7c reset-aware policy rules**
(`wait_for_reset` + `snapshot_before_reset`). New
`src/policy/rules/reset.rs` consumes the F-5b (Claude sidefile) and
F-6 (Codex App Server) `quota_*_resets_at` Unix timestamps to give
the operator concrete pacing advice as a quota window approaches
reset. The orchestrator `eval_reset(id, signals, gates,
now_unix_seconds) -> Vec<Recommendation>` evaluates the 5h and
weekly windows independently — either window can fire either rule
on the same tick. `recommend_wait_for_reset` (Severity::Concern,
ProjectCanonical) fires when `quota_*_pressure >= 0.85` AND the
matching window resets in <= 30 minutes; the reason embeds the
actual percentage, the 85% threshold, and a `2h13m` / `45m` /
`30s` countdown via a local `format_eta_short` helper that mirrors
`panels.rs::format_resets_eta` so the badge and the reason string
agree on wording. next*step tells the operator to stop submitting
prompts until the window resets.
`recommend_snapshot_before_reset` (Severity::Good,
ProjectCanonical) fires when ANY quota window resets in <= 5
minutes AND the corresponding pressure is at least 50% (no point
preserving handoff state if the operator just started); next_step
prompts the `s` key to write a runtime snapshot. Both rules gate
on `IdentityConfidence >= Medium` and suppress when `idle_state`
is `InputWait` or `PermissionWait`, mirroring the existing
project-canonical recommendation conventions. Architecture choice:
`now_unix_seconds` is a parameter rather than a private read inside
`eval_reset` so tests can inject the value directly for
deterministic timing — `Engine::evaluate` reads `SystemTime::now()`
once per call and threads the same instant to `eval_reset`, which
guarantees the 5h and weekly window comparisons are consistent
within a single tick (a real-clock read inside each rule could in
principle straddle a window boundary, even though the practical
risk is tiny). Hardcoded thresholds for v1:
`WAIT_PRESSURE_THRESHOLD=0.85`, `WAIT_ETA_SECS=1800` (30 minutes),
`SNAPSHOT_PRESSURE_THRESHOLD=0.50`, `SNAPSHOT_ETA_SECS=300` (5
minutes). F-7d would expose these via a new `[reset]` config
section once tuning data lands. Provider scope: F-7c is a
Claude+Codex-only feature because only F-5b (Claude sidefile) and
F-6 (Codex App Server) populate `quota*\*\_resets*at`. Gemini `/model`
`Reset:`rows for the current status-table model family are parsed as
display-only`RuntimeFactKind::ModelReset` facts (`RESET <model>
<time/remaining> [Official]`) but do not populate the F-7c
`quota*\*\_resets_at` policy inputs. Bundled small
cleanup:`src/ui/panels.rs` sparkline test refactored from let-mut

- field reassign to struct-update syntax (silences
  `clippy::field_reassign_with_default`). 10 new unit tests with
  deterministic now-injection. Tests grew to 765 lib + 68 integration
  green.

v1.42.0 ships **Phase H opt-in auto-snapshot at reset boundary**.
A new `app::auto_snapshot::maybe_auto_snapshot` hook in
`src/app/auto_snapshot.rs` runs after `policy.evaluate(...)` in
the event loop. When `[reset] auto_snapshot = true` AND the F-7c
`recommend_snapshot_before_reset` advisory fires, the hook writes
one snapshot per `(pane_id, quota_kind, window_id)` where
`window_id = quota_*_resets_at` floored to the nearest minute.
The hook reuses the `SnapshotWriter` and the `SnapshotWritten`
audit kind unchanged; metadata (`trigger=auto_reset_boundary`,
`quota_kind`, `window_id`) is encoded in the audit `summary`
string. Failures push a Warning `SystemNotice` and leave dedup
untouched so the next tick can retry. Restart inside a window
resets in-memory dedup, permitting one extra snapshot for that
window — acceptable: the snapshot is harmless, and persisting
dedup to disk is not worth the schema change in v1. The policy
code, the F-7d thresholds, and the existing dashboard
recommendation flow are all untouched.

v1.37.0 ships four feature slices as the current runtime baseline.
F-8 extends cross-pane orchestration from same-path/branch heuristics
to file-level Claude tool-call observations: when the operator enables
`[security] cross_pane_file_findings`, `Edit` / `Write` /
`MultiEdit`-style markers are resolved against each pane's
`current_path` and can emit a `ConcurrentFileEdit` warning for two
busy Main/Review panes touching the same absolute file. F-9 adds an
opt-in `[profile_switch]` rule that reads a per-pane sliding
`error_hint` history and recommends a script-low-token profile when
the configured error rate is reached. F-9b adds the SQLite USD cost
ledger and budget-alert path described above. Phase 6 Team Mode adds
the layered config loader described above so shared team defaults and
machine-local overrides can coexist without provider reconfiguration or
destructive automation.

v1.33.0 ships **Phase F F-4b Gemini `/stats` output parser**; the
later v1.35.x sync extends the same path to Gemini `/model` reset
rows. New
private parsers in `src/adapters/gemini.rs` close the long-deferred
F-4b slice that was originally listed alongside F-4 in v1.25.0 but
pending a live Ink-rendered fixture. `parse_gemini_stats_session(tail)
-> GeminiSessionStats { session_id, tool_calls }` defensively scans
stripped-box-char lines for `Session ID:` and `Tool Calls:` anchors;
`parse_gemini_stats_model(tail) -> Option<GeminiModelStats {
input_tokens, output_tokens, cache_reads }>` performs section-anchored
scanning for the `Tokens` header followed by `Total` / `Input` /
`Cache Reads` / `Output` rows. `parse_gemini_model_resets(tail,
current_model)` scans the `/model` picker for model-specific `Reset:` /
`Resets:` rows, filters to the current status-table model family when
known, and preserves the provider-rendered reset time plus remaining
duration as display-only runtime facts. Architecture choice:
section-anchored
scanning over plain regex — Gemini's `/stats` output is rendered by
Ink and the box-drawing characters surrounding each row vary across
terminal widths and Ink versions, so the parser strips those
characters first (`strip_box_chars`), then walks lines forward from
each labeled anchor. Helpers `strip_metric_label`, `first_count_in`,
and `parse_count_with_commas` keep the line scan cheap and defensive.
Honesty rule: the model parser returns `Some` ONLY when both
`input_tokens` AND `output_tokens` parse — don't ship half-data.
`GeminiAdapter::parse` integrates both parsers AFTER the existing
status-table block: `RuntimeFactKind::SessionId` (reused from F-5b),
`RuntimeFactKind::ToolCalls`, and `RuntimeFactKind::ModelReset`
populate from the runtime panels (`SID` / `CALLS` / `RESET` badges),
while `input_tokens` / `output_tokens` / `cached_input_tokens` populate
via `is_none()` guards. The is_none()
guard pattern is shared with F-5b (Claude
sidefile) and F-6 (Codex app-server): adapters' enrichment paths
preserve any earlier-surface authority on the same field rather than
overwriting it. OAuth Gemini honesty: `cache_reads` stays `None` per
FAQ-documented Google limit (the `Cache Reads` row is hidden for
OAuth users); Qmonster never synthesizes a zero. Bundled v1.33.x
polish: `src/ui/panels.rs::primary_metric_row` adds `RESET 5H <eta>`
/ `RESET 7D <eta>` badges to the compact one-line view via the same
`format_resets_eta` helper and SourceKind labeling as the verbose
F-5b `metric_row`. 8 new unit tests with synthetic Ink-rendered
fixtures (honesty cases: OAuth → cache_reads=None, panel absent →
defaults). Tests grew to 753 lib + 68 integration green.

v1.32.0 ships **Phase F F-6 Codex App Server JSON-RPC client**. New
`src/adapters/codex_app_server.rs` spawns `codex app-server` as a
child process with `-c sandbox_mode="danger-full-access"` to bypass
bubblewrap on Linux (without the flag, the spawn fails with a sandbox
setup error), pipes its stdin / stdout, and speaks JSON-RPC over
NDJSON. The client first sends `initialize`, then
`account/rateLimits/read`; the parser turns the response into
`CodexRateLimits { primary: Option<CodexRateWindow>, secondary:
Option<CodexRateWindow> }` where each window carries `used_percent`
(clamped 0..=100), `window_duration_mins`, and
`resets_at_unix_seconds`. Architecture choice: a `JsonRpcIo` trait
abstracts the child's IO so tests inject a `VecDequeIo` stub for
fully synchronous in-memory unit testing; `SubprocessIo` is the
production implementation, and its `Drop` closes stdin (graceful EOF)
then runs `try_wait` → `kill` fallback for clean child reap on TUI
shutdown. `Context` gains `codex_app_server: Option<CodexAppServer<SubprocessIo>>`
(long-lived client owned by the app loop) and
`codex_rate_limits: Option<CodexRateLimits>` (cached account-level
snapshot updated once per polling tick). Lifecycle: `tui_loop`
spawns the server once at startup when
`[provider_setup] codex_app_server = true`; spawn success surfaces
as `SystemNotice` (Severity::Good), spawn failure surfaces as
`SystemNotice` (Severity::Warning) — startup never aborts on
app-server failure, so an operator who enabled the toggle but
doesn't have a working `codex app-server` binary still gets the rest
of the TUI. `event_loop::run_once_with_target` calls
`read_rate_limits()` once per polling tick BEFORE iterating panes;
on success it caches the result on `Context.codex_rate_limits`, on
failure it logs to stderr, drops the client, and clears the cache so
stale data can never linger past one transient failure. Account-
level read once-per-tick + per-pane apply: new
`apply_codex_rate_limits(signals, rl)` helper runs inside the
per-pane loop only when `provider == Codex`, writing the cached
account-level snapshot into the pane's SignalSet. Two precedence
rules:`quota_5h_pressure` / `quota_weekly_pressure` populate ONLY
when the statusline path didn't (is_none guards — the existing per-
pane bottom-status authority stays intact when present), but
`quota_5h_resets_at` / `quota_weekly_resets_at` populate
unconditionally (no other surface emits Codex reset timestamps).
Result: F-5b's `5h resets in <eta>` / `7d resets in <eta>` rows
now appear on Codex pane cards too, consistent with the Claude
sidefile path. Tests grew to 726 lib + 68 integration green.

v1.31.0 ships **Phase F F-5b Claude sidefile reader**. The recommended
`~/.claude/statusline.sh` (G-1 snippet) drops a per-session JSON file at
`~/.local/share/ai-cli-status/claude/<session_id>.json` carrying raw
`cache_read_input_tokens` / `input_tokens` / `output_tokens` /
`cache_creation_input_tokens`, cumulative `cost.total_cost_usd`,
`transcript_path`, and Unix `resets_at` timestamps for the 5h and 7-day
rate-limit windows. F-5b reads it. New `src/adapters/claude_sidefile.rs`
defines a `ClaudeSidefile` serde struct + `read_sidefile_for_path(home,
current_path)` matcher that finds the most-recently-modified
`<sid>.json` whose `cwd` field equals the pane's `current_path`
(newest-mtime tiebreak; best-effort silent None on missing dir /
malformed JSON / no match). Architecture choice: post-adapter
enrichment in `parse_for_with_environment` mirroring the Phase F-1
(process memory `/proc` walk) and F-2 (agent-memory file scan)
patterns — adapters stay pure on tail-derived signals, environment-
derived state lands in a single enrichment seam. A provider gate
restricts the enrichment to Claude panes so a Codex / Gemini pane
sharing the cwd never inherits Claude session state. New `SignalSet`
fields `quota_5h_resets_at` and `quota_weekly_resets_at`
(`Option<MetricValue<u64>>`, Unix seconds, `SourceKind::ProviderOfficial`)
feed two new metric rows: `5h resets in <eta>` and `7d resets in <eta>`.
New helper `format_resets_eta` formats `2h13m` / `45m` / `30s` and caps
at 14 days to reject sentinels. New `RuntimeFactKind::{SessionId,
TranscriptPath}` variants surface per-pane Claude session identity
(rendering labels `SID` / `XSCRIPT`). `cost_usd` now populates for
Claude panes via the existing `cost $X.XX` row (was always None on the
statusline-only path). `is_none()` guards preserve the statusline path's
values for raw counts (input / output / cached); the one explicit
override exception is `cache_hit_ratio` — the sidefile-derived precise
count-based ratio (`cache_read_input_tokens / (input_tokens +
cache_read_input_tokens)`) wins over the rounded `cache N%` statusline
value, so F-7 / F-7b cache rules fire on Claude panes with full
fidelity instead of integer-rounded ratios. `cache_creation_input_tokens`
is the prompt-cache write cost and is intentionally **not** folded
into the hit-ratio denominator — it surfaces separately as the
expanded-pane `cache io <read> / <create>` row so cache cost-vs-benefit
analysis can reason about creation cost without conflating it with
read-hit performance.
Tests grew to 710 lib + 68 integration green.

v1.30.0 ships **Phase G G-2 [provider_setup] config + Phase F F-5
Claude statusline cache parser**. G-2 adds operator-tunable Provider
Setup defaults via a new `[provider_setup]` section in `qmonster.toml`:
`claude_sidefile = true` (default — recommended sidefile JSON-export
workflow on at startup; operator can opt out) and `codex_app_server = false`
(default — advanced background daemon, opt-in). New `ProviderSetupConfig`
struct in `src/app/config.rs` mirrors the F-7-config per-section pattern
with `#[serde(default)]` per-field so existing configs keep working
unchanged. New `ProviderSetupOverlay::from_config(&QmonsterConfig)`
constructor seeds the runtime per-tab toggles from persisted config so
a fresh `P` press shows the recommended posture without an extra `s`
keystroke; `tui_loop.rs` uses `from_config` instead of `::new()` at
startup. Sidefile-on-default writes the full Claude statusLine JSON to
`~/.local/share/ai-cli-status/claude/<session_id>.json` so downstream
Claude tooling (the F-5 reader, future audit pipelines) can read raw
`cache_read_input_tokens`, `cache_creation_input_tokens`, `resets_at`,
`cost`, and `transcript_path` without re-parsing the tmux capture.
F-5 wires the other end: the recommended `~/.claude/statusline.sh`
emits an optional `cache N%` token between `CTX` and `5h` whenever the
live session JSON populates `cache_read_input_tokens`; the Claude
adapter parses it into a new
`SignalSet.cache_hit_ratio: Option<MetricValue<f64>>` field
(stored as 0..1, `SourceKind::ProviderOfficial`). New
`ClaudeStatusLine.cache_hit_ratio: Option<f64>` carries the per-line
parse, and a new `find_status_label_in(line, label, start, end)`
range-bounded label search lets `parse_claude_status_line_row` narrow
CTX's percent-search bound to stop at the optional `cache` label so
the cache value cannot bleed into CTX. The existing count-derived
ratio (Codex F-4 path: `cached / (input + cached)`) remains as
fallback; new helpers `policy/rules/cache.rs::cache_hit_ratio(signals)`
and `panels.rs::signals_cache_pct_with_source()` encapsulate the
"prefer direct field, fall back to count-derived" precedence so the
CACHE badge call sites and the F-7/F-7b cache rules stay aligned. The
result: the CACHE badge surfaces for Claude panes alongside Codex, and
the F-7/F-7b cache-aware advisories fire on Claude panes too — a Claude
pane on the recommended statusline.sh now gets the same cache-pressure
signal Codex does. Tests grew to 699 lib + 68 integration green.

v1.29.0 opens **Phase G** with **G-1 Provider Setup overlay**. Phase G is
"operator integration helpers" — a stream distinct from Phase F's cache
observability stack. New module `src/ui/provider_setup.rs` owns: (1) the
view-model (`ProviderSetupTab` enum, `ProviderSetupOverlay` struct with
`open: bool`, `tab`, per-tab toggles, `scroll_offset`), (2) read-only
state detectors that probe `~/.claude/statusline.sh`,
`~/.codex/config.toml`, and `~/.gemini/settings.json`, (3) 7 const
snippet `include_str!` references composed by `render_tab_content`.
`src/app/provider_setup_overlay.rs` mirrors `settings_overlay.rs` for
key dispatch (Esc/q close, 1/2/3/4 switch tabs, Tab/arrow cycle,
↑↓/j/k scroll).
Render plumbed via `dashboard_render.rs::DashboardFrameView` field +
`dashboard.rs::render_provider_setup_modal`. The overlay is read-only —
Qmonster never writes provider config files; the operator copies the
displayed snippet manually. The Claude tab's snippet is the recommended
`statusline.sh` (with cache hit ratio calculation that feeds Qmonster's
F-4 CACHE badge); the Codex tab's snippet enumerates the bottom-status
`/statusline` items and explains the `/status` welcome panel
`(+ N cached)` source for Qmonster's F-4 cache parser; the Gemini tab
shows the `ui.footer.*` JSON template plus an OAuth-stays-informational
note (per FAQ-documented OAuth cache limit). The Tmux tab copies a
bundle installer that writes `~/ts.sh` and
`~/.tmux/qmonster.tmux.conf` for the recommended four-pane workflow.

v1.28.0 continues **Phase F** with **F-7-config operator-tunable cache thresholds**:
new `CacheConfig` struct in `src/app/config.rs` with 6 fields
(`hot_ratio_threshold`, `cold_ratio_threshold`, `hot_low_ctx_threshold`,
`cold_high_ctx_threshold`, `drift_drop_threshold`, `drift_min_samples`)
mirrors the `[cost]` / `[context]` / `[quota]` per-section pattern.
`PolicyGates` gains 6 `cache_*` fields populated from `CacheConfig`
in `from_config_and_identity` (8th positional param;
`#[allow(clippy::too_many_arguments)]` applied narrowly).
`src/policy/rules/cache.rs` removes the 6 const declarations
introduced in F-7 / F-7b and reads from `gates.cache_*`; reason
strings interpolate the configured threshold so operators see the
actual value that fired. Defaults match the prior hardcoded
constants exactly so no v1.27.x behavior change for default configs.
Operators edit `~/.qmonster/config/qmonster.toml`'s new `[cache]`
section to retune. Settings overlay (`S`) shows the current cache
thresholds in `Parameters` and cache rule conditions in `Rules`;
editing those thresholds remains direct TOML. Refactor side effect: 22
`PolicyGates { … }` literals across `advisories.rs` /
`auto_memory.rs` / `profiles.rs` are simplified to
`..PolicyGates::default()` spread syntax — purely mechanical, no
semantic change.

v1.27.1 is a **Phase F Codex Token usage parsing follow-up**. Codex
`Token usage:` summary lines are now parsed as one ProviderOfficial
surface (`total=`, `input=`, `(+ N cached)`, `output=`) and applied
together, so footer placeholders such as `0 in · 0 out` cannot hide
the real session counts. This preserves F-3 samples, F-4 cache-hit
ratio badges, and F-7/F-7b cache policy inputs on newly resumed Codex
sessions.

v1.27.0 continues **Phase F** with **F-7b cache drift detection rule**:
new `recommend_cache_drift_compact` in `src/policy/rules/cache.rs`
consumes F-3's `recent_token_samples` time series to detect cache hit
ratio drops over time. `Engine::evaluate` extends from 4 to 5
parameters with `recent_token_samples: &[TokenSample]`; threaded
only to `eval_cache` (other eval\_\* functions unchanged). The event
loop reorders per-pane work: `recent_token_samples` SQLite read
moves BEFORE `policy.evaluate`, the F-3 sample write moves AFTER
evaluate, so the historical window passed to policy is strictly
older than the current iteration's snapshot. PaneReport reuses the
early-fetched vec (no duplicate read). The rule fires
`Severity::Concern` with `suggested_command: Some("/compact")` when
the drop ≥ `DRIFT_RATIO_DROP_THRESHOLD` (0.30) between
`samples.last()` (oldest in DESC window) and the current SignalSet
ratio, AND `samples.len() ≥ DRIFT_MIN_SAMPLES` (4), AND
`IdentityConfidence ≥ Medium`, AND no input/permission wait. The
three cache rules (hot warning, cold compact, drift compact) can
co-fire — drift fires on the trend independent of hot/cold
thresholds. Hard-coded thresholds for v1; operator-tunable
thresholds deferred.
