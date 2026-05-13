# VALIDATION

- Version: v0.4.0
- Date: 2026-04-20 (round r2 reconciled) / 2026-04-30 (implementation validation sync) / 2026-05-11 (v2.1.0 ledger sync; release log + live-smoke validation matrix updated through v2.1.0 r2 synthesis-slice bundle) / 2026-05-12 (current — v2.2.0 critical-eval improvement bundle: P0 dead-code purge (`SignalSet.repeated_output` field + alerts/advisories rules; `TaskType::{CodeExploration,LogTriage,Summary}` variants; `log_storm` alert dedup with advisory; quota-window dedup to one most-urgent rec), P0-3 inline threshold provenance tags on every `PolicyGates::default()` magic number, P1-1 `AnomalyKind::supported_providers()` matrix gating `eval_anomalies` per provider + Settings Rules `supports: …` chips, P1-2 SubagentSideEffect prose lead-with-disclaimer (`⚠ correlation only —`) + evidence row `cooccurring_kinds`, P1-3 `RecommendationOutcome::Copied` lifecycle row written from the `y`-copy path. P2 decisions stay drafted in `.mission/decisions/MDR-DRAFT-v2.2.0-*` pending measurement and operator approval.)

This doc defines what "good" looks like for Qmonster at each phase, and
what reviewers (Codex, Gemini, and the human operator) should
specifically check. It is intentionally short; deeper rubric detail
lives in `REVIEW_GUIDE.md`. Every displayed metric must carry a
`SourceKind` label per `ARCHITECTURE.md` §"SourceKind taxonomy".
Checkboxes below represent phase acceptance evidence. Later phases may
supersede an earlier phase's negative scope item; those cases are
called out inline.
Current local verification (2026-05-06): `cargo fmt --all --check`,
`git diff --check`, `cargo test --all-targets`,
`cargo clippy --all-targets -- -D warnings`,
`cargo build --release`, `npm pack --dry-run`, and
`scripts/verify-shared.sh` pass for v1.41.0. Current code-level
verification at the v1.41.0 release commit is
`cargo test --all-targets` with 1028 lib tests, 50 event-loop
integration tests, 18 false-positive regression tests, and 6
idle-state regression tests passing, plus `cargo build --release`,
`cargo clippy --all-targets -- -D warnings`,
`git diff --check`, and `npm pack --dry-run`.
Official
`mission-spec validate .` is still unavailable locally because
`mission-spec` is not installed, so `scripts/verify-shared.sh` falls
back to the lite ledger-structure check after cargo checks.
Latest focused verification (2026-05-07): `cargo fmt --all --check`,
`git diff --check`, `cargo test --all-targets`, and
`cargo clippy --all-targets -- -D warnings`
pass for the Phase 8 footer/help/S/P/anomaly discoverability pass
(1211 lib tests plus CLI, event-loop, false-positive, idle-state,
insights-report, and store-insights integration suites).
Latest focused verification (2026-05-11): `cargo fmt --all --check`,
`git diff --check`, `cargo test --all-targets`,
`cargo clippy --all-targets -- -D warnings`,
`scripts/verify-shared.sh`, and `cargo build --release` pass for the
post-v2.0.0 slice set: attribution lock follow-ups, pane-bucketed
Insights, action-impact windows, timestamp-normalized anomaly slope
detection, Insights data completeness, live-smoke validation docs, and
UI synthesis slices A-I (Now strip, next-best-action, `/compact`
payoff, anomaly evidence expansion, ETA chips, policy mini-strip, cost
breakdown, cross-pane correlations, audit severity chip).
`scripts/verify-shared.sh` used the lite ledger structure fallback
because `mission-spec` is not installed locally.

## Planning-phase gates (Phase 0)

- [ ] `mission.yaml` validates with mission-spec schema.
      Current 2026-04-27 local check is blocked because the
      `mission-spec` binary is not installed in this workspace;
      `scripts/verify-shared.sh` now runs a lite ledger-structure
      fallback after cargo checks.
- [x] `mission-history.yaml` has a `1.0.0` entry and (after r2) a
      `1.1.0` reconciliation entry.
- [x] `.mission/CURRENT_STATE.md` is filled with real content for today
      (not the empty template).
- [x] `docs/ai/*.md` are canonical (stable rules), not diaries.
- [x] `CLAUDE.md`, `AGENTS.md`, `GEMINI.md` each ≤ ~60 lines and only
      route (no procedures, no architecture, no state).
- [x] `.docs/claude/Qmonster-v0.4.0-2026-04-20-claude-plan-r1.md`
      exists and covers all 11 required sections.
- [x] `.docs/codex/Qmonster-v0.4.0-2026-04-20-codex-crosscheck-r1.md`
      and `.docs/gemini/Qmonster-v0.4.0-2026-04-20-gemini-research-r1.md`
      exist with explicit verdicts.
- [x] `.docs/final/Qmonster-v0.4.0-2026-04-20-claude-final-r2.md`
      exists and classifies every reviewer item (now / Phase 1 / Phase 2+ / rejected).
- [x] Every provider lever in docs carries a `SourceKind`:
      `ProviderOfficial` / `ProjectCanonical` / `Heuristic` / `Estimated`.
- [x] Phase 0 placeholder-only binary existed before implementation.
      Current acceptance is the shipped TUI plus reproducible
      `cargo test --all-targets` / clippy evidence, not a placeholder.

## Phase 1 (Observe-first MVP) historical acceptance checks

