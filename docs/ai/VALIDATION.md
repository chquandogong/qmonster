# VALIDATION

- Version: v0.4.0
- Date: 2026-04-20 (round r2 reconciled)

This doc defines what "good" looks like for Qmonster at each phase, and
what reviewers (Codex, Gemini, and the human operator) should
specifically check. It is intentionally short; deeper rubric detail
lives in `REVIEW_GUIDE.md`. Every displayed metric must carry a
`SourceKind` label per `ARCHITECTURE.md` §"SourceKind taxonomy".

## Planning-phase gates (Phase 0)

- [ ] `mission.yaml` validates with mission-spec schema.
- [ ] `mission-history.yaml` has a `1.0.0` entry and (after r2) a
      `1.1.0` reconciliation entry.
- [ ] `.mission/CURRENT_STATE.md` is filled with real content for today
      (not the empty template).
- [ ] `docs/ai/*.md` are canonical (stable rules), not diaries.
- [ ] `CLAUDE.md`, `AGENTS.md`, `GEMINI.md` each ≤ ~60 lines and only
      route (no procedures, no architecture, no state).
- [ ] `.docs/claude/Qmonster-v0.4.0-2026-04-20-claude-plan-r1.md`
      exists and covers all 11 required sections.
- [ ] `.docs/codex/Qmonster-v0.4.0-2026-04-20-codex-crosscheck-r1.md`
      and `.docs/gemini/Qmonster-v0.4.0-2026-04-20-gemini-research-r1.md`
      exist with explicit verdicts.
- [ ] `.docs/final/Qmonster-v0.4.0-2026-04-20-claude-final-r2.md`
      exists and classifies every reviewer item (now / Phase 1 / Phase 2+ / rejected).
- [ ] Every provider lever in docs carries a `SourceKind`:
      `ProviderOfficial` / `ProjectCanonical` / `Heuristic` / `Estimated`.
- [ ] `cargo check` still produces the placeholder binary — no
      premature implementation.

## Phase 1 (Observe-first MVP) checks

- [ ] `tmux::PaneSource` returns `RawPaneSnapshot` via polling.
- [ ] `domain::IdentityResolver` resolves `(provider, instance, role,
pane_id)` with an `IdentityConfidence` level. Provider-specific
      recommendations are suppressed when confidence is `Low` or
      `Unknown`.
- [ ] `adapters/` never performs identity inference.
- [ ] Basic alert extraction in `policy/rules/alerts.rs`: input-wait,
      permission-wait, log-storm, repeated-output, verbose-answer,
      error-hint, **subagent-hint**.
- [ ] **Subagent detection warning** fires when the tail matches the
      detector vocabulary (provider-specific patterns; initially
      `Heuristic`) and tells the operator "token consumption may be
      delayed or missing in main stats".
- [ ] **Zombie pane / session re-attach**: on `pane_dead` transition,
      per-pane alert queue is drained and context-pressure warnings
      for that pane are reset. On session re-attach, stale alerts
      older than the re-attach moment are hidden.
- [ ] **Version drift detector**: on startup and on each operator-
      requested refresh, Qmonster captures CLI versions
      (`claude --version`, `codex --version`, `gemini --version`,
      `tmux -V` when available); on change, fires a `warning`-severity
      alert "re-verify `(official)` tags in `docs/ai/`". Runs only when
      the operator triggers it (consistent with
      `refresh.policy = manual_only`).
- [ ] Context / token / cost metrics are **display-only** in Phase 1;
      not used as gating signals. Each value carries a `MetricValue<T>`
      with `SourceKind` and is rendered with a `SourceKind` badge.
- [ ] `store/sink.rs` ships only `EventSink` trait + `NoopSink` +
      `InMemorySink`. **No SQLite. No archive persistence. No
      snapshot export.**
- [ ] Recommendations are explainable: every recommendation carries a
      human-readable `reason` and a `SourceKind`.
- [ ] Destructive automation is disabled by default (`recommend_only`).
- [ ] Safety precedence enforced at startup: attempted upward override
      of `actions.mode`, `allow_auto_prompt_send`,
      `allow_destructive_actions`, `refresh.policy` via env/CLI is
      ignored and logged as `risk`.
- [ ] Qmonster runtime writes only within `~/.qmonster/`. No writes to
      `CURRENT_STATE.md`, `mission.yaml`, provider config files, or
      source files.

