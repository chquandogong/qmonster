# 2026-05-07 - Token Insights overlay (design)

Status: design accepted, implementation plan pending.
Audience: Qmonster TUI maintainers and Claude/Codex/Gemini implementers.
Baseline: `main` at `160fba1` (v1.42.0 publication verified) with
existing local `.mission/CURRENT_STATE.md` edits left untouched.
Merged input: Gemini's
`docs/superpowers/specs/2026-05-07-optimization-overlay-design.md`.

This design adds an operator-facing insight surface for Qmonster's
second core axis: token optimization. The feature is not another raw
metrics screen. It explains which token-pressure situations occurred,
which recommendations Qmonster emitted, what the operator or system did
next, and which recommendations were ignored.

## 1. Goal

Add a new `i` (Insights) overlay that answers four operator questions:

1. What token-optimization situations is Qmonster seeing now?
2. Which situations happened most often over a selected time window?
3. What action followed each recommendation: accepted, rejected,
   blocked, archived, snapshotted, hidden, auto-snapshotted, or ignored?
4. What can Qmonster honestly claim about token/cache/cost trends, and
   what remains unknown?

The first version is operations-centered. It can also support demos and
development validation through an Evidence tab, but those are secondary
uses.

## 2. Product Decision

Use a new `i` overlay instead of adding more rows to the existing `m`
Metrics overlay.

Rationale:

- `m` should stay a pressure/telemetry screen: CTX, quota, cache,
  tokens, memory, cost, reset ETA, anomalies.
- `i` should explain decisions and outcomes: signal -> recommendation
  -> action -> outcome.
- Keeping them separate avoids overcrowding the metrics overlay and
  gives ignored/blocked actions enough space to be legible.

The first implementation should still prove the data path with a
read-only query/report slice before the full TUI overlay lands.

Gemini draft decisions reviewed:

- Accepted: the Overview tab should use a simple three-part shape:
  top token/cache/cost summary, middle six-situation counters, bottom
  recent actions feed.
- Accepted with adjustment: cache hit ratio trend can use sparklines,
  but the label must be cache reuse/trend, not exact token savings.
- Accepted with adjustment: SQLite queries must not block rendering.
  The final design uses a cached `InsightsSnapshot` refreshed outside
  the `ui/` render function rather than requiring async runtime wiring
  in v1.
- Not accepted: `o` key and `optimization_*` module names. The accepted
  product is an Insights surface, so the key and module names stay
  `i`, `app/insights_overlay.rs`, and `ui/insights.rs`.

## 3. Non-goals

- Exact "tokens saved" claims. Qmonster can show measured token/cache
  trends and estimated opportunity, but it must not claim exact savings
  unless a provider exposes a reliable counter.
- Per-subagent token attribution. This remains blocked until providers
  expose structured per-subagent counters.
- Auto `/compact`, `/clear`, `/memory`, or provider setup writes.
  Existing manual approval and prompt-send policies stay unchanged.
- Replacing `m`. The Insights overlay complements Metrics; it does not
  absorb the metrics overlay.
- Storing raw pane tails in SQLite. The existing audit isolation rule
  remains intact.
- Editing provider configuration files from Qmonster. Provider Setup
  remains copy/manual.

## 4. User Stories

- As an operator, I want to see which of the six original
  token-optimization situations happened most, so I know where the
  workflow is wasting context.
- As an operator, I want to see whether recommendations led to action
  or were ignored, so I can tune thresholds and behavior.
- As an operator, I want cache state shown alongside context pressure,
  so I can avoid compacting while cache is hot and compact when cache is
  cold or drifting.
- As a maintainer, I want the Evidence tab to connect design docs,
  source files, signals, and rules, so a demo can prove that the
  shipped behavior matches the architecture.

## 5. Situation Taxonomy

Keep the original six situation categories from the token-optimization
plan. Treat cache as a cross-cutting decision axis rather than a seventh
situation.