- [x] `tmux::PaneSource` returns `RawPaneSnapshot` via polling.
- [x] `domain::IdentityResolver` resolves `(provider, instance, role,
pane_id)` with an `IdentityConfidence` level. Provider-specific
      recommendations are suppressed when confidence is `Low` or
      `Unknown`.
      v1.15.20 adds Medium-confidence default-role fallback for
      structurally identified non-canonical provider/status panes:
      Claude/Codex/Gemini fall back to `main`, Qmonster to `monitor`;
      canonical pane titles still win and prose-only tail hints stay
      Low/Unknown.
- [x] **Attribution Lock / identity conflict suppression** (post-v2.0.0
      Slice 0, 2026-05-11): canonical or non-canonical title evidence no
      longer wins blindly when process-level command evidence names a
      different provider. Such panes resolve with
      `IdentityConfidence::Conflict`; provider adapters, Claude sidefile
      enrichment, CLI version facts, Codex account rate-limit enrichment,
      and provider-profile recommendations are suppressed. Pane titles
      show `IDENTITY CONFLICT`, and audit records one metadata-only
      `IdentitySuppressed` event per pane conflict episode. Claude
      sidefile enrichment with `pane_pid` now requires two matching
      facts: sidefile `cwd == current_path` and descendant CLI evidence
      whose basename is Claude. Follow-up fix (2026-05-11): a live
      `qmonster` process command is authoritative for the Qmonster
      self-monitor pane even when the tmux title is stale provider
      metadata from the slot; it resolves as Qmonster/monitor with
      Medium confidence, not Conflict. Follow-up fix (2026-05-11):
      token history is filtered by the current resolved provider before
      policy/UI use, and Qmonster/Unknown panes receive no token-history
      window; the expanded Panes token rows render only for Claude,
      Codex, and Gemini. Locked by
      `canonical_title_command_provider_conflict_is_not_high_confidence`,
      `qmonster_command_overrides_stale_canonical_provider_title`,
      `sidefile_skipped_for_conflicting_claude_identity`,
      `sidefile_skipped_when_descendant_process_is_not_claude`,
      `run_once_records_identity_suppression_for_title_command_conflict`,
      `run_once_treats_qmonster_command_as_monitor_despite_stale_provider_title`,
      `run_once_does_not_attach_stale_provider_token_samples_to_qmonster_pane`,
      `selected_qmonster_pane_omits_token_row_even_with_stale_samples`,
      and `panel_title_marks_identity_conflict`.
- [x] `adapters/` never performs identity inference.
- [x] Basic alert extraction in `policy/rules/alerts.rs`: input-wait,
      permission-wait, verbose-answer, error-hint, **subagent-hint**.
      (`log-storm` and `repeated-output` historically ran here too;
      `log-storm` moved to `advisories.rs` only as part of the v2.2.0
      dedup, and `repeated-output` was removed in v2.2.0 as dead code —
      no adapter ever populated the underlying signal.)
- [x] **Subagent detection warning** fires when the tail matches the
      detector vocabulary (provider-specific patterns; initially
      `Heuristic`) and tells the operator "token consumption may be
      delayed or missing in main stats". v1.19.0 (Phase D D3-A) adds
      `● task(` to the marker set so Claude Code's Task subagent tool
      tail signature triggers the alert; ordinary tool calls
      (`● Bash(...)`, `● Read(...)`) and TODO-list prose
      (`Task 1 — capture fixtures`) stay silent. v1.19.0 also marks
      per-subagent **token attribution** as permanently deferred (D3-C):
      no provider exposes structured per-subagent input/output
      counters, so any "Sub" split would be a delta-window guess that
      conflates with main-agent activity.
- [x] **Zombie pane / session re-attach**: on `pane_dead` transition,
      per-pane alert queue is drained and context-pressure warnings
      for that pane are reset. On session re-attach, stale alerts
      older than the re-attach moment are hidden.
- [x] **Version drift detector**: on startup and on each operator-
      requested refresh, Qmonster captures CLI versions
      (`claude --version`, `codex --version`, `gemini --version`,
      `tmux -V` when available); on change, fires a `warning`-severity
      alert "re-verify `[ProviderOfficial]` tags in `docs/ai/`".
      Runs only when the operator triggers it (consistent with
      `refresh.policy = manual_only`).
- [x] Context / token / cost metrics are **display-only** in Phase 1;
      not used as gating signals. Each value carries a `MetricValue<T>`
      with `SourceKind` and is rendered with a `SourceKind` badge.
- [x] Phase 1 storage baseline shipped only `EventSink` trait +
      `NoopSink` + `InMemorySink`; SQLite, archive persistence, and
      snapshot export are Phase 2+ features.
- [x] Recommendations are explainable: every recommendation carries a
      human-readable `reason` and a `SourceKind`.
- [x] Destructive automation is disabled by default (`recommend_only`).
- [x] Safety precedence enforced at startup: attempted upward override
      of `actions.mode`, `allow_auto_prompt_send`,
      `allow_destructive_actions`, `refresh.policy` via env/CLI is
      ignored and logged as `risk`.
- [x] Qmonster runtime writes only within the resolved Qmonster root
      (default `~/.qmonster/`). No writes to `CURRENT_STATE.md`,
      `mission.yaml`, provider config files, or source files.

## Phase 2 (Archive / Checkpoint) checks

- [x] `store/sqlite.rs` initialized; audit DB stores metadata only.
- [x] `store/archive_fs.rs` writes raw tails with preview/full split at
      the configured char threshold.
- [x] `store/audit.rs` writer type signature cannot accept raw bytes
      (compile-time enforcement). Raw text cannot bleed into audit.
- [x] Runtime checkpoints land in `<qmonster-root>/snapshots/`, never
      in `.mission/CURRENT_STATE.md`.
