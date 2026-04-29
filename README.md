# Qmonster

Observe-first TUI for multi-CLI tmux development — watches Claude Code /
Codex / Gemini panes (plus itself), surfaces alerts, token-pressure
metrics, runtime facts, and recommendations. It does not touch observed
panes automatically; the operator can press `u` to cycle read-only
provider runtime slash commands on selected non-Claude panes.

- Version: npm package `1.28.0`; current mission ledger `v1.29.0`. Runtime version is sourced from `git describe --tags --always --dirty` via `build.rs` and surfaced in the TUI footer. `Cargo.toml`'s `0.1.0` is internal crate metadata, not the operator-facing version.
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

Current release: `v1.29.0` / npm `1.28.0` (npm publish deferred for this slice).

`v1.29.0` opens **Phase G** with **G-1 Provider Setup overlay**: the new `P` key opens a 3-tab
in-TUI modal (Claude/Codex/Gemini) showing the recommended config snippet for each provider's
statusline/footer plus detected current state of `~/.claude/statusline.sh`, `~/.codex/config.toml`,
and `~/.gemini/settings.json`. Each snippet is rendered inline as copy-pasteable text — the
Claude tab carries an `[s]` toggle that reveals an additional sidefile JSON-export block (writes
the full statusLine JSON to `~/.local/share/ai-cli-status/claude/<session_id>.json` for future
F-5 reader); the Codex tab carries an `[s]` toggle for the deferred Codex App Server polling
guide. Read-only — Qmonster never writes provider config files; operator copies manually. Keys:
`P` opens, `1`/`2`/`3` switch tabs, `s` toggles, `↑↓`/`j/k` scroll, `q`/`Esc` close. 8 new tests;
678 lib + 68 integration green.

`v1.28.0` continues Phase F with F-7-config: operator-tunable cache thresholds. New `CacheConfig`
struct in `src/app/config.rs` exposes the 6 thresholds previously hardcoded in F-7 plus F-7b via
a `[cache]` section in `qmonster.toml`: `hot_ratio_threshold` (0.6), `cold_ratio_threshold` (0.3),
`hot_low_ctx_threshold` (0.7), `cold_high_ctx_threshold` (0.6), `drift_drop_threshold` (0.30),
`drift_min_samples` (4). `PolicyGates` gains 6 `cache_*` fields populated from `CacheConfig` in
`from_config_and_identity` (8th param). `src/policy/rules/cache.rs` removes the 6 const declarations
and reads from `gates.cache_*`; reason strings interpolate the configured threshold so operators see
the actual value that fired. Defaults match prior hardcoded constants exactly — no v1.27.x behavior
change for default configs. Side effect: 22 `PolicyGates { … }` literals across `advisories.rs`
plus `auto_memory.rs` plus `profiles.rs` simplified to `..PolicyGates::default()` spread — purely
mechanical. Settings overlay UI for cache thresholds deferred; operators edit `qmonster.toml`
directly. 3 cache/config tests; 662 lib plus 68 integration green.

`v1.27.1` is a Phase F follow-up for Codex token observability. The Codex adapter now parses
the provider `Token usage:` summary atomically (`total=`, `input=`, `(+ N cached)`, `output=`)
and lets that official surface win over footer placeholders such as `0 in · 0 out`. This keeps
F-3 sampling, F-4 `CACHE` badges, and F-7/F-7b cache recommendations visible on Codex sessions
where the bottom status line has not populated token counters yet.

`v1.27.0` continues Phase F with F-7b: cache drift detection rule. New `recommend_cache_drift_compact`
rule in `src/policy/rules/cache.rs` consumes F-3's `recent_token_samples` time series to detect cache
hit ratio drops over time. `Engine::evaluate` gains a 5th parameter `recent_token_samples: &[TokenSample]`
threaded only to `eval_cache` (other eval\_\* functions unchanged). event_loop reorders per-pane work:
read `recent_token_samples` FIRST, `policy.evaluate` SECOND, F-3 write LAST — so the historical window
passed to policy is strictly older than the current iteration's snapshot. PaneReport reuses the
early-fetched vec (no duplicate SQLite read). The new rule fires `Severity::Concern` `ProjectCanonical`
with `suggested_command: /compact` when `cache_hit_ratio` has dropped ≥ 30 pp (DRIFT_RATIO_DROP_THRESHOLD)
between `recent_token_samples.last()` (oldest in DESC window) and the current SignalSet, AND
`samples.len() ≥ 4` (DRIFT_MIN_SAMPLES), AND `IdentityConfidence ≥ Medium`, AND no input/permission wait.
The three cache rules (hot warning, cold compact, drift compact) can co-fire — drift fires on the trend
independent of hot/cold thresholds. Hard-coded thresholds for v1; operator-tunable thresholds deferred.
Deferred siblings: F-4b (Gemini /stats), F-5 (Claude statusLine), F-6 (Codex App Server), F-7c
(wait_for_reset plus snapshot_before_reset — depend on F-5/F-6 reset_eta). 4 new tests; 659 lib plus
68 integration green.

