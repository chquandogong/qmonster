# Design: Migrate pane data acquisition from tmux-scraping toward structured channels

- Date: 2026-06-29
- Author: Claude (Opus 4.8), brainstorming → design phase
- Status: **DESIGN — awaiting operator spec review before writing the implementation plan.**
- Raw research + verified on-disk schemas: `.docs/claude/Qmonster-2026-06-29-sdk-api-migration-research.md`
- Scope: **target C, sequenced A → B → C** (operator-approved).

## 1. Problem & goal

Qmonster reads most per-pane runtime signals by scraping `tmux capture-pane` output and parsing each
provider's rendered statusline/status box (`src/adapters/{claude,codex,gemini}.rs`). Scraping is fragile:
a provider statusline format change silently breaks a parser. The operator asked to move signal
acquisition off terminal-text-scraping onto official/structured channels (SDK/API/headless/session files)
where one exists, and to re-check what each CLI now exposes since the versions the code references.

Qmonster is **already hybrid**: it layers two structured channels (`claude_sidefile.rs` JSON,
`codex_app_server.rs` JSON-RPC) over the scrape with `is_none()` fallback. This work **widens that
structured layer**; it does not rip out scraping.

### Non-goals (explicitly out of scope)

- Full removal of scrape parsers. Each provider keeps ≥1 signal with no passive structured source.
- Monitor-launched channels: `claude -p --output-format stream-json`, Claude Agent SDK, `codex exec --json`
  — these only observe sessions Qmonster itself launches; Qmonster observes human-launched panes. Excluded.