- [x] Retention job honors the 14-day default (config-driven).
- [x] `.mission/CURRENT_STATE.md` refresh flow documented and exercised
      at least once as a **human** day-end action.

## Phase 3 (Policy Engine) checks

- [x] A–G canonical rules each fire in a reproducible test fixture:
      log-storm, code-exploration, context-pressure, verbose-output,
      permission-wait, quota-tight. (`repeated-output` rule was removed
      in v2.2.0 because the underlying `SignalSet.repeated_output` was
      always `false` — no adapter ever populated it.)
- [x] **Concurrent-work warning** across panes (Gemini G-11). v1.15.23
      narrows the old project-level proxy: fires only when two or more
      busy Main/Review panes share both `current_path` and
      `signals.git_branch`. Path-only overlap is no longer enough.
      File-level detection remains deferred until a trustworthy
      active-file signal exists. v1.17.0 (Phase D D1) splits the same
      group by tmux window: same-window groups still emit the existing
      `ConcurrentMutatingWork` Warning; cross-window groups emit a new
      `CrossWindowConcurrentWork` Concern when `[security]
cross_window_findings = true`. Default config preserves the
      v1.15.23 behavior exactly.
- [x] `aggressive_mode` only surfaces recommendations when
      `quota_tight = true` in config.
- [x] Every rule carries a `SourceKind`; for `Heuristic` rules, a
      pointer to the community source is recorded.
- [x] Quota-pressure gradient advisories keep authority split honest:
      provider quota metrics are `ProviderOfficial`, while the 75% /
      85% advisory recommendations are `Estimated` Qmonster thresholds.
      Claude and Codex keep rolling 5-hour and weekly windows separate;
      Gemini keeps the single `quota_pressure` surface.
- [x] Security posture advisories are opt-in: permissive runtime facts
      (YOLO / bypass / Full Access / `danger-full-access` / no sandbox)
      stay badge-only by default and become passive `Concern`
      recommendations only when `[security] posture_advisories = true`.
- [x] Identity-drift anomaly detection is opt-in (Phase D D2 v1.18.0):
      when `[security] identity_drift_findings = true`, a passive
      `Concern` recommendation fires the first time a pane's resolved
      provider or `current_path` changes between polls. Per-session
      dedup by `(pane_id, "<kind>:<from>→<to>")` keeps the same drift
      from re-firing every poll. `Provider::Unknown → known` and empty
      → present path transitions are treated as identity catch-up
      rather than drift, so initial sightings stay quiet. Lifecycle
      reset (`PaneLifecycleEvent::{BecameDead, Reappeared}`) clears
      both the per-pane history and the dedup keys for that pane.
- [x] Recommendations may carry a `suggested_command: Option<String>`
      for copy-paste ergonomics. The value must be runnable on a single
      surface (shell command or in-pane slash-command). Config-edit
      instructions, comments, placeholders, and mixed-mode prose
      (e.g. TUI keybinding prose plus a slash-command) belong in
      `next_step`, not here. Rendered by both the alert queue and
      `--once` with a ``run: `…``` prefix; the interactive copy path
      defensively suppresses `# …` comments and `<placeholder>` values.
- [x] v1.15.24 makes `suggested_command` copyable in the interactive
      TUI: when Alerts are focused, `y` copies the selected alert's
      non-empty `run:` command to the system clipboard via `arboard`.
      Missing commands and clipboard backend failures surface as
      `SystemNotice` rows instead of silently failing.
- [x] Recommendations may carry a `next_step: Option<String>` — prose
      precondition that precedes the runnable `suggested_command`.
      Required for strong recs whose safe execution depends on a step
      that cannot be expressed on the same surface as the command
      (e.g. C-warning / C-critical need "press `s` to snapshot first"
      before running `/compact`). Rendered as `next: …` **before**
      ``run: `…``` so the ordering is inherent to the field layout.
- [x] "Checkpoint before compact" surfaces as a **strong recommendation
      with actionable steps**, not as forced automation. The snapshot
      step lives in `next_step`, and `/compact` lives verbatim in
      `suggested_command`; the single-source render helper
      `ui::alerts::format_strong_rec_body` guarantees the `next:` →
      `run:` order in both the TUI and `--once`.

## Phase 4 (Provider Profiles) checks

Levers below are cited as `[ProviderOfficial]` with doc pointers.

- [x] Claude settings surface audited: `includeGitInstructions`,
      `BASH_MAX_OUTPUT_LENGTH`, `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS`,
      `MAX_MCP_OUTPUT_TOKENS`, `autoConnectIde`,
      `autoInstallIdeExtension`, `CLAUDE_CODE_GLOB_NO_IGNORE`,
      `ENABLE_CLAUDEAI_MCP_SERVERS`, `attribution.commit`,
      `attribution.pr`, and low-token flags
      (`--bare`, `--exclude-dynamic-system-prompt-sections`,
      `--strict-mcp-config`, `--disable-slash-commands`, `--tools`,
      `--no-session-persistence`). `[ProviderOfficial]`
      (P4-1 v1.8.0 claude-default 3 PO levers + P4-3 v1.8.3
      claude-script-low-token 8 PO levers)
- [x] Codex settings surface audited: `web_search` (`cached` default),
      `tool_output_token_limit`, `commit_attribution`,
      `model_auto_compact_token_limit`, `[features].apps`,
      `[apps._default].enabled`, `mcp_servers.<id>.enabled`, and exec
      flags (`codex exec --profile --json --output-last-message
--sandbox read-only --ephemeral --color never`).
      `[ProviderOfficial]`
      (P4-4 v1.8.4 codex-default + P4-5 v1.8.7
      codex-script-low-token aggressive bundle)
