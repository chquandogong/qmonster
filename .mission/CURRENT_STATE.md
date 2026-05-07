# CURRENT_STATE

_Last updated: 2026-05-07 (Claude, v1.48.0 release ledger sync)_

## Mission

- Title: Qmonster v1.48.0 - Token Insights Phase 8: `qmonster insights --since` CLI report (v1) + recommendation lifecycle ledger / TTL ignored / `i` overlay (v2).
- Version surfaces: mission ledger target `1.48.0`; npm package metadata `qmonster@1.48.0`; latest local Git tag `v1.48.0`.
- Branch / worktree at handoff start: `main`, tag `v1.48.0`.
- Release publication state: v1.47.0 is published. `Release and Package Mirror` workflow run `25490555532` (2026-05-07, 7m16s, success) created GitHub Release `v1.47.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.47.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`), v1.42.0 (`25472444159`), v1.43.0 (`25474748447`), v1.44.0 (`25476534645`), v1.45.0 (`25478893257`), v1.46.0 (`25485133895`) publications also remain live — `npm view qmonster versions` lists `1.37.0` through `1.47.0` with `dist-tags.latest = 1.47.0`. v1.48.0 CI publication is a pending external follow-up.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, Phase 7 v1 anomaly observation surface, Phase 7 v2 promotion, Phase 7 v2 detectors, Phase 7 v3 (a+b), Phase 7 v3 (c), Phase 8 v1 token insights query layer, and Phase 8 v2 recommendation lifecycle ledger are complete.

## v1.48.0 Feature State

Token Insights Phase 8 ships the read-only SQLite query layer + `qmonster insights --since` CLI (Phase 8 v1) and adds the live-runtime recommendation lifecycle ledger + TTL-based ignored classifier + `i` overlay (Phase 8 v2). It does not change provider adapters, the audit chain, the `SignalSet` schema, or the existing v1.47.0 anomaly tables. The work landed across many parallel-track commits between v1.47.0 publication and the v1.48.0 ledger sync; none of the Phase 7 v3 (c) artifacts (`anomaly_events`, `anomaly_history_snapshots`, the `n` overlay) were touched.

1. **`SqliteInsightsStore`** (`src/store/insights.rs`): Read-only SQLite aggregator over the existing `audit_events`, `token_usage_samples`, `cost_usage_events` tables. Builds an `InsightsSnapshot` with six-situation recommendation aggregation, cache trend (read vs create), token growth, USD cost delta, action ledger, and recommendation timeline. `open_read_only(&path)` shares the audit DB without holding a writer lock.

2. **`InsightsSnapshot` + helper types** (`src/store/insights.rs`): `InsightsWindow`, `SituationSummary`, `CacheInsightSummary`, `ActionLedgerRow`, `RecommendationTimelineItem`, plus `snapshot_with_ignored_ttl(window, ttl_secs)` for TTL-aware aggregation. The store distinguishes live colon-prefixed actions (e.g. `prompt_send: /compact`) from generic alert actions and counts them in their canonical situation buckets.

3. **`InsightsReport` formatter** (`src/insights_report.rs`): `parse_since_arg`, `resolve_insights_paths`, `format_insights_report_lines`, `empty_insights_snapshot`. Renders a stable text report (label/value pairs, deterministic line order) suitable for `--once`-style operator consumption. Handles missing-DB and empty-window cases gracefully.

4. **`qmonster insights --since` CLI** (`src/main.rs`): New `CliCommand::Insights { since }` runs the report path before `build_startup_runtime`, so report generation does not require tmux. Parses ranges like `24h`, `30m`, `7d`. Read-only — never writes.

5. **`recommendation_events` + `recommendation_outcomes` SQLite tables** (`src/store/recommendation_lifecycle.rs`): New schemas added inside `AuditDb::open` for the lifecycle ledger. `recommendation_events` records every emitted Recommendation with provider, role, design situation, action, source label, reason, suggested command, strong-rec flag, and threshold snapshot. `recommendation_outcomes` records prompt-send accept/reject/block/complete/fail, archive writes, manual snapshots, auto-snapshots, and alert-hide events; `insert_correlated_recommendation_outcome` ledger-links each outcome to the latest matching prior event when one exists, suppressing the TTL "ignored" classifier for the same `(pane_id, action)` when correlation succeeds.