| Situation | Signals | Implemented rule surface | Typical outcomes |
| --- | --- | --- | --- |
| A. Log storm / repeated output | `log_storm`, `repeated_output`, `output_chars`, raw archive effect | `policy/rules/alerts.rs`, `policy/rules/advisories.rs`, `RequestedEffect::ArchiveLocal`, `store/archive_fs.rs` | archive, summarize, hide, ignored |
| B. Code exploration | `TaskType::CodeExploration`, `verbose_answer`, `active_files`, cross-pane edit findings | `policy/rules/advisories.rs`, `policy/rules/concurrent.rs`, `policy/rules/anomaly.rs` | delegate, graph/symbol advice, cross-pane warning, ignored |
| C. Context pressure | `context_pressure`, token IO, cache ratio, reset ETA | `policy/rules/advisories.rs`, `policy/rules/cache.rs`, `policy/rules/reset.rs`, `app/auto_snapshot.rs` | snapshot, `/compact`, wait for reset, auto-snapshot, ignored |
| D. Verbose review | `Role::Review`, `verbose_answer` | `policy/rules/advisories.rs`, `policy/rules/profiles.rs` | terse profile, copy/config hint, ignored |
| E. Input / permission wait | `IdleCause::InputWait`, `PermissionWait`, `LimitHit` | `policy/rules/idle.rs` | operator response, blocked, elapsed response time, ignored |
| F. Quota-tight / cost | split quota, `quota_pressure`, `cost_usd`, error history | `policy/rules/advisories.rs`, `policy/rules/cost_budget.rs`, `policy/rules/profile_switch.rs`, `policy/rules/profiles.rs` | pace, profile switch, pause, budget alert, ignored |

Cache axis:

| Cache state | Signal | Existing rule | Insight question |
| --- | --- | --- | --- |
| Hot cache | hit ratio above hot threshold while CTX has headroom | `cache: avoid /compact while cache is hot` | Did the operator wait, or compact anyway? |
| Cold cache | hit ratio below cold threshold while CTX is filling | `cache: /compact is safe - cache is cold` | Did cold-cache opportunities lead to compaction? |
| Cache drift | hit ratio dropped over recent samples | `cache: drift detected - /compact will let cache rebuild` | Which workloads cause cache drift most often? |

## 6. Existing Data Sources

Reuse existing durable tables first:

- `token_usage_samples`: `pane_id`, provider, cumulative
  `input_tokens`, `output_tokens`, `cached_input_tokens`, and `cost_usd`.
- `audit_events`: `RecommendationEmitted`, `AlertFired`,
  `ArchiveWritten`, `SnapshotWritten`, `PromptSendProposed`,
  `PromptSendAccepted`, `PromptSendRejected`, `PromptSendCompleted`,
  `PromptSendFailed`, `PromptSendBlocked`, runtime refresh events, and
  safety/config events.
- `cost_usage_events`: positive USD deltas by pane and provider.
- `cost_budget_alerts`: one-shot 80% and 100% threshold claims.

These sources can already support a basic read-only report:

- recommendation frequency by `action`
- alert/recommendation frequency by severity/provider/pane
- token/cache/cost trends by pane
- prompt-send terminal outcomes
- snapshot/archive events

They cannot honestly infer all lifecycle outcomes. Hidden/dismissed and
TTL ignored need a small new ledger.

## 7. New Storage

Add one focused lifecycle surface in SQLite with two new tables:
`recommendation_events` and `recommendation_outcomes`.

### `recommendation_events`

One row per emitted recommendation after edge/dedup policy has decided
it is visible to the operator.

Fields:

- `id INTEGER PRIMARY KEY`
- `ts_unix_ms INTEGER NOT NULL`
- `pane_id TEXT NOT NULL`
- `provider TEXT`
- `role TEXT`
- `situation TEXT NOT NULL`
- `action TEXT NOT NULL`
- `severity TEXT NOT NULL`
- `source_kind TEXT NOT NULL`
- `reason_summary TEXT NOT NULL`
- `suggested_command TEXT`
- `is_strong INTEGER NOT NULL`
- `dedup_key TEXT NOT NULL`
- `threshold_snapshot_json TEXT`

`dedup_key` should be stable enough to correlate the same logical
recommendation across poll ticks, e.g. `{pane_id}:{action}` with
situation-specific additions only when needed.

### `recommendation_outcomes`

One row per lifecycle outcome.

Fields:

- `id INTEGER PRIMARY KEY`
- `ts_unix_ms INTEGER NOT NULL`
- `recommendation_event_id INTEGER`
- `pane_id TEXT NOT NULL`
- `action TEXT NOT NULL`
- `outcome TEXT NOT NULL`
- `audit_event_id INTEGER`
- `summary TEXT NOT NULL`

Allowed `outcome` values:

- `accepted`
- `rejected`
- `completed`
- `failed`
- `blocked`
- `archived`
- `snapshot_written`
- `auto_snapshot_written`
- `hidden`
- `ignored`

Use existing `audit_events` ids where available. UI-only hide/dismiss
should be recorded as a lightweight outcome instead of being guessed
from ephemeral dashboard state.