## Phase 2 (Archive / Checkpoint) checks

- [ ] `store/sqlite.rs` initialized; audit DB stores metadata only.
- [ ] `store/archive_fs.rs` writes raw tails with preview/full split at
      the configured char threshold.
- [ ] `store/audit.rs` writer type signature cannot accept raw bytes
      (compile-time enforcement). Raw text cannot bleed into audit.
- [ ] Runtime checkpoints land in `~/.qmonster/snapshots/`, never in
      `.mission/CURRENT_STATE.md`.
- [ ] Retention job honors the 14-day default (config-driven).
- [ ] `.mission/CURRENT_STATE.md` refresh flow documented and exercised
      at least once as a **human** day-end action.

## Phase 3 (Policy Engine) checks

- [ ] A–G canonical rules each fire in a reproducible test fixture:
      log-storm, code-exploration, context-pressure, verbose-output,
      permission-wait, quota-tight, repeated-output.
- [ ] **Concurrent-work warning** across panes (Gemini G-11). Phase 3A
      ships a **project-level proxy**: fires when two or more Main/Review
      panes operate in the same `current_path` with recent output. The
      stricter "same file or git branch" trigger is deferred to Phase 3B
      or a later round (file-level) and Phase 4+ (git-branch-level).
- [ ] `aggressive_mode` only surfaces recommendations when
      `quota_tight = true` in config.
- [ ] Every rule carries a `SourceKind`; for `Heuristic` rules, a
      pointer to the community source is recorded.
- [ ] Recommendations may carry a `suggested_command: Option<String>`
      for copy-paste ergonomics. The value must be runnable on a single
      surface (shell command, in-pane slash-command, or `# config-edit …`
      comment pointer) — mixed-mode prose (e.g. TUI keybinding prose
      plus a slash-command) belongs in `next_step`, not here. Rendered
      by both the alert queue and `--once` with a ``run: `…``` prefix.
- [ ] Recommendations may carry a `next_step: Option<String>` — prose
      precondition that precedes the runnable `suggested_command`.
      Required for strong recs whose safe execution depends on a step
      that cannot be expressed on the same surface as the command
      (e.g. C-warning / C-critical need "press `s` to snapshot first"
      before running `/compact`). Rendered as `next: …` **before**
      ``run: `…``` so the ordering is inherent to the field layout.
- [ ] "Checkpoint before compact" surfaces as a **strong recommendation
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
- [ ] `token.thresholds` structure may split per-provider ONLY if
      Phase-1 fixture data justifies the split; otherwise keep one
      global block.
      (deferred — Phase-1 fixture-driven justification has not been
      measured yet; single global block currently stands)

## Phase 5 (Safer Actuation) checks

- [ ] Manual prompt-send helper requires explicit user confirmation.
- [ ] Every actuation is recorded in the audit log with approver, pane,
      command, and outcome (metadata only, no raw text).
- [ ] Destructive actions remain outside the automation surface. No
      code path exists for auto `/compact`, `/clear`, `/memory`
      mutation, provider reconfiguration.

## Non-functional checks (all phases)

- [ ] No giant always-loaded prompt file in CLAUDE.md, AGENTS.md, or
      GEMINI.md.
- [ ] Auto memory never holds the only copy of today's state.
- [ ] Refresh remains `manual_only` unless the operator explicitly
      changes policy.
- [ ] `logging.sensitivity` honors `balanced` by default.
- [ ] Color usage follows the low-saturation palette rule. No
      color-only state indication. Every color is accompanied by a
      numeric % or severity letter.
- [ ] `SourceKind` labels visible next to every metric and every
      recommendation — in the UI, not just in docs.
- [ ] Qmonster runtime never writes outside `~/.qmonster/`.
- [ ] Audit log and raw archive are separated at the writer level
      (type-level enforcement, not policy).

## Evidence expectations

- Phase-0 evidence: the planning bundle (mission artefacts +
  `docs/ai` + planning report + review docs + final synthesis).
- Phase-1+ evidence: reproducible pane fixtures (tail samples) + test
  output + screenshots (saved under `.docs/final/` for the round).
- Per-fixture: include the pane tail, the resolved identity (with
  confidence), the emitted signals (with `SourceKind`), and the
  triggered recommendations.