6. **`SqliteRecommendationLifecycleSink`** (`src/store/recommendation_lifecycle.rs`): Wraps `AuditDb`, mirroring the existing token/cost/anomaly sink pattern. Used by the live runtime path; opened alongside other sinks at startup.

7. **`Context.recommendation_lifecycle_sink`** (`src/app/bootstrap.rs`): New optional sink field plumbed via `with_recommendation_lifecycle_sink` builder. None when the DB couldn't be opened — graceful degradation to no-persistence.

8. **`run_once_with_target` lifecycle write path** (`src/app/event_loop.rs`, `src/app/insights_lifecycle.rs`): Each emitted `Recommendation` is recorded in `recommendation_events` from the live runtime path. Strong prompt-send recommendations are ledgered by their slash command (`/compact`, etc.) so later operator outcomes correlate with the actual actuation. Outcome paths (prompt-send accept/reject/block/complete/fail; archive write; manual `s`; auto-snapshot; alert hide) write `recommendation_outcomes` via `insert_correlated_recommendation_outcome`.

9. **TTL-based ignored classifier** (`src/store/recommendation_lifecycle.rs`, `src/store/insights.rs`): A recommendation is "ignored" when no correlated outcome appears within `[insights] ignored_ttl_secs` (default 1800 = 30 minutes). When an unlinked outcome IS observed for the same `(pane_id, action)` afterwards, the ignored classification is suppressed for that pair so manual `/compact` etc. don't get over-counted as ignored.

10. **`InsightsConfig.ignored_ttl_secs`** (`src/app/config.rs`): New `[insights]` config table. Default 1800 seconds (30 minutes). `#[serde(default)]` so pre-v1.48 toml files load cleanly.

11. **`i` overlay (TUI)** (`src/ui/insights.rs`, `src/app/insights_overlay.rs`): New `i` keybind opens the SQLite-backed Token Insights overlay. Renders six-situation recommendation aggregation, cache trend, token growth, cost delta, action ledger, recommendation timeline. `r` refreshes the current `[insights] default_window_secs` window. Read-only surface; no actions, no filters in v1.

12. **Settings + docs** (`src/ui/settings.rs`, `docs/ai/UI_MANUAL.md` §8.9, `docs/ai/ARCHITECTURE.md`, `docs/ai/VALIDATION.md`, `config/qmonster.example.toml`): Settings Parameters/Rules tabs reflect `[insights]` config. UI_MANUAL §8.9 documents the `i` overlay + `qmonster insights --since` CLI. VALIDATION.md adds Phase 8 evidence checklist + v1.48.0 row. ARCHITECTURE.md prepended a v1.48.0 paragraph. example.toml gains a commented `[insights]` block.

## Latest Release Notes

- `v1.48.0` is a minor feature release over the v1.47.0 Phase 7 v3 (c) baseline.
- Local tag: `v1.48.0` points at the v1.48.0 release ledger sync commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.48.0 release ledger sync`.
- Reference plan: `docs/superpowers/plans/2026-05-07-token-insights-overlay.md` (v1) + `docs/superpowers/plans/2026-05-07-token-insights-lifecycle-ledger.md` (v2).
- Reference spec: `docs/superpowers/specs/2026-05-07-token-insights-overlay-design.md`.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.48.0 work has landed yet.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 / v1.46.0 / v1.47.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` / `25478893257` / `25485133895` / `25490555532` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47}.0`; `npm view qmonster versions` lists `1.37.0` through `1.47.0` with `dist-tags.latest = 1.47.0`.
- v1.48.0 CI publication (GitHub Release + npm package) is a pending external follow-up.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
2. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.48.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1209 lib tests + 61 integration tests, all green (lib up from 1162 at v1.46.0 / 1180 at v1.47.0 baseline; integration up from 58 at v1.46 / 60 at v1.47).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.47.0 still apply when the v1.48.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. Phase 7 D3 stays provider-blocked; tag protection / ruleset activation is operator-side. Otherwise pick from upstream backlog (no formal next-phase planning queued at the moment).