## 8. Ignored Classification

Ignored is a Qmonster classification, not a provider fact.

Default rule:

- If a recommendation has no correlated outcome within a configured TTL,
  classify it as `ignored`.
- Initial default TTL: 30 minutes.
- Session end may also close still-open recommendations as ignored.
- The UI must label this as `TTL ignored`, not as an operator-confirmed
  rejection.

Correlation should start conservative:

- Same `pane_id + action` for recommendation-specific outcomes.
- Prompt-send outcomes correlate through `proposal_id` where present.
- Snapshot/archive outcomes correlate to recent context/log/reset
  recommendations on the same pane.
- Auto-snapshot outcomes correlate to `snapshot before ... resets`
  recommendations and carry the existing
  `trigger=auto_reset_boundary` summary metadata.

If correlation is ambiguous, do not attach the outcome. Leave the
recommendation pending until TTL, then classify it as ignored.

## 9. Insights Query Layer

Add `src/store/insights.rs`, a read-only query module that turns SQLite
rows into UI-ready aggregates.

Core structs:

- `InsightsWindow { since_ms, until_ms }`
- `SituationSummary { situation, current_count, window_count,
  outcome_counts, top_actions }`
- `RecommendationTimelineItem { ts, pane_id, provider, situation,
  signal_summary, action, severity, source_kind, outcome }`
- `CacheInsightSummary { hot_count, cold_count, drift_count,
  followed_count, ignored_count, avg_cache_ratio, token_growth }`
- `ActionLedgerRow { action, emitted, accepted, rejected, blocked,
  completed, failed, hidden, ignored }`

The query layer is pure read/aggregation code over SQLite. It should not
depend on ratatui.

The TUI render path must not run SQLite queries directly. The app layer
owns an `InsightsRuntimeState` containing the most recent
`InsightsSnapshot`, a refresh timestamp, and an optional error summary.
Opening the overlay or changing the time window requests a refresh; the
renderer consumes the cached snapshot only. The first implementation may
perform the refresh synchronously in the event loop if the query is
bounded and cheap, but the `ui/` module must remain pure. If fixture
data shows refresh cost can exceed the poll budget, move the refresh to
a small background worker before shipping the TUI slice.

## 10. UI Design

New key:

- `i`: open/close Token Insights overlay.
- Existing overlay close behavior: `i`, `Esc`, `q`, `[x]`.

Tabs:

1. **Overview**
   - Time window selector label: last 1h / 24h / session / all.
   - Three vertical sections:
     top token/cache/cost summary, middle six-situation matrix, bottom
     recent actions feed.
   - Summary labels: "cache reuse", "token growth", "cost delta", and
     "estimated opportunity"; never "total token savings".
   - Cache hit ratio trend sparkline over the selected window.
   - Hottest current pane link back to metrics.
2. **Situations**
   - Situation -> signals -> rule modules -> outcomes.
   - Shows design label and implementation status.
3. **Timeline**
   - `signal -> recommendation -> action -> outcome`.
   - Filters by pane/provider/situation/outcome.
4. **Action Ledger**
   - Counts by action and outcome.
   - Explicit `ignored` column.
5. **Evidence**
   - Design docs: `docs/ai/PROJECT_BRIEF.md`,
     `docs/ai/ARCHITECTURE.md`, `docs/ai/UI_MANUAL.md`,
     `mission.yaml`.
   - Implementation: `src/domain/signal.rs`,
     `src/policy/rules/*.rs`, `src/store/*.rs`,
     `src/ui/metrics.rs`, and the new insights UI modules.
   - SourceKind legend and threshold snapshots.

The UI should be dense and operational, not marketing-like. Use tables,
small bars, tabs, and concise labels. Keep text wrap and line truncation
consistent with the existing overlay patterns.

## 11. CLI / Report Proof

Before shipping the TUI overlay, add a CLI/report proof so real SQLite
data can validate the model.

Command:

- `qmonster insights --since 24h`

Report sections should mirror the overlay:

- situation summary
- cache summary
- action ledger
- recent timeline
- evidence/source labels

This gives demos and reviewers a stable text artifact before the TUI
rendering work begins.

## 12. Implementation Slices

1. **Read-only insights query layer**
   - Add `store/insights.rs`.
   - Aggregate existing `audit_events`, `token_usage_samples`, and
     `cost_usage_events`.
   - Verify with fixture SQLite databases.
