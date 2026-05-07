# CURRENT_STATE

_Last updated: 2026-05-08 (Claude, v1.50.0 release ledger sync)_

## Mission

- Title: Qmonster v1.50.0 — operator UX polish: Insights overlay async fetch + spinner placeholder + Phase 7 v2 CrossPane fold-back + provider-coverage matrix + pricing rows.
- Version surfaces: mission ledger target `1.50.0`; npm package metadata `qmonster@1.50.0`; latest local Git tag `v1.49.0` (v1.50.0 tag is the next step).
- Branch / worktree at handoff start: `main`.
- Release publication state: v1.49.0 is published. `Release and Package Mirror` workflow run `25498621086` (2026-05-07, 7m1s, success) created GitHub Release `v1.49.0` with full asset set (binary tarball, npm tarball, SBOM, sbom-diff, checksums) and published `qmonster@1.49.0` to npm + GitHub Packages mirror. Sibling v1.37.0 (`25159598038`), v1.38.0 (`25305201597`), v1.39.0 (`25311723861`), v1.40.0 (`25421376056`), v1.41.0 (`25424418078`), v1.42.0 (`25472444159`), v1.43.0 (`25474748447`), v1.44.0 (`25476534645`), v1.45.0 (`25478893257`), v1.46.0 (`25485133895`), v1.47.0 (`25490555532`), v1.48.0 (`25491535211`) publications also remain live — `npm view qmonster versions` lists `1.37.0` through `1.49.0` with `dist-tags.latest = 1.49.0`; v1.50.0 publication will follow this commit.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, Phase 6 Team Mode, the v1.38 UX bundle (F1/F2/F3/F4), the v1.39 polish + correctness round, the v1.40 operator-controlled overlay geometry round, the v1.41 a-overlay polish round, Phase H opt-in auto-snapshot, Phase 7 v1 anomaly observation surface, Phase 7 v2 promotion, Phase 7 v2 detectors, Phase 7 v3 (a+b), Phase 7 v3 (c), Phase 8 v1 token insights query layer, and Phase 8 v2 recommendation lifecycle ledger are complete. v1.50.0 ships as an operator UX polish bundle on top of the v1.49.0 baseline; no new phase is opened.

## v1.50.0 Feature State

v1.50.0 layers five themes on top of the v1.49.0 baseline. It does not change provider adapters, the audit chain core, the `SignalSet` schema, or any SQLite schema. Phase 8 (Token Insights, shipped in v1.48.0) remains the underlying runtime baseline.

1. **Insights overlay async fetch + centered spinner placeholder** (`src/app/insights_load.rs` (new), `src/ui/insights.rs`, `src/app/bootstrap.rs`, `src/app/tui_loop.rs`):
   - New module `src/app/insights_load.rs`: `InsightsLoadOutcome { request_id: u64, result: Result<InsightsSnapshot, String> }` + `spawn_insights_load(audit_path, window, ignored_ttl_secs, sender, request_id)` using `std::thread::spawn` + `std::panic::catch_unwind(AssertUnwindSafe(...))`. A worker-thread panic surfaces as `Err("insights load thread panicked")` instead of leaving the overlay stuck in Loading. Missing-DB → `Ok(empty_insights_snapshot)` (graceful first-time operator); SQLite failure → `Err(format!("open/query {} failed: {e}", path.display()))`.
   - `Context` gains `insights_load_tx: Sender<InsightsLoadOutcome>`, `insights_load_rx: Receiver<InsightsLoadOutcome>`, `next_insights_request_id: u64` (initialized in `Context::new` via `mpsc::channel()`).
   - `tui_loop`'s `refresh_insights_overlay` body replaced with worker-thread dispatch: increments `ctx.next_insights_request_id`, calls `overlay.mark_loading(id)`, spawns the worker. Signature changes from `&Context<P, N>` to `&mut Context<P, N>`.
   - Main loop drains `ctx.insights_load_rx` per iteration via `try_recv` non-blocking (placed before `event::poll(100ms)` so it runs even on timeout-only ticks). Each outcome calls `InsightsOverlay::set_snapshot_for(id, result, label)`; outcomes whose `request_id != pending_request_id` (or whose overlay closed) are silently dropped.
   - After the drain, `if insights_overlay.is_loading() { insights_overlay.advance_spinner() }`. Phase advances mod 10.
   - `InsightsOverlay` state: `loading: bool`, `spinner_phase: u8`, `pending_request_id: u64`. Methods: `mark_loading`, `is_loading`, `pending_request_id`, `spinner_glyph`, `advance_spinner`, `set_snapshot_for`. `close()` clears `loading`. `line_count()` returns 0 while loading so scroll keys are no-ops.
   - `SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']`.
   - `render_insights_modal` loading branch: centered `Aggregating insights ⠋` line via Layout vertical `[Min(0), Length(1), Min(0)]` + `Alignment::Center`; `theme::BORDER_ACTIVE` + `Modifier::BOLD`; 6 panels suppressed; block title gains a ` — loading` suffix.
   - Subagent-driven 12-task TDD plan; 13 commits `cff0314 → 8e4ebe8`. Reference spec/plan: `docs/superpowers/specs/2026-05-07-insights-overlay-loading-spinner-design.md` + `docs/superpowers/plans/2026-05-07-insights-overlay-loading-spinner.md`.