`v1.26.0` continues Phase F with F-7: cache-aware advisory rules. Two new rules in `src/policy/rules/cache.rs`
turn F-4's `cached_input_tokens` data into actionable `/compact` decisions.
`recommend_cache_hot_compact_warning` (Severity::Concern, SourceKind::ProjectCanonical) fires when
`cache_hit_ratio > 60%` AND `context_pressure < 70%`, advising the operator NOT to compact (compact
resets cache; let context fill further first — wait until ctx >= 80% so the cache rebuild cost amortizes
over more turns). `recommend_compact_when_cache_cold` (Severity::Good, SourceKind::ProjectCanonical,
suggested_command: `/compact`) fires when `cache_hit_ratio < 30%` AND `context_pressure > 60%`,
advising a snapshot-first `/compact` (cache rebuild cost is already paid on every turn, so compacting
won't cost cache effectiveness). Both rules gate on `IdentityConfidence >= Medium` and suppress when
input/permission wait is active. The two rules are mutually exclusive by construction: hot requires
ratio greater than 0.6, cold requires ratio less than 0.3 — strictly disjoint regions; the intermediate
30-60% band triggers neither rule. `Engine::evaluate` dispatches `eval_cache` after `eval_agent_memory`.
Thresholds are hard-coded for v1; operator-tunable thresholds are deferred. Deferred siblings: F-4b
(Gemini /stats parsing), F-5 (Claude statusLine command opt-in), F-6 (Codex App Server resetsAt),
F-7b (cache_drift_detected via recent_token_samples), F-7c (wait_for_reset / snapshot_before_reset —
depend on F-5/F-6 reset_eta). Tests grew to 654 lib and 68 integration green.

`v1.25.0` continues Phase F with F-4: Codex `cached_input_tokens`
parser plus a CACHE hit ratio UI badge. New `parse_codex_cached_input_tokens`
extracts the `(+ N cached)` token from the Codex `/status` welcome
panel into `SignalSet.cached_input_tokens` (ProviderOfficial). The
`token_usage_samples` SQLite table gains a nullable
`cached_input_tokens INTEGER` column; `AuditDb::open` runs an
idempotent `ALTER TABLE` migration that swallows "duplicate column"
errors so v1.24.0 DBs upgrade in place. `TokenSample.cached_input_tokens`
round-trips through INSERT/SELECT, and the event-loop sampling predicate
extends to include cache-only rows. UI surfaces a `cache <N.N>%` text
field plus a `CACHE <N.N>%` badge with one-decimal precision,
computed at render time as `cached / (input + cached) * 100`. Honesty
rule preserved: badge omitted when `cached_input_tokens` is None
(Claude no statusline cache surface; Gemini OAuth FAQ-documented limit).
Tests grew to 646 lib and 68 integration green.

`v1.24.0` continues Phase F with F-3: token usage time series and sparkline UI.
Token samples are persisted to a new `token_usage_samples` SQLite table in
`qmonster.db` — one row per pane per poll when Codex (or a future provider)
reports at least one of `input_tokens` / `output_tokens` / `cost_usd`.
`SqliteTokenUsageSink::record_sample` is fire-and-forget with an `error_count`
AtomicU64 counter mirroring the audit sink; failures log to stderr and never
crash the polling loop. `recent_samples(pane_id, limit=20)` is called for every
live pane every iteration (indexed, sub-ms). The expanded selected pane card
renders a `TOKENS` sparkline by computing `input_tokens` deltas between adjacent
samples (saturating-sub for provider counter resets) and mapping them to the
8-block Unicode set. The sparkline is omitted when fewer than 2 samples exist
(honesty rule). Retention sweep extended: `DELETE FROM token_usage_samples WHERE
ts_unix_ms less than (now minus max_age_days times 86_400_000)` runs after the
archive plus snapshot file sweeps; zero retention is a no-op. Tests grew to 639 lib
and 68 integration green.

`v1.23.0` continues Phase F with F-2: agent memory file scan and bloat
advisory. New `adapters::agent_memory` discovers provider-specific memory
files (Claude sums `CLAUDE.md` plus `~/.claude/CLAUDE.md` plus
`~/.claude/projects/<encoded>/memory/*.md`; Codex sums `AGENTS.md` plus
`~/.codex/AGENTS.md` plus `~/.codex/AGENTS.override.md`; Gemini sums
`GEMINI.md` plus `<project>/.gemini/GEMINI.md` plus `~/.gemini/GEMINI.md`)
and sums their byte sizes (per-file capped at 1 MiB) into
`SignalSet.agent_memory_bytes`. UI surfaces a `MEM-FILE <KB|MB> [Heur]`
badge; sub-1 KiB renders as `<1 KB`. The new
`recommend_memory_bloat_advisory` rule fires `Severity::Concern` above
50_000 bytes (~49 KiB), routing the operator toward `.claude/skills/`,
`~/.codex/AGENTS.override.md`, or `.gemini/skills/` on-demand files.
Tests grew to 630 lib and 67 integration green.

`v1.22.0` opened Phase F with F-1: process RSS is surfaced as `memory <N> MB [Heur]`
on Claude/Codex pane cards. tmux `#{pane_pid}` is captured as the 9th
`PANE_LIST_FORMAT` field; a new `adapters::process_memory` helper walks
`/proc/<pid>/task/<pid>/children` recursively (depth ≤ 5, visited-set) to
find the highest-RSS descendant, preferring `claude/codex/gemini/node/python`
comm names. Gemini's status-table `[Official]` MEM path is preserved
untouched — the `/proc` fill only applies when the provider adapter left
`process_memory_mb` as `None`. 608 lib + 67 integration tests green.

| Area                  | Status   | Operator-visible result                                                                                                                                                                                                                                       |
| --------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Phases 0-5            | Shipped  | Planning, observe-first MVP, SQLite/archive/checkpoints, policy engine, provider profiles, and safer prompt-send are complete.                                                                                                                                |
| Runtime observability | Shipped  | Pane state, command row, provider facts, copyable `run:` commands, security posture badges, cost/context/quota gradients, and settings overlay are live.                                                                                                      |
| Phase B visibility    | Complete | Standard config/pricing paths, Codex token in/out, opt-in security advisories, branch-aware concurrent-work warnings, and copyable commands are closed.                                                                                                       |
| Phase C C1            | Complete | `src/main.rs` is a thin CLI/startup wrapper; the live TUI loop and helpers live under `src/app/`.                                                                                                                                                             |
| Phase C C2            | Complete | `[tmux] source = "auto"` tries control-mode first and falls back to polling when attach is unavailable.                                                                                                                                                       |
| Phase C C3            | Complete | Review-tier profiles (`codex-review`, `gemini-policy-review`) fire on healthy `Role::Review` panes with source-labeled payloads.                                                                                                                              |
| Phase D D1            | Shipped  | Opt-in cross-window concurrent-work findings exist for same path + branch panes across tmux windows.                                                                                                                                                          |
| Phase D D2            | Shipped  | Opt-in identity-drift findings catch provider or worktree changes in the same pane, with per-session dedup.                                                                                                                                                   |
| Phase D D3            | Closed   | Claude subagent detection is refined; per-subagent token attribution stays permanently deferred because providers do not expose counters.                                                                                                                     |
| Phase E E1            | Shipped  | Gemini status-table memory is parsed into `MEM` badges and `--once` metrics.                                                                                                                                                                                  |
| Phase E E2            | Shipped  | Settings overlay writes preserve existing TOML comments, unrelated sections, and key order.                                                                                                                                                                   |
| Phase F F-1           | Shipped  | Process RSS surfaced as `memory <N> MB [Heur]` on Claude/Codex pane cards via /proc descendant walk; Gemini status-table `[Official]` path untouched.                                                                                                         |
| Phase F F-2           | Shipped  | Agent memory file scan (CLAUDE.md / AGENTS.md / GEMINI.md + home-dir + Claude project memory dir) surfaces `MEM-FILE <KB\|MB> [Heur]` badge; `recommend_memory_bloat_advisory` fires Concern above 50_000 bytes (~49 KiB).                                    |
| Phase F F-3           | Shipped  | Token usage persisted to `token_usage_samples` SQLite table (per pane per poll); selected pane card renders `TOKENS ▁▂▃▄▅▆▇█` sparkline of input-token deltas; retention sweep ages out rows with the same `max_age_days` knob as archive/snapshots.          |
| Phase F F-4           | Shipped  | Codex `/status` welcome panel `(+ N cached)` parser populates `SignalSet.cached_input_tokens`; UI renders `CACHE <%>` badge with `cached / (input + cached) * 100` and one-decimal precision; honesty rule preserves missing badge for Claude / Gemini OAuth. |
| Phase F F-7           | Shipped  | Cache-aware advisory rules: `cache_hot_compact_warning` (Concern when cache hot AND ctx headroom) and `compact_when_cache_cold` (Good with `/compact` suggestion when cache cold AND ctx filling); mutually exclusive by ratio threshold construction.        |
| Phase F F-7b          | Shipped  | Cache drift detection rule: fires `Severity::Concern` with suggested `/compact` when `cache_hit_ratio` drops ≥ 30 pp over last 4+ samples; uses F-3 `recent_token_samples` time series; Engine::evaluate gains 5th param.                                     |
| Phase F F-7-config    | Shipped  | `[cache]` config section exposes 6 thresholds for the F-7/F-7b cache-aware rules; defaults preserve prior behavior; reason strings interpolate the configured values so operators see what actually fired.                                                    |
| Phase G G-1           | Shipped  | Provider Setup overlay (`P` key); 3 tabs (Claude/Codex/Gemini); read-only state detectors + `include_str!` snippet content; `s` toggles per-tab optional sections (Claude sidefile JSON / Codex app-server). Read-only — never writes provider config.        |

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