2. **Recommendation lifecycle ledger**
   - Add schema and writer for recommendation emission/outcomes.
   - Record hide/dismiss outcomes and TTL ignored classifier.
   - Correlate prompt-send, snapshot, archive, and auto-snapshot
     outcomes conservatively.
3. **CLI/report proof**
   - Add `qmonster insights --since ...` or `--once --insights`.
   - Add golden text tests.
4. **TUI `i` overlay**
   - Add `app/insights_overlay.rs`, `ui/insights.rs`,
     `InsightsRuntimeState`, key dispatch, help footer chip, and help
     overlay row.
   - Reuse existing overlay geometry patterns where practical.
5. **Docs and validation**
   - Update `UI_MANUAL`, `ARCHITECTURE`, `VALIDATION`, README key list,
     and config example if a TTL config is added.

## 13. Configuration

Add a small `[insights]` config section with the lifecycle ledger slice:

```toml
[insights]
ignored_ttl_secs = 1800
default_window_secs = 86400
```

Defaults:

- `ignored_ttl_secs = 1800` (30 minutes)
- `default_window_secs = 86400` (24 hours)

The read-only query/report slice may ship before this config exists
because it does not classify ignored outcomes. Once ignored
classification lands, the TTL must come from `[insights]` and the
Settings overlay should expose it under Parameters/Rules.

## 14. Honesty Rules

The overlay may claim:

- exact recommendation counts from persisted recommendation rows
- exact action outcomes when an audit/outcome row exists
- `TTL ignored` classifications
- measured token/cache/cost trends from persisted samples
- estimated cost from Qmonster pricing config, already labeled
  `Estimated`

The overlay must not claim:

- exact tokens saved
- per-subagent token attribution
- that ignored means deliberate operator rejection
- that absent provider fields are zero
- that heuristic signals are provider official

## 15. Error Handling

- SQLite read failure: show a Warning system notice and render an empty
  overlay state with the error summary.
- Slow SQLite refresh: keep rendering the previous `InsightsSnapshot`
  and show a stale-data badge with the last successful refresh time.
- Missing lifecycle table during first migration: create it through the
  normal `AuditDb::open` schema path.
- Correlation ambiguity: do not attach the outcome; let TTL classify.
- Clock skew or invalid timestamps: exclude affected rows from windowed
  aggregates and show a small Evidence warning count.
- Large histories: all queries must accept `LIMIT` and bounded time
  windows; overlay should default to 24h.

## 16. Test Plan

| Test | Kind | Asserts |
| --- | --- | --- |
| `insights_query_empty_db_returns_zero_state` | unit | Empty DB yields empty summaries, no panic. |
| `insights_counts_recommendations_by_situation` | unit | Fixture recommendation rows aggregate into six situation buckets. |
| `cache_hot_cold_drift_aggregate_from_actions` | unit | Cache actions map to hot/cold/drift counts. |
| `prompt_send_outcomes_correlate_by_proposal_id` | unit | Accepted/completed/blocked/rejected rows attach to the right recommendation. |
| `snapshot_outcome_correlates_to_context_pressure` | unit | SnapshotWritten near a context/reset rec becomes `snapshot_written`. |
| `ttl_marks_unresolved_recommendation_ignored` | unit | No outcome after TTL creates `ignored`. |
| `hidden_outcome_is_recorded_not_inferred` | unit | Hide/dismiss writes a lifecycle outcome row. |
| `insights_report_renders_action_ledger` | integration/golden | Text report contains emitted/accepted/ignored counts. |
| `insights_snapshot_refresh_not_in_ui_renderer` | unit | `ui/insights.rs` renders from an `InsightsSnapshot` and does not depend on SQLite types. |
| `insights_overlay_stale_snapshot_badge` | UI unit | Failed/slow refresh preserves prior snapshot and renders stale/error status. |
| `insights_overlay_key_toggles` | UI unit | `i` opens and closes the overlay. |
| `insights_overlay_lines_do_not_overflow` | UI unit | Narrow viewport truncates/wraps without overflow. |

## 17. Rollout

Recommended version path:

- Phase 8 v1: read-only query layer + report proof.
- Phase 8 v2: lifecycle ledger + ignored classifier.
- Phase 8 v3: `i` overlay.

This avoids building a large UI before validating the data. If the
report proof shows that inferred outcomes are too weak, the lifecycle
ledger can be expanded before any TUI commitments.

## 18. Browser Brainstorm Artifact

The design was reviewed with the visual companion at
`.superpowers/brainstorm/2789993-1778124593/`. Those files are local
brainstorm artifacts and are not part of the shipped product surface.