2. **Phase 7 v2 `CrossPaneEditCluster` fold-back loop closure** (commits `458a931` + `3b93330`):
   - `CrossPaneFinding` gains `paths: Vec<String>` field; `eval_concurrent_files` populates it for every `ConcurrentFileEdit` finding.
   - New `fold_finding_paths_into_history` helper in the event loop appends those paths to the front entry of every attributed pane's `AnomalyHistory.cross_pane_edit_paths` deque.
   - The Option A 1-tick-lag pattern at `event_loop.rs:495-500` is preserved; the detector running on tick T sees tick T-1's fold-back.
   - 3 new unit tests in `event_loop.rs::tests` lock the contract end-to-end including the round-trip-fires-detector case.
   - Closes the dead-pipe gap surfaced during the Claude-led v1.49.0 audit (Phase 7 v2 detector existed but `cross_pane_edit_paths` was always `Vec::new()`).

3. **Provider-coverage matrix in `docs/ai/ARCHITECTURE.md`** (commit `a5f4c7e`):
   - v1.49.0-baseline matrix of every signal the policy + anomaly stack reads against Claude / Codex / Gemini availability.
   - Detector/rule reach implications: e.g. cache rule = Claude+Codex, `auto_memory` / `agent_memory` = Claude-only, `CrossPaneEditCluster` = Claude-only until other adapters emit `active_files`.
   - Canonical "render as em-dash, omit silently, exclude from aggregations" rules so surfaces stay honest about provider gaps instead of silently emitting `None`.
   - `docs/ai/REVIEW_GUIDE.md` cites the matrix as the canonical reference reviewers should consult when a metric chip is empty (commit `caa5cda`).

4. **`config/pricing.example.toml` Gemini + Claude Opus placeholder rows** (commit `013d9f0`):
   - `claude-opus-4-7`, `gemini-3.1-pro`, `gemini-3.1-flash` placeholder entries.
   - Rates 0.00 → `lookup()` returns `None`, no v1.49.0 behaviour change.
   - Operators no longer have to discover the `(provider, model)` keying convention themselves to enable the COST badge / `cost_slope` anomaly detector on those panes — it becomes a single-line edit.

5. **Codex parallel parameter-edit affordance for Settings overlay** (mixed across the `chore(ai): apply Claude edit` commits `458a931` / `cd78b5e`):
   - `src/ui/settings.rs` gains `ParameterField` / `ParameterEditKind` editors, parameter keystroke routing, and the activate/commit pipeline.
   - Lib test count moves from 1265 (v1.49.0) → 1285 across the polish round.

## Latest Release Notes

- `v1.50.0` is an operator UX polish release over the v1.49.0 baseline.
- Local tag: `v1.50.0` will point at the v1.50.0 release ledger sync commit (recorded in `mission-history.yaml` `related_commits`).
- Commit summary: `v1.50.0 release ledger sync`.
- Reference plan / spec: `docs/superpowers/plans/2026-05-07-insights-overlay-loading-spinner.md` + `docs/superpowers/specs/2026-05-07-insights-overlay-loading-spinner-design.md` for the named line item; the CrossPane fold-back / provider matrix / pricing extension / Settings parameter-edit affordance landed via direct main-pane / parallel-track commits.

## Post-tag polish (on main, untagged)

No genuinely-post-v1.50.0 work has landed yet.

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 / v1.46.0 / v1.47.0 / v1.48.0 / v1.49.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` / `25478893257` / `25485133895` / `25490555532` / `25491535211` / `25498621086` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47,48,49}.0`; `npm view qmonster versions` lists `1.37.0` through `1.49.0` with `dist-tags.latest = 1.49.0`. v1.50.0 publication will follow this ledger sync.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
2. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.50.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1285 lib tests + 63 integration tests, all green (lib up from 1265 at v1.49.0 baseline as new Insights spinner / CrossPane fold-back / Settings parameter-edit tests landed).
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.49.0 still apply when the v1.50.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. Phase 7 D3 stays provider-blocked; tag protection / ruleset activation is operator-side. Otherwise pick the next slice from the post-v1.49.0 audit's recommended list (`.docs/claude/Qmonster-v0.4.0-2026-05-08-claude-feature-audit-and-slice-1-r1.md`): `error_burst` evidence enrichment (signal shape change), an `m`-overlay smoke render test that exercises a Gemini pane with pricing entered to lock the cost_slope activation contract.