- Live Codex app-server thread subscription (`thread/tokenUsage/updated`, `thread/status/changed`):
  confirmed unreachable across process boundaries for a session Qmonster did not launch (OpenAI issue #25914).
- Wiring an OpenTelemetry collector for Claude/Codex (per-process, heavy, opt-in) — deferred, not this work.
- agy quota / cost / context: no passive structured source exists (interactive `/usage` or the IDE language
  server only). Stays Hidden per the v2.4.0 six-gate ObserveOnly contract.
- No new actuation, no new audit kind, no SQLite schema change driven by this design.

## 2. Verified facts (this machine, 2026-06-29)

Installed: Claude Code **2.1.195**, codex-cli **0.142.3**, agy **1.0.13**. Real files sampled read-only.

- **Claude sidefile** (`~/.local/share/ai-cli-status/claude/<session>.json`) carries, structured:
  `context_window.{used_percentage, remaining_percentage, context_window_size, total_input_tokens,
total_output_tokens, current_usage{…}}`, `rate_limits.{five_hour,seven_day}.{used_percentage, resets_at}`,
  `model.{id, display_name}`, `version`, `effort.level`, `cost.{total_cost_usd, total_lines_added,
total_lines_removed, total_duration_ms, total_api_duration_ms}`, plus `thinking.enabled`, `fast_mode`,
  `output_style.name`, `session_name`, `workspace.{…,repo}`, `exceeds_200k_tokens`.
  - `ClaudeSidefile` struct currently extracts only `resets_at` + `current_usage` token counts → the rest
    is dropped by serde and re-derived from the scrape.
  - **`permission_mode` is NOT present** in the sampled sidefile → keep scraping it.
- **Codex rollout** (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`), envelope `{type, payload}`:
  `session_meta.payload.{cli_version, cwd, model_provider, originator, git, session_id, thread_source}`;
  `turn_context.payload.model`; `event_msg` `payload.type=="token_count"` →
  `payload.info.{total_token_usage{input,cached_input,output,reasoning_output,total}, last_token_usage{…},
model_context_window}`; `event_msg` `task_started`/`task_complete`.
  - `model_context_window` is inline → context% derivation needs no static per-model table.
  - rate-limits are NOT in rollout (stay on app-server); awaiting-input has no clean flag.
- **agy transcript** (`~/.gemini/antigravity-cli/brain/<uuid>/.system_generated/logs/transcript.jsonl`):
  `{step_index, source, type, status, created_at, content, [tool_calls], [thinking], [truncated_fields]}`.
  - **No token / model / usage / cost / context fields anywhere** (verified key-name scan: 0 hits).
  - **No top-level `cwd`** → pane↔conversation correlation is the hard part (use `history.jsonl` / tool-arg paths).

## 3. Core architectural decision: source precedence (APPROVED)

Today the structured layer fills with an `is_none()` guard (scrape wins; structured only fills gaps). For
fields the scrape always populates (CTX%, model), structured would never win — defeating the goal.

**Decision: for the _migrated_ fields, invert to "structured-preferred, scrape-fallback."** When the
structured channel yields a value, it overrides the scraped value; scrape fills only when the structured
value is absent. `SourceKind` stays `ProviderOfficial` (same authority, higher precision/robustness). This
mirrors the existing `cache_hit_ratio` override precedent (sidefile precise ratio already beats the rounded
`cache N%` scrape). Fields with no structured source (Claude `permission_mode`, Codex awaiting-input) stay
scrape-authoritative.

Implementation shape: a small per-field merge helper `prefer_structured(scraped, structured)` (returns the
structured `MetricValue` when `Some`, else the scraped one), applied at the adapter enrichment seam, so the
precedence rule is explicit and unit-testable rather than scattered across `is_none()` checks.

## 4. Slice A — Claude sidefile extension (lowest risk, highest ROI)

**Module:** `src/adapters/claude_sidefile.rs` (+ enrichment seam in `src/adapters/mod.rs`).

1. Extend `ClaudeSidefile` serde structs to deserialize the verified fields:
   - `ClaudeSidefileContextWindow`: add `used_percentage: Option<f64>`, `context_window_size: Option<u64>`,
     `total_input_tokens`, `total_output_tokens` (Option<u64>).
   - `ClaudeSidefileRateWindow`: add `used_percentage: Option<f64>` (alongside existing `resets_at`).
   - Add `model: Option<{ id, display_name }>`, `version: Option<String>`, `effort: Option<{ level }>`,
     `cost.{total_lines_added, total_lines_removed}` (Option<u64>). Optional bonus
     (`thinking.enabled`, `fast_mode`, `session_name`) only if a consumer surface uses them — else skip (YAGNI).
2. At the Claude enrichment seam, apply **structured-preferred** for: `context_pressure`
   (= `context_window.used_percentage / 100`), `quota_5h_pressure` (= `five_hour.used_percentage / 100`),
   `quota_weekly_pressure` (= `seven_day.used_percentage / 100`), `model_name`, `reasoning_effort`,
   `cost_usd`, and surface `context_window_size` where the UI shows it. All `SourceKind::ProviderOfficial`.
3. **Keep scraping** `permission_mode` (absent from sidefile) and any field a given sidefile omits.
4. Honesty: when the operator has not installed the sidefile-export `statusline.sh`, `read_sidefile_for_path`
   returns `None` and everything falls back to scrape exactly as today — no regression for non-sidefile users.

**Tests:** extend `read_sidefile_parses_full_real_world_shape` with the verified fields; add precedence unit
tests (structured value wins when present; scrape wins when sidefile field absent); add a real masked
fixture under `tests/fixtures/` sanitized from this machine's sidefile.

## 5. Slice B — Codex rollout-file tailer (medium risk)

**New module:** `src/adapters/codex_rollout.rs` (pure reader, modeled on `claude_sidefile.rs`).

1. **Discovery + pane correlation:** find candidate `~/.codex/sessions/**/rollout-*.jsonl` (honor
   `CODEX_HOME`), match to a pane by `session_meta.cwd == current_path`, newest-mtime tie-break (same
   contract as the sidefile reader). Bound the scan (e.g. only recent date partitions / cap candidates) so
   a deep session history never makes a poll expensive.
2. **Parse (defensive, version-gated on `session_meta.cli_version`):**
   - `turn_context.payload.model` → `model_name` (`ProviderOfficial`).
   - latest `token_count` `payload.info` → token usage (`total_token_usage` + `last_token_usage`) and
     `model_context_window`.
   - `task_started` / `task_complete` ordering → working/idle state.
3. **Context% derivation + calibration:** compute a candidate context ratio from `last_token_usage` vs
   `model_context_window`; **calibrate the exact formula against the existing scraped "Context %"** on real
   data before trusting it. If they disagree beyond tolerance, keep the scrape and log — never publish a
   miscalibrated number. (Deliberate calibration task, not a guess.)
4. **Precedence:** structured-preferred for `model_name`, token counts, `context_pressure` (once calibrated),
   working/idle. **app-server stays the sole source for rate limits** (`account/rateLimits/read`, unchanged).
   **awaiting-input stays scrape/heuristic** (no clean rollout flag).
5. **Config:** new `[provider_setup] codex_rollout` toggle, **default-on** (operator-approved — read-only,
   no subprocess, cheap). Disable path returns to pure scrape + app-server.
6. **Stale-reference fixes (carried here):** `codex_app_server.rs` doc-comment "0.125.0/0.128.0" → note
   0.142.x and the `conversation/*`→`thread/*` rename (our `initialize` + `account/rateLimits/read` calls are
   unaffected; the notification-skip loop remains correct).

**Tests:** real masked `rollout-*.jsonl` fixture (sanitized from this machine); unit tests for discovery,
cwd correlation, token_count parse, model parse, task_started/complete → idle/working, context% calibration
(asserting agreement with a known scraped value), version-gate fallback on an unrecognized `cli_version`,
and defensive parse on a truncated/old-format line.

## 6. Slice C — agy enricher + refresh

### C1 — Documentation / version refresh (safe; always done)

- `docs/ai/ARCHITECTURE.md`: provider-coverage matrix + version sync (Claude 2.1.x sidefile-derived fields,
  Codex rollout channel, agy transcript ceiling); note the new structured-preferred precedence rule.
- `docs/ai/UI_MANUAL.md` (Korean): Provider Setup agy tab + Codex rollout toggle description.
- `codex_app_server.rs` version strings (also touched in Slice B).

### C2 — agy transcript-tail activity enricher (optional, gated, probe-first)

- New reader for `~/.gemini/antigravity-cli/brain/<uuid>/.system_generated/logs/transcript.jsonl`.
- **Surfaces only:** working/idle (from `created_at` recency / step progression), activity kind
  (`type`/`tool_calls[].name`), conversation id. **No token/context/quota/cost** — verified absent; the
  v2.4.0 six ObserveOnly gates (`AnomalyKind::supports_provider`, `provider_honesty` Hidden, profile_switch
  None, insights `unsupported`, token-sample filter, dispatch via `common::parse_common_signals`) stay intact.
- **Correlation:** no `cwd` in transcript → resolve the active conversation via
  `~/.gemini/antigravity-cli/history.jsonl` and/or workspace paths inside `tool_calls[].args`. Treat schema
  as unstable/reverse-engineered; probe a live file and parse defensively; if correlation is ambiguous,
  surface nothing (honest) rather than guess.
- **Config:** new `[provider_setup] agy_transcript` toggle, **default-off** (unstable schema, opt-in).
- This graduates the v2.4.0 follow-up only partially — it is "activity observation," not the "documented
  headless companion" the follow-up awaits; the follow-up note stays open for the real API.

**Tests:** real masked `transcript.jsonl` fixture; unit tests for activity/idle derivation and the
"correlation ambiguous → emit nothing" honesty path. If correlation proves unreliable on real data, **C2 may
be dropped to a deferred follow-up and only C1 ships** — decided during implementation against real files.

## 7. Cross-cutting

- **ObserveOnly / safety precedence / no actuation:** unchanged across all slices.
- **SourceKind:** every migrated value keeps an explicit `SourceKind` (sidefile/rollout = `ProviderOfficial`;
  derived context% = `ProviderOfficial` if calibrated to the provider number, else `Estimated`).
- **Filesystem read boundary:** adds read access to `~/.codex/sessions` and `~/.gemini/antigravity-cli`.
  Precedent exists (Qmonster already reads `~/.claude/*` and `~/.local/share/ai-cli-status/*`). Strictly
  read-only; the write boundary (Qmonster root only) is unchanged. Document the new read paths.
- **Config surface:** `[provider_setup]` gains `codex_rollout` (default true) and `agy_transcript`
  (default false), alongside existing `claude_sidefile` / `codex_app_server`.
- **TDD + real fixtures:** every parser is built against a masked real file from this machine, version-gated,
  defensively parsed (schema drift is expected, especially Codex/agy).
- **Review gates:** per project convention — Codex + Gemini cross-check per slice; `.docs/<model>/` narrative
  - `.mission/evals/` mirror; version bump per slice; `cargo fmt`/`clippy -D warnings`/`test --all-targets`
    green before each tag.

## 8. Sequencing & version mapping (proposal)

- **A** → one minor (Claude sidefile extension + precedence helper). Safe to ship first.
- **B** → one (or two) minor(s) (rollout reader; calibration may warrant its own follow-up patch).
- **C1** → folds into the A or B ledger-sync, or its own doc patch.
- **C2** → one minor _if_ real-file validation succeeds; else a deferred follow-up.
  Exact version numbers assigned at implementation time against the live ledger.

## 9. Risks & open items

- **Codex context% formula** — must be calibrated against the scrape on real data (Task in §5.3). Highest
  technical uncertainty in the plan.
- **agy correlation** — no cwd in transcript; may prove too unreliable for honest per-pane attribution → C2
  is explicitly droppable.
- **Schema drift** — Codex rollout and agy transcript are reverse-engineered/undocumented; version-gating +
  defensive parsing + "emit nothing when unsure" are mandatory, not optional.
- **Sidefile dependency** — Slice A's win requires the operator's recommended `statusline.sh`; non-users see
  no change (graceful, already the case).

## 10. Done-when (high level; full done_when in mission.yaml at implementation)

- Claude panes source CTX% / 5h% / 7d% / model / effort / cost from the sidefile JSON when present, scrape
  otherwise; permission_mode still scraped; SourceKind correct; no regression for non-sidefile users.
- Codex panes source model + token usage + working/idle from the rollout file when present; context% either
  calibrated-and-preferred or left to scrape; rate limits still from app-server; `codex_rollout` toggle works.
- Stale Codex version/protocol references corrected; provider-coverage matrix + UI_MANUAL updated.
- (If C2 ships) agy panes show transcript-derived activity/idle only; all analytic surfaces stay Hidden.
- Each slice: fmt + clippy + full test suite green; real masked fixtures committed; reviewer gates passed.
