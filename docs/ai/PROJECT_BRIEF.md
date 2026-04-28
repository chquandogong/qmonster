# PROJECT_BRIEF

- Project: **Qmonster** — Dr. QUAN's Q + monitoring/master
- Version: v0.4.0
- Date: 2026-04-20 (initial) / 2026-04-29 (current — v1.27.0 Phase F F-7b cache drift detection rule)
- Target env: Ubuntu + tmux + Rust TUI
- Phase: **Phases 1–5 + Phase B + Phase C C1/C2/C3 + Phase D D1/D2/D3 + Phase E E1/E2 + Phase F F-1/F-2/F-3/F-4/F-7/F-7b are complete.** v1.27.0 continues Phase F with F-7b: cache drift detection rule. New `recommend_cache_drift_compact` rule in `src/policy/rules/cache.rs` consumes F-3's `recent_token_samples` time series to detect cache hit ratio drops over time; fires `Severity::Concern` with `suggested_command: /compact` when drop ≥ 30 pp over last 4+ samples; `Engine::evaluate` gains 5th param `recent_token_samples: &[TokenSample]`; event_loop reorders read/evaluate/write for correct historical window. 659 lib plus 68 integration tests green. v1.26.0 continues Phase F with F-7: cache-aware advisory rules. Two new rules in `src/policy/rules/cache.rs` turn F-4's `cached_input_tokens` data into actionable `/compact` decisions. `recommend_cache_hot_compact_warning` (Concern, ProjectCanonical) fires when cache hit ratio > 60% AND context pressure < 70%, advising NOT to compact (compact resets cache; let context fill further). `recommend_compact_when_cache_cold` (Good, ProjectCanonical, suggested_command: `/compact`) fires when cache hit ratio < 30% AND context pressure > 60%, advising snapshot-first compact. Both gate on `IdentityConfidence ≥ Medium` and suppress on input/permission wait. Rules are mutually exclusive by construction (hot > 60%, cold < 30% — strictly disjoint). 654 lib plus 68 integration tests green. v1.25.0 continues Phase F with F-4: Codex cached_input_tokens parser plus CACHE hit ratio UI badge. New `parse_codex_cached_input_tokens` extracts `(+ N cached)` from the Codex `/status` welcome panel into `SignalSet.cached_input_tokens` (ProviderOfficial); `token_usage_samples` gains a nullable `cached_input_tokens INTEGER` column with idempotent ALTER TABLE migration; `TokenSample.cached_input_tokens` round-trips through INSERT/SELECT; event loop extends the sampling predicate; UI renders `CACHE <N.N>%` badge with honesty rule (absent for Claude/Gemini OAuth). 646 lib plus 68 integration tests green. v1.24.0 continues Phase F with F-3: token usage time series + sparkline UI. New `token_usage_samples` SQLite table stores per-pane per-poll token samples; `SqliteTokenUsageSink` provides `record_sample` and `recent_samples`; UI renders `TOKENS ▁▂▃▄▅▆▇█` sparkline from `input_tokens` deltas on the expanded selected pane card; retention sweep extended to DELETE aged rows. v1.23.0 continues Phase F with F-2: agent memory file scan + bloat advisory. New `adapters::agent_memory` sums provider-specific memory files (CLAUDE.md / AGENTS.md / GEMINI.md plus home-dir + Claude project memory dir) into `SignalSet.agent_memory_bytes`; UI surfaces `MEM-FILE <KB|MB> [Heur]` badge; new `recommend_memory_bloat_advisory` rule fires `Severity::Concern` above 50_000 bytes (~49 KiB). v1.22.0 opens Phase F with F-1: tmux `#{pane_pid}` is now the 9th `PANE_LIST_FORMAT` field; `adapters::process_memory` walks `/proc/<pid>/task/<pid>/children` (depth ≤ 5 with a visited-set) to surface descendant RSS as `MEM [Heur]` on Claude/Codex pane cards; Gemini's status-table `[Official]` path is preserved. v1.21.3 adds the MIT license and changes Claude `/clear` handling so statusline `CTX —` renders as `CTX 0%` instead of replaying the pre-clear cached CTX value. v1.21.2 keeps Claude statusline placeholder metrics bounded per column so `5h` / `7d` placeholders do not steal adjacent percentages. v1.21.1 keeps Codex `/clear` sessions observable by treating the newest bottom status line as authoritative even when Codex omits the `N used` total immediately after clear; Qmonster still surfaces visible `CTX`, split quota, model/path/branch, and token in/out fields instead of dropping the whole row. v1.21.0 finishes Phase E by parsing the Gemini status-table `memory` column (`118.8 MB` / `1.2 GB`) into `SignalSet.process_memory_mb`, rendered as a `MEM` badge on Gemini pane cards and a `memory <value>` entry in the `--once` metrics row. Claude / Codex leave the field `None` because their status surfaces don't expose process memory. v1.20.0 opened Phase E by replacing the prior `toml::to_string_pretty` settings-save path with a `toml_edit::DocumentMut` surgical update: the operator's hand-written `qmonster.toml` keeps its top-level comments, unrelated sections (`[tmux]`, `[security]`, `[idle]`, ...), and key order across every overlay save. Settings overlay still validates threshold pairs before writing, and the fresh-write fallback (file does not exist yet) keeps the legacy scaffold so operators who have never opened the overlay get a sensible starting file. v1.19.0 closes Phase D D3: D3-A refines Claude subagent detection so `● Task(` fires `subagent_hint` while ordinary tool calls (`● Bash(...)`, `● Read(...)`) and TODO-list prose (`Task 1 — ...`) stay silent; D3-C marks per-subagent token attribution as permanently deferred — none of the three providers exposes per-subagent input/output counters, and any delta-window guess would re-introduce the v1.13.0 anti-pattern Qmonster already removed for log_storm/verbose_answer. v1.18.0 added Phase D D2 identity-drift anomaly detection: when an operator opts in via `[security] identity_drift_findings = true`, a passive `Concern` recommendation fires on the affected pane the first time its resolved provider or `current_path` changes between polls (Claude→Codex inside the same pane, or `cd` into a different worktree). Per-session dedup keeps the same drift from re-firing. Default config preserves the v1.17.1 alert volume exactly. v1.17.0 opened Phase D with cross-window concurrent-work correlation: same `current_path` + `git_branch` panes spread across 2+ tmux windows fire a `Cross-Window` Concern finding when the operator opts in via `[security] cross_window_findings = true`. Default config keeps the existing same-window `Cross-Pane` Warning behavior unchanged. v1.17.1 changes Claude observability to statusline-first: Qmonster reads Claude `CTX`, `5h`, `7d`, model, effort, path, and permission mode directly from the visible statusline and no longer sends Claude slash commands from the `u` key. Phase C C2 made `[tmux] source = "auto"` the default: auto attaches control-mode first, falls back to polling with a startup notice if attach fails, and keeps explicit `control_mode` strict. v1.16.51 originally split Claude/Codex pressure semantics into context, 5h quota, and weekly quota windows; Codex still sources those from bottom-status `5h`/`weekly`, while Claude now sources the current values from statusline `CTX`/`5h`/`7d`. v1.16.53 inverts Codex remaining-quota percentages into Qmonster pressure. v1.16.54 keeps runtime refresh captures parseable without hiding the live prompt cursor, restores reliable `c` system-notice clearing, and aligns npm package semver with the ledger tag. v1.16.55 completes C3 by adding `codex-review` and `gemini-policy-review` profile recommendations for healthy `Role::Review` panes. v1.16.56 narrows the `code-exploration` advisory: the bare `output_chars >= 1500` fallback was firing on every healthy Main pane in the 2026-04-28 live audit (same v1.13.0 anti-pattern that was already removed for log_storm/verbose_answer). v1.16.57 fixes two live operator findings: Gemini `thinking...` tails suppress stale-IDLE fallback, and Claude active-pane runtime refresh stops sending defensive pre-`Escape`. v1.16.58 suppresses Gemini live-prompt `IDLE DONE` while recent tail history is still changing.

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