- [x] High-risk Claude levers (`CLAUDE_CODE_DISABLE_AUTO_MEMORY`,
      `CLAUDE_CODE_DISABLE_CLAUDE_MDS`,
      `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS`) are gated to
      `claude-script-low-token` only — never proposed as always-on
      defaults.
      (locked by `high_risk_claude_levers_are_gated_to_claude_
script_low_token_only` test in P4-3 v1.8.3)
- [x] Gemini profile recommendations stay advisory; `save_memory` /
      Auto Memory is not treated as a state store.
      (P4-6 v1.8.8 gemini-default + P4-7 v1.8.9/v1.8.10
      gemini-script-low-token. `Severity::Good` + no Notify path;
      `experimental.autoMemory = false` ships as the documented
      disable surface per v1.8.10 correction)
- [x] High-compression profile recommendations carry a
      `side_effects: Vec<String>` list (e.g., "may lose debugging
      detail") visible in the UI (Gemini G-6).
      (G-6 parity across all 3 aggressive profiles — Claude in
      P4-3, Codex in P4-5, Gemini in P4-7. Renderer = `format_
profile_lines` in `src/ui/panels.rs`; emits a
      `side_effects (<n>):` block after the lever rows)
- [x] Auto-memory guidance: profiles recommend "record to MDR /
      CURRENT_STATE, not to auto-memory" when state-critical work is
      detected (Gemini G-5).
      (P4-2 v1.8.2 `recommend_mdr_over_auto_memory` rule in
      `src/policy/rules/auto_memory.rs`; fires on any provider
      under `TaskType::Review` / `TaskType::SessionResume`. G-5
      cross-reference also embedded in the aggressive profile
      `side_effects` for all 3 providers)
- [x] Review-tier profiles fire on review-role panes without colliding
      with main-pane baseline/aggressive profiles.
      (Phase C C3 v1.16.55: `codex-review` and
      `gemini-policy-review` fire only on `Role::Review` at medium-or-
      higher confidence, carry structured profile payloads, and remain
      independent of the `quota_tight` main-pane switch.)
- [x] `token.thresholds` structure may split per-provider ONLY if
      Phase-1 fixture data justifies the split; otherwise keep one
      global block.
      (P4-8 decision v1.8.13: single global block retained. Rationale:
      (1) the current `tests/fixtures/real/` corpus is a real-tail
      parser/idle regression corpus, not measured per-provider
      threshold calibration data that would justify a split; (2) of
      the 5 keys in `[token.thresholds]`
      (`context_warn`, `context_crit`, `big_output_chars`,
      `log_storm_lines`, `repeat_window`), only `big_output_chars`
      is currently wired into Rust (`src/app/config.rs:131`,
      consumed by `ArchiveWriter`) — the other 4 keys are config-
      surface only, so a speculative per-provider split would add
      config burden without touching actual policy behavior; (3)
      provider asymmetry lives in the profile-rule layer
      (`src/policy/rules/profiles.rs`) and the identity-confidence
      gates, not in raw token thresholds. If future Phase-1
      fixture data ever surfaces a real per-provider divergence,
      the split can still happen then without penalty — the
      current global block is not locking us in.)

## Phase 5 (Safer Actuation) checks

- [x] Manual prompt-send helper requires explicit user confirmation.
      (P5-3 v1.10.0: `check_send_gate` enforces explicit operator
      keystroke `p` + `allow_auto_prompt_send = true` as two independent
      gates before any `tmux send-keys` call; `EffectRunner::permit`
      remains display-layer only and does NOT gate execution)
- [x] Every actuation is recorded in the audit log with approver, pane,
      command, and outcome (metadata only, no raw text).
      (P5-3 v1.10.0: `PromptSendCompleted` on successful send,
      `PromptSendFailed` on post-confirmation error, `PromptSendBlocked`
      on ObserveOnly accept-block; prose summary `"{pane} {cmd}
(verb; ...)"` — no raw bytes per audit-isolation rule.
      SQLite roundtrip locked by `p5_3_prompt_send_kinds_roundtrip_
through_sqlite` test in `src/store/audit.rs`)
- [x] Destructive actions remain outside the automation surface. No
      code path exists for auto `/compact`, `/clear`, `/memory`
      mutation, provider reconfiguration.
      (P5-3 v1.10.0: safety-precedence asymmetry preserved; only
      explicit operator `p` keystroke + `allow_auto_prompt_send = true`
      can reach `send_keys`; observe_only and recommend_only defaults
      both gate before `Execute` branch; no auto-trigger path exists)
- [x] Phase H opt-in auto-snapshot (v1.42.0): `[reset] auto_snapshot = false`
      by default; `auto_snapshot = true` writes one snapshot per
      `(pane_id, quota_kind, window_id)` triple via
      `app::auto_snapshot::maybe_auto_snapshot`; `SnapshotWritten`
      audit summary carries `trigger=auto_reset_boundary` and
      `quota_kind`.
- [x] Phase 7 v1 anomaly observation (v1.43.0) + Phase 7 v2 promotion (v1.44.0) + Phase 7 v2 detectors (v1.45.0):
      `[anomaly] enabled = false` by default; `enabled = true` opens
      the m overlay ANOMALIES row per pane via
      `policy::rules::anomaly::eval_anomalies` over a per-pane rolling
      `AnomalyHistory`; edge-triggered dedup keeps one emission per
      active window. v1.44.0 adds Recommendation + Notify promotion
      for confidence=High signals via `promote_anomalies_to_recommendations`.
      v1.45.0 adds 4 new AnomalyKind variants (CostSlope / TokenSlope
      / MemoryGrowth / SubagentSideEffect) that inherit the promotion
      path. No new AuditEventKind, no new RequestedEffect, no new
      schema, no new operator-tunable promotion thresholds.
- [x] Phase 7 anomaly slope timing correction (post-v2.0.0 Slice 3,
      2026-05-11): CostSlope and TokenSlope normalize by real
      `AnomalyHistory` tick timestamps when available instead of
      assuming `window_polls × 5s`. TokenSlope preserves existing
      `tokens_per_poll` config semantics by converting elapsed seconds
      to 5-second poll-equivalent units. Fired slope signals include a
      `sample_coverage` evidence row with sample count and elapsed
      seconds. Locked by
      `detect_cost_slope_uses_actual_elapsed_seconds_when_available`,
      `detect_token_slope_uses_actual_elapsed_seconds_when_available`,
      and `detect_cost_slope_reports_coverage_and_elapsed_seconds`.
- [x] Token Insights Phase 8 lifecycle writer: live `run_once` records
      recommendation events to `recommendation_events`, strong
      prompt-send recommendations are keyed by the slash command, and
      prompt-send/archive/snapshot/alert-hide outcomes write correlated
      `recommendation_outcomes`. TTL ignored remains a Qmonster
      classification controlled by `[insights] ignored_ttl_secs`.
- [x] Token Insights Phase 8 TUI overlay: `i` opens the SQLite-backed
      report overlay; `r` refreshes; `Esc`/`q`/`i` close. The overlay
      uses the CLI report formatter so situation, cache, timeline, and
      action-ledger labels stay aligned.
- [x] Token Insights pane-bucketed aggregation (post-v2.0.0 Slice 1,
      2026-05-11): cache reuse, token growth, and cost delta are
      accumulated by `(pane_id, provider)` before the aggregate cache
      summary is derived. Token counter resets act as implicit session
      boundaries: negative deltas are not counted, while later positive
      growth in the same pane/provider bucket is counted. The CLI/TUI
      report renders the top pane buckets with sample count, cache
      reuse, token growth, and cost delta. Locked by
      `cache_summary_buckets_token_growth_by_pane_and_provider`,
      `cache_summary_continues_token_growth_after_counter_reset`, and
      `insights_report_renders_action_ledger`.
- [x] Token Insights action-impact / ROI loop (post-v2.0.0 Slice 2,
      2026-05-11): linked or matched recommendation outcomes compare
      the same pane/provider over a 10-minute pre-event metric window
      and a 10-minute post-outcome metric window. The report renders
      `Action Impact` rows with token growth, cost delta, and cache
      reuse before/after; deltas are comparative evidence, not
      provider-official saved-token claims. Locked by
      `insights_lifecycle_reports_action_metric_impact_windows` and
      `insights_report_renders_action_ledger`.
- [x] Token Insights data completeness (post-v2.0.0 Slice 4,
      2026-05-11): snapshots include pane bucket count, total token
      samples, cache-ratio bucket coverage, cost-delta bucket coverage,
      and action-impact count. The CLI/TUI report renders a
      `Data Completeness` section so operators can distinguish missing
      window data from real zeroes. Locked by
      `cache_summary_buckets_token_growth_by_pane_and_provider` and
      `insights_report_renders_action_ledger`.
- [x] UI synthesis Now strip (post-v2.0.0 Slice A, 2026-05-11):
      dashboard chrome reserves a one-line `Now` strip that prioritizes
      wait/input states, strong risk recommendations, quota pressure,
      recent promoted anomalies, and a healthy fallback. Locked by the
      `now_strip_*` dashboard regression tests.
- [x] Token Insights next-best-action and `/compact` payoff
      (post-v2.0.0 Slices B/C, 2026-05-11): snapshots include the
      latest strong unaccepted recommendation, open proposal count, and
      the last `/compact` pre/post token-growth comparison when enough
      samples exist. Reports render `Now Suggested Action` and
      `/compact Payoff`. Locked by
      `insights_next_best_action_selects_pending_strong_recommendation`,
      `insights_next_best_action_picks_latest_unaccepted_strong_rec`,
      and the `compact_payoff_*` integration tests.
- [x] Anomaly evidence persistence and expansion (post-v2.0.0 Slice D,
      2026-05-11): promoted anomaly events persist evidence JSON in
      SQLite, hydrate it back into the ring view, and expose an `e`
      toggle in the Anomaly Events overlay. Locked by
      `anomaly_event_evidence_migrates_and_roundtrips`,
      `evidence_rows_render_only_when_expanded`, and
      `e_toggles_anomaly_evidence_only_when_open`.
- [x] Metrics ETA and runtime policy mini-strip (post-v2.0.0 Slices E/F,
      2026-05-11): Metrics renders estimated 85% pressure ETA chips
      only for sufficiently clean rising trends, and pane metric rows
      show provider-scoped `POLICY approval=... sandbox=...` runtime
      facts for Claude/Codex/Gemini. Locked by the `eta_chip_*`,
      `metrics_lines_render_eta_banner_below_hottest_banner`,
      `policy_mini_strip_provider_matrix_known_and_unknown_states`, and
      `metric_badge_line_includes_policy_mini_strip` tests.
- [x] Token Insights cost breakdown and cross-pane anomaly correlation
      (post-v2.0.0 Slices G/H, 2026-05-11): snapshots include
      cost-usage groups by pane/model/situation and same-minute
      anomaly groups across multiple panes. Reports render
      `Cost Breakdown` and `Cross-pane Correlations`. Locked by
      `cost_breakdown_groups_window_cost_by_pane_model_and_situation`,
      `cost_breakdown_no_data_returns_empty_groups`,
      `anomaly_correlations_group_two_panes_in_same_minute`, and
      `anomaly_correlations_ignore_same_pane_or_different_minutes`.
- [x] Dashboard audit severity chip (post-v2.0.0 Slice I,
      2026-05-11): footer status now includes stable `★a` state from
      the recent audit-event severity window. Warning/Risk render in
      severity color; none/Concern stay dim. Locked by
      `recent_audit_max_severity_respects_recency_window`,
      `recent_audit_max_severity_returns_none_without_recent_rows`,
      `footer_audit_concern_renders_dim`,
      `footer_nonzero_counts_render_severity_color`, and
      `footer_zero_counts_render_dim`.
- [x] Overlay chrome first slice: shared `[x]` style uses active-border
      color rather than severity color; `i` Token Insights and `n`
      Anomaly Events support `[`/`]` resize, `=` geometry reset,
      title-row drag, same-key close, body-local wheel/arrow scroll,
      `[x]` close parity with `m`/`a`, and footer `focus: overlay`
      while any key-owning overlay is open.
- [x] Token optimization discoverability: footer/help list `s` snapshot,
      `n` Anomaly Events, `m` Metrics, `a` Pending Actions, and `i`
      Token Insights in a stable cluster; `S` Settings documents
      `[insights]`, `[anomaly]`, `[reset]`, and `[provider_setup]`
      surfaces and scrolls read-only tabs with body-local wheel /
      arrow / page keys; `P` Provider Setup states that configured telemetry
      feeds `m`/`n`/`i`; Anomaly Events supports `[x]` close parity.

## Non-functional checks (all phases)

- [x] No giant always-loaded prompt file in CLAUDE.md, AGENTS.md, or
      GEMINI.md.
- [x] Auto memory never holds the only copy of today's state.
- [x] Refresh remains `manual_only` unless the operator explicitly
      changes policy.
- [x] Default operator config path is `~/.qmonster/config/qmonster.toml`.
      Qmonster loads it automatically when present; otherwise defaults
      remain in memory but the settings overlay can save there.
- [x] Cost pricing remains operator-curated at
      `~/.qmonster/config/pricing.toml`; Qmonster does not fetch prices.
- [x] `logging.sensitivity` honors `balanced` by default.
- [x] Color usage follows the low-saturation palette rule. No
      color-only state indication. Every color is accompanied by a
      numeric % or severity letter, and pane state transitions pair the
      pulse highlight with a `CHANGED` text badge.
- [x] `SourceKind` labels visible next to every metric and every
      recommendation — in the UI, not just in docs.
- [x] Provider runtime facts are visible when sourced from provider
      status/slash output or readable local provider settings. Unknown
      values stay blank; Qmonster does not infer hidden tools, skills,
      plugins, sandbox, or permission state from prose.
- [x] Qmonster runtime never writes outside the resolved Qmonster root.
- [x] Audit log and raw archive are separated at the writer level
      (type-level enforcement, not policy).

## Evidence expectations

- Phase-0 evidence: the planning bundle (mission artefacts +
  `docs/ai` + planning report + review docs + final synthesis).
- Phase-1+ evidence: reproducible pane fixtures (tail samples) + test
  output + screenshots (saved under `.docs/final/` for the round).
- Per-fixture: include the pane tail, the resolved identity (with
  confidence), the emitted signals (with `SourceKind`), and the
  triggered recommendations.
- Phase 8 Token Insights evidence: include
  `run_once_writes_recommendation_lifecycle_events`,
  `insights_lifecycle_counts_hidden_and_suppresses_ignored_for_unlinked_matches`,
  `insights_report_renders_available_recommendation_lifecycle`,
  `cache_summary_buckets_token_growth_by_pane_and_provider`,
  `cache_summary_continues_token_growth_after_counter_reset`,
  `insights_lifecycle_reports_action_metric_impact_windows`, and overlay
  key/render tests, plus `cargo test --all-targets` and clippy.

## Live smoke checklist

- [ ] Capture the tmux pane matrix and verify each pane's provider,
      role, confidence, current command, and source-kind badges.
- [ ] Include wrapper-command panes such as `node /usr/bin/gemini`,
      shell-launched provider CLIs, and the Qmonster monitor pane.
- [ ] Reuse a stale tmux title intentionally and confirm command
      evidence either resolves Qmonster correctly or marks a real
      cross-provider conflict as `IDENTITY CONFLICT`.
- [ ] Place an unrelated Claude sidefile in the same cwd and confirm
      sidefile enrichment is suppressed unless cwd and descendant CLI
      evidence both match Claude.
- [ ] Open expanded Panes for Qmonster/Unknown panes and confirm no
      stale provider token row is rendered.
- [ ] Open Token Insights and confirm `Pane Buckets`, `Action Impact`,
      and `Data Completeness` agree with the pane matrix for the chosen
      time window.
- [ ] Open Anomaly Events after sparse or delayed polling and confirm
      CostSlope/TokenSlope evidence includes `sample_coverage` and real
      elapsed seconds.

## Release validation log

| Version | Scope                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Gates passed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| v2.1.0  | r2 cross-validated synthesis-slice implementation bundle on top of the v2.0.0 milestone. Lands the post-v2.0.0 set documented above: Slice 0 Attribution Lock (descendant exe + identity conflict suppression, Claude sidefile multi-evidence matching, IdentitySuppressed audit kind), Slice 1 pane-bucketed Insights re-aggregation, Slice 2 ROI loop closure (`/compact` payoff windows + outcome family separation + rule tuning candidates), Slice 3 anomaly time-normalization (`ts_unix_ms` based slopes) + evidence enrichment with `evidence_json` schema migration, Slice 4 Insights data completeness, Slice 6 live-smoke validation matrix + `--once` fixtures, plus UI synthesis slices A-I (Now strip, next-best-action, `/compact` payoff chip, anomaly evidence row expansion, ETA chips with timestamp normalization, policy mini-strip, cost breakdown by pane/model/situation, cross-pane correlation hints, audit severity chip). Does not add new modals, new detectors, or any automatic actuation (3-way Claude/Codex/Gemini consensus). | cargo fmt --check + clippy + test --all-targets (1478 lib + 68 integration + 53 supporting, all green in 1.12s); evidence: insights_lifecycle_reports_action_metric_impact_windows + insights_next_best_action_picks_latest_unaccepted_strong_rec + compact_payoff_reports_saved_tokens_when_threshold_and_samples_pass + compact_payoff_reports_neutral_when_change_is_below_threshold + compact_payoff_reports_regressed_when_after_growth_is_higher + cache_summary_reports_cache_states_latest_ratio_token_growth_and_cost_delta + cost_breakdown_groups_window_cost_by_pane_model_and_situation + insights_read_only_snapshot_supports_old_schema_db + r2-plan synthesis slices A-I regression suite |
| v2.0.0  | Milestone marker + cross-cutting consistency sweep on top of the v1.60.0 polish baseline. Every operator-facing surface (alerts/panes/settings/metrics/anomaly/insights/help/fx/theme) is polished, the v1.58.0-deferred Light theme is closed by v1.60.0, and version surfaces (README/VERSION/PROJECT_BRIEF/mission ledger/CURRENT_STATE/package.json) align on v2.0.0. No code-behaviour change beyond a help-modal `/` prose tweak that documents the v1.59.0 dual semantic (Alerts focus → filter input; Panes focus → split-cycle). v2.0.0 is the operator-chosen marker for "every dashboard surface is polished" — not a semver breaking-change signal.                                                                                                                                                                                                                                                                                                                                                                                                 | cargo fmt --check + clippy + test --all-targets (1445 lib + 65 integration + 53 supporting); evidence: full v1.60.0 test suite preserved + the existing slash_key.contains("cycle") help test still passes against the widened `/` prose                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| v1.60.0 | Five-slice polish bundle (closes deferred-from-v1.58.0 Light theme): Insights overlay freshness chip + r-refresh toast; Metrics overlay CTX/QUOTA/CACHE delta arrows on the left column; pricing.toml staleness advisory (90-day mtime threshold); Help modal section jumping (1..N keys); Light theme variant + theme-aware accessor migration (153 sed-migrated call sites)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | cargo fmt --check + clippy + test --all-targets (1434 lib + 65 integration + 53 supporting); evidence: relative_age_label_covers_each_bucket_boundary + is_stale_flips_after_threshold_and_stays_false_without_snapshot + update_pressure_observation_first_call_seeds_values_with_none_trends + update_pressure_observation_emits_up_down_steady_against_prior_value + pricing_staleness_days_returns_age_when_mtime_predates_threshold + help_section_line_indices_returns_at_least_four_sections_in_order + theme_mode_cycle_covers_dark_high_contrast_light_in_order + light_theme_severity_colors_stay_distinct + dark_and_high_contrast_text_dim_match_legacy_constant                              |
| v1.59.0 | Five-slice polish bundle + sampler meta-effect: anomaly events row severity color coding; Settings dirty-row visual marker (green ● for modified-but-unsaved + existing yellow \* for differs-from-default); Alerts filter (`/` symmetric to v1.58.0 Settings filter); three new fx effects (snow / fireworks / plasma); Sampler meta-effect that auto-cycles every effect every 5s; refreshed README quickstart                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | cargo fmt --check + clippy + test --all-targets (1415 lib + 65 integration + 53 supporting); evidence: alert_filter_input_buffer_lifecycle + alert_filter_rejects_control_chars_but_keeps_unicode_letters + alert_filter_matches_title_headline_details_and_command + snow_spawns_full_field_and_keeps_count_stable_after_step + fireworks_explodes_rocket_when_burst_height_reached + plasma_step_advances_time_and_sample_returns_within_bounds + sampler_rotates_to_next_effect_after_interval_elapses + sampler_next_effect_skips_self_and_covers_every_other_variant + fx_scene_from_effect_dispatches_every_v1_59_variant + fx_snow / fireworks / plasma / sampler_smoke_renders_without_panic      |
| v1.58.1 | Hotfix on v1.58.0 Settings Parameters filter: arrow-key navigation now skips fields hidden by the active /-filter (jumps to first/last visible on a hidden selection, no-ops on empty match set) so the body no longer appears frozen                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | cargo fmt --check + clippy + test --all-targets (1400 lib + 65 integration + 53 supporting); evidence: next_parameter_skips_fields_filtered_out_by_active_filter + prev_parameter_skips_fields_filtered_out_by_active_filter + next_parameter_no_op_when_filter_matches_nothing                                                                                                                                                                                                                                                                                                                                                                                                                           |
| v1.58.0 | Five-axis polish bundle: hover_help dashboard expansion (divider/footer/version-badge); n-overlay row filter (`f` cycles All → Promoted → High); provider-honesty cost chip (Pending → `cost ? [pricing]`); Settings Parameters filter via `/`; theme high_contrast variant via `[ux] theme`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | cargo fmt --check + clippy + test --all-targets (1397 lib + 65 integration + 53 supporting); evidence: f_cycles_anomaly_row_filter_and_resets_scroll + anomaly_filter_matches_per_variant + every_topic_has_korean_and_english_help (covers new 3 dashboard topics) + existing CacheMetricStatus / Settings parameter pipeline tests green at the new variants                                                                                                                                                                                                                                                                                                                                            |
| v1.57.0 | Cross-cutting consistency sweep: README/VERSION/mission.yaml/PROJECT_BRIEF version surfaces aligned to v1.57.0; UI_MANUAL gains Q hotkey + decorative fx overlay subsection + IME drag-bar prose; fx dismiss hint corrected from "Q to toggle" to "Q opens fx"; footer key cluster advertises `Q fx`; REVIEW_GUIDE matrix reference refreshed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | cargo fmt --check + clippy + test --all-targets (1394 lib + 65 integration + 53 supporting); evidence: full prior test suite green at the new docs/ui-strings (no test delta — sweep is documentation + 2 UI-string fixes)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| v1.56.0 | Audit-doc polish bundle: provider-honesty cache chips (CacheMetricStatus enum, `cache ?` / `cache —` per-provider) + matrix fx backdrop dim + Insights Situations rendering regression guard + ARCHITECTURE cache-rule paragraph                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | cargo fmt --check + clippy + test --all-targets (1394 lib + 65 integration + 53 supporting); evidence: gemini_with_stats_but_no_cache_reads_is_structurally_unsupported + cache_chip_text_renders_per_provider_placeholder + metric_badge_line_emits_cache_placeholder_per_provider + metrics_cache_bar_derives_ratio_from_raw_cached_tokens + rendered_overlay_surfaces_populated_situations_section                                                                                                                                                                                                                                                                                                     |
| v1.55.0 | fx overlay smoothness: 60 FPS with halved per-frame velocities, tmux poll tick suspended while overlay is active, new defaults `[fx] text='~O~ Qmonster'` / `duration_secs=50`, `~` block-glyph added                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | cargo fmt --check + clippy + test --all-targets (1373 lib + 65 integration + 53 supporting); evidence: existing 30-test fx suite (banner*bounces_off*\*, confetti_step_decrements_life_and_applies_gravity, matrix_drops_off_screen_streams, fx_overlay_preserves_underlying_cells_outside_effect_glyphs, etc.) all green at the new constants                                                                                                                                                                                                                                                                                                                                                            |
| v1.54.0 | Non-destructive fx overlay: render banner/confetti/matrix on top of the existing dashboard instead of clearing the viewport; bottom-right dismiss hint rewritten as cell-by-cell non-space writes so the footer stays readable                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | cargo fmt --check + clippy + test --all-targets (1363 lib + 65 integration + 53 supporting); evidence: fx_overlay_preserves_underlying_cells_outside_effect_glyphs (regression guard pinning ≥1500/1920 sentinel cells survive after overlay render)                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| v1.53.0 | Decorative `[fx]` overlay: 3 selectable effects (bouncing rainbow banner / confetti burst / matrix rain) wired through Q hotkey, p-accept celebration, and idle screensaver; 8 Settings parameters; click/key dismiss; ~30 FPS only while open                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | cargo fmt --check + clippy + test --all-targets (1362 lib + 65 integration + 53 supporting); evidence: banner_bounces_off_left_edge + confetti_step_decrements_life_and_applies_gravity + matrix_drops_off_screen_streams + open_initializes_scene_matching_effect + celebration_trigger_forces_confetti_regardless_of_configured_effect + screensaver_auto_open_fires_after_idle_threshold + fx_banner_smoke_renders_without_panic                                                                                                                                                                                                                                                                       |
| v1.52.0 | Pane CLI version badge: parse-from-tail first, probe-via-/proc fallback, cached by (provider, pid, comm, argv, exe_path); Provider::Qmonster exempt; renders `CLI <version> [Official]` in panel title between role and pane id, silently omitted when unconfirmed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | cargo fmt --check + clippy + test --all-targets (1324 lib + 65 integration + 53 supporting); evidence: cli_version cache-key equality + tail-first-then-probe orchestrator branching tests + 2 integration tests for the parse-from-tail and /proc probe-fallback rendering paths                                                                                                                                                                                                                                                                                                                                                                                                                         |
| v1.51.0 | build.rs tag-cache fix + git modal origin/contributors + IME drag-bar indicator + Settings tab grouping + anomaly default-on flip                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | cargo fmt --check + clippy + test --all-targets (1317 lib + 63 integration + 53 supporting); evidence: normalize_origin_url_rewrites_ssh_to_https + parse_shortlog_strips_email_and_returns_count + non_ascii_letter_activates_and_signals_first_transition + split_divider_renders_ime_warning_when_active + settings_tab_strip_renders_inter_group_seam_between_parameters_and_rules + anomaly_config_explicit_disable_is_honoured                                                                                                                                                                                                                                                                      |
| v1.50.0 | Insights overlay async fetch + spinner placeholder                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | cargo fmt --check + clippy + test --all-targets (lib ~1278, integration 63); evidence: loading_state_renders_centered_spinner_and_text + set_snapshot_for_drops_stale_request_id + spawn_insights_load_emits_outcome_with_matching_request_id                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| v1.48.x | Token Insights Phase 8 + UI discoverability                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | cargo fmt --check + clippy + test --all-targets (1211 lib tests + integration suites)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| v1.47.0 | Phase 7 v3 (c): SQLite persistence (closes Phase 7)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | cargo fmt + clippy + test --all-targets (lib ~1200, integration 60); SBOM diff guard                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| v1.46.0 | Phase 7 v3 (a+b): per-kind promotion gate + n overlay                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | cargo fmt + clippy + test --all-targets (lib ~1162, integration 58); SBOM diff guard                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
