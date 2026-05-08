# CURRENT_STATE

_Last updated: 2026-05-08 (Claude, v1.55.0 ledger sync)_

## Mission

- Title: Qmonster v1.55.0 — fx overlay smoothness: 60 FPS render with halved per-frame velocities, tmux poll tick suspended while overlay is active so the 50-200ms capture/parse pass no longer stutters animation; new defaults `[fx] text = "~O~ Qmonster"` and `[fx] duration_secs = 50`.
- Version surfaces: npm package metadata `qmonster@1.55.0`; local Git tag pending sync commit; GitHub Release publication is CI-owned.
- Branch / worktree at handoff start: `main`, tag `v1.55.0` to be created at the ledger sync commit. v1.54.0 is the immediate prior tagged baseline.
- Release publication state: **v1.54.0 is published**. `Release and Package Mirror` workflow run `25537887533` (2026-05-08, 7m5s, success) created GitHub Release `v1.54.0` (published 2026-05-08T05:11:41Z) with full asset set (`qmonster-v1.54.0-linux-x86_64.tar.gz`, `qmonster-1.54.0.tgz`, `qmonster-v1.54.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`) and published `qmonster@1.54.0` to npm + GitHub Packages mirror. **v1.53.0 is also published** (run `25537122599`, 6m48s, GitHub Release published 2026-05-08T04:46:41Z). v1.52.0 is published (run `25535686447`, 7m2s, GitHub Release published 2026-05-08T03:58:45Z). v1.51.0 is published (run `25535433723`, 7m2s). v1.50.0 is published (run `25505209461`, 7m26s). Sibling v1.37.0–v1.49.0 publications all remain live — `npm view qmonster versions` lists `1.37.0` through `1.54.0` with `dist-tags.latest = 1.54.0`; GitHub Release pages at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54}.0`.
- Current phase: All prior phases complete. v1.50.0 / v1.51.0 / v1.52.0 / v1.53.0 / v1.54.0 publication baselines still apply; v1.55.0 ships the fx-overlay smoothness polish + new defaults on top — no new phase is opened.

## v1.55.0 Feature State

v1.55.0 polishes the v1.53.0 / v1.54.0 fx overlay so animation feels
smooth on a busy operator dashboard. Three changes:

1. **60 FPS render with halved per-frame velocities** (`src/app/fx_state.rs`):
   - `FX_TARGET_FPS` 30 → 60 (`FX_FRAME_INTERVAL_MS` 33 → 16). The main
     loop's `event::poll(Duration)` while the overlay is open follows
     this constant.
   - All velocities halved so apparent speed stays the same as v1.54.0,
     but frames render twice as often: banner `vx` 0.55 → 0.275, `vy`
     0.30 → 0.15; confetti spawn speed `0.4 + rand*1.0` → `0.2 +
     rand*0.5`; confetti `CONFETTI_GRAVITY` 0.06 → 0.03 with
     `CONFETTI_INITIAL_LIFE` 60 → 120 frames so on-screen lifetime is
     unchanged; matrix stream speed `0.25 + rand*0.6` →
     `0.12 + rand*0.30`. Banner hue rotation `FX_HUE_STEP` 2 → 1
     keeps the same ~6-second full rotation.
   - Effect: the visible juddering from rendering the same integer-
     rounded position for 2 frames in a row at 30 FPS is halved at
     60 FPS — sub-cell increments still alias to the same cell, but
     the eye sees a refresh twice as often, smoothing perceived motion.

2. **Suspend tmux poll tick while overlay is active** (`src/app/tui_loop.rs`):
   - The dashboard's main 2-second tmux capture / parse / policy
     evaluation pass takes 50-200ms when it fires. While the fx
     overlay is open the loop now skips `handle_poll_tick` entirely
     (`if … >= poll && !fx_overlay.is_open()`), so animation no
     longer hitches every 2 seconds.
   - Maximum staleness while overlay is open: one skipped tick
     (~2 seconds of frozen pane data). Acceptable for a deliberately
     decorative moment; the tick resumes the moment the overlay
     dismisses, which is the next event::poll iteration.

3. **New `[fx]` defaults** (`src/app/config.rs`):
   - `text` `"QMONSTER"` → `"~O~ Qmonster"`. Block font now renders
     `~` as a wave glyph (`▄▀▄ / ▀▄`) so the new default renders
     correctly out of the box.
   - `duration_secs` `5` → `50`. Five seconds was too short for a
     celebration / screensaver moment to read; 50 seconds gives the
     operator room to enjoy the effect (or dismiss it sooner with
     any key / click).
   - Existing operators' `qmonster.toml` overrides are honoured —
     the new defaults only apply to first-run / unconfigured installs.

## v1.54.0 Feature State

v1.54.0 is a one-line polish layered on the v1.53.0 fx overlay: the
overlay no longer wipes the viewport before drawing. The dashboard
(alerts, panes, footer, version badge) and any modals already painted
this frame stay visible **underneath** the bouncing banner / confetti
particles / matrix streams. Operators no longer lose situational
awareness when the overlay is active.

1. **Non-destructive `render_fx_overlay`** (`src/ui/fx.rs`):
   - Dropped `frame.render_widget(Clear, area)` from the dispatcher
     entry. Each effect already painted its glyphs via direct
     `buf[(x, y)]` cell writes, so removing the upfront `Clear`
     leaves every cell the dashboard / modals already populated
     intact.
   - Replaced the bottom-right dismiss `Paragraph` with a new
     `paint_hint_inline` helper that walks the hint string and only
     writes non-space characters via direct `buf[(x, y)]` writes —
     spaces stay transparent so the underlying footer text is still
     readable.
   - Removed the now-unused `Clear` and `Paragraph` imports from
     `ratatui::widgets`.

2. **Regression guard test** (`fx_overlay_preserves_underlying_cells_outside_effect_glyphs`):
   - Paints sentinel `*` cells across the full 80×24 viewport,
     then renders the fx overlay on top in the SAME `terminal.draw`
     pass (mirroring the real `render_dashboard_frame` sequencing
     where the overlay is the last widget). Asserts at least 1500 of
     the 1920 sentinel cells survive — confetti only lands ~80
     particles plus a short hint, so the underlying dashboard must
     remain visible.
   - The pre-v1.54.0 Clear-based render dropped the survivor count
     to ~0; this test pins the new behaviour against future
     regressions.

3. **Heuristic boundary**: matrix rain's per-column streams still
   land on top of dashboard text, which can look visually noisy.
   Operators who find the overlap distracting can either pick a
   shorter `[fx] duration_secs`, switch effect to banner / confetti,
   or disable the screensaver trigger that auto-opens it.

## v1.53.0 Feature State

v1.53.0 adds the optional decorative effects (`[fx]`) overlay on top of the v1.52.0 baseline. Three selectable effects, three triggers, eight Settings parameters. Inert until opt-in: the overlay never opens unless `[fx] enabled = true` AND one of the three triggers fires.

1. **Three selectable effects** (`src/app/fx_state.rs` + `src/ui/fx.rs`):
   - **Banner** — operator-supplied `[fx] text` rendered in a hand-rolled 5-row block font (A-Z, 0-9, space, `-!?.*`); position bounces DVD-screensaver style off viewport edges; per-cell HSL→RGB hue rotation (`FX_HUE_STEP = 2°` per frame at 30 FPS, full rotation in ~6s).
   - **Confetti** — 80 unicode sparkles (`✨ ⋆ ☆ ⭐ ✦ ✧ ❋ ❀ ✺`) spawned in a radial pattern at viewport centre with light gravity (`CONFETTI_GRAVITY = 0.06`); `CONFETTI_INITIAL_LIFE = 60` frames before fade; particles drift toward background as they age.
   - **Matrix** — per-column falling streams of `ｱ-ﾄ` katakana + ASCII glyphs; head is bright white-green (`Rgb(220, 255, 220)` + BOLD), tail dims toward green-only as `i / length` increases; one trail glyph mutates per frame for the flicker effect; streams refill once they scroll past the bottom.

2. **Three triggers** (`src/app/fx_overlay.rs` + `src/app/tui_loop.rs`):
   - **`Q` hotkey** — uppercase Q (distinct from lowercase `q` which closes most modals) opens the configured effect via `FxTrigger::Hotkey`. Gated by `[fx] hotkey_enabled = true`.
   - **`p` accept celebration** — when an operator accepts a pending action via `p` (either through the action-explainer's Enter path or the direct dispatch path), `FxTrigger::Celebration` fires and **always shoots confetti regardless of the configured effect** — confetti is the natural confirmation glyph. Gated by `[fx] celebration_enabled = false` (default off; opt-in).
   - **Idle screensaver** — when `now - last_user_activity >= screensaver_idle_secs`, `FxTrigger::Screensaver` opens the configured effect. `last_user_activity` is updated on every keypress and mouse event so any operator input keeps the saver suppressed. Gated by `[fx] screensaver_enabled = false` (default off; opt-in).

3. **Click / any-key dismiss + auto-dismiss**:
   - The overlay short-circuits per-overlay key dispatch: any keypress while it's open just calls `fx_overlay.dismiss()` and `continue`s past the rest of the dispatch chain.
   - Mouse `Down(_)` events do the same.
   - `[fx] duration_secs` (default 5; 0 = stay until dismissed) auto-dismisses by elapsed time.
   - `FxScene::is_finished()` for confetti returns true once every particle has expired, so a celebration burst self-dismisses naturally even with `duration_secs = 0`.

4. **Performance gating**:
   - While the overlay is closed, `event::poll(100ms)` keeps the existing 10 FPS quiet path — zero added cost.
   - While the overlay is active, the loop tightens to `event::poll(FX_FRAME_INTERVAL_MS = 33ms)` for smooth ~30 FPS animation.
   - `FxScene::step` is pure (no IO); seeded by a small xorshift64 so frame jitter doesn't collapse to identical patterns each open.

5. **Settings → Parameters integration** (`src/ui/settings.rs`, FX section):
   - 8 new `ParameterField` variants — `FxEnabled`, `FxText`, `FxEffect`, `FxDurationSecs`, `FxHotkeyEnabled`, `FxCelebrationEnabled`, `FxScreensaverEnabled`, `FxScreensaverIdleSecs`.
   - Edit kinds: 4 Bool (toggle), 1 Enum (`banner` → `confetti` → `matrix` cycle via `FxEffect::cycle`), 1 Text (`fx text` — banner string), 2 Number (`duration_secs` ≥ 0, `screensaver_idle_secs` ≥ 60).
   - Standard pipeline: `parameter_label`, `parameter_toml_path`, `parameter_value_for_display`, `parameter_value_for_edit`, `toggle_parameter_bool`, `cycle_parameter_enum`, `apply_parameter_edit`, `merge_parameter_field` all extended.
   - All 8 fields participate in the v1.51.0 editable/reference grouping (Parameters tab is `(editable)` group).

6. **Heuristic boundary**: the celebration trigger fires once per accepted `p` dispatch; failed dispatches never fire (the celebration call lives inside the success branches of both the action-explainer Enter path and the direct dispatch path). The screensaver trigger never fires while ANY overlay is open — `should_auto_open_screensaver` short-circuits on `fx_overlay.is_open()`.

## v1.52.0 Feature State

v1.52.0 adds one operator-visible feature on top of the v1.51.0 baseline: a per-pane CLI version badge in the panel title. It does not change the audit chain core, the SQLite schema, or any policy rule semantics.

1. **Pane CLI version badge** (new `src/app/cli_version.rs`, `src/adapters/process_memory.rs` extension, `src/ui/panels.rs`, `src/app/event_loop.rs`, `src/domain/signal.rs`, `src/policy/rules/advisories.rs`):
   - New module `src/app/cli_version.rs` (270 LoC) holds:
     - `CliVersionCacheKey { provider, pid, comm, argv, exe_path }` — deduplicates probe shell-outs per stable descendant process identity.
     - `CliVersionCache = HashMap<CliVersionCacheKey, Option<RuntimeFact>>` — `Some(None)` memoises a "probed and didn't return a version" outcome so we don't reshell every poll.
     - `resolve_cli_version_fact(provider, tail, pane_pid, cache) -> Option<RuntimeFact>` — orchestrator. Returns `None` for `Provider::Qmonster` (we don't probe our own monitor pane). Tries `parse_cli_version_from_tail` first (cheapest, most reliable — provider already rendered the version on screen). Falls back to `read_descendant_cli_process(pane_pid)` + cached `probe_cli_version(desc)` shell-out.
     - `parse_cli_version_from_tail(provider, tail)` — provider-aware regex extraction from the captured pane tail text.
     - `probe_cli_version(desc)` — runs the resolved executable / script with `--version` (or provider-specific equivalent); parses the structured output into `RuntimeFact { kind: RuntimeFactKind::CliVersion, value, source_kind, ... }`.
   - `src/adapters/process_memory.rs` gains `CliProcessDescriptor { pid, comm, exe_path, argv, … }` and `read_descendant_cli_process(pane_pid: u32)` (with a `_with_proc_root` test seam) — walks `/proc/<pid>/cmdline` + `/proc/<pid>/exe` resolution to find the deepest known CLI subprocess that owns the pane's tty. Returns `None` when the pane is dead or owned by something we don't recognize.
   - `src/app/event_loop.rs` calls `resolve_cli_version_fact(provider, tail, pane_pid, &mut ctx.cli_version_cache)` per pane per poll and threads the resulting `RuntimeFact` into the report so the UI can render it.
   - `src/ui/panels.rs` adds the badge rendering: `session:window · Provider role · CLI <version> [Official] · %pane_id`. The `[Official]` chip comes from `RuntimeFact.source_kind` (`SourceKind::ProviderOfficial` for tail-parsed; whatever the probe parser declares for the shell-out path). When the resolved version cannot be confirmed to belong to the current pane (descendant process gone, parser failed, etc.), the badge is silently omitted — the pre-v1.52.0 title format is preserved.
   - `Context.cli_version_cache: CliVersionCache` (`src/app/bootstrap.rs`) is the only new persistent runtime state.
   - `src/domain/signal.rs` adds the `RuntimeFactKind::CliVersion` variant alongside the existing runtime-fact taxonomy.
   - 2 new integration tests in `tests/event_loop_integration.rs` exercise the per-pane badge end-to-end (parse-from-tail and probe-fallback paths). Lib tests gain ~7 new module tests under `cli_version::tests` and `process_memory::tests` for the descendant-process resolver and key-equality contract.
   - **Qmonster monitor pane is intentionally exempt** from the badge so the layout doesn't carry recursive self-reference.
   - **Heuristic boundary**: the badge only renders when we can confirm the resolved binary belongs to the current pane's process tree. Stale cache entries are scoped by descendant-pid + argv + exe_path, so a CLI restart inside the pane re-probes naturally on the next poll.

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

## v1.51.0 Feature State

v1.51.0 layers four themes on top of the v1.50.0 baseline. It does not
change provider adapters, the audit chain core, the `SignalSet` schema,
or any SQLite schema.

1. **`build.rs` tag-cache invalidation** (`build.rs`):
   - Cargo's `rerun-if-changed` directives now also watch
     `.git/refs/tags` (the directory whose mtime advances when any tag
     is created or deleted) and `.git/packed-refs`. Without these the
     compiled `QMONSTER_GIT_VERSION` env var would lag the actual
     `git describe --tags --always --dirty` output by however many
     tag bumps happened between rebuilds — the bug that left the
     footer badge showing `v1.48.0` long after `v1.49.0` / `v1.50.0`
     were tagged.

2. **Git modal: origin URL + Contributors** (`src/app/git_info.rs`):
   - `GitSnapshot` gains `origin_url: Option<String>`,
     `top_contributors: Vec<ContribLine>`,
     `extra_contributors: usize`,
     `extra_contributor_commits: usize`.
   - `capture_snapshot` calls `git config --get remote.origin.url` and
     `git shortlog -sne HEAD` (falling back to `-sn` if the host's
     git lacks `-e`), funnelling the raw output through new pure
     helpers `normalize_origin_url` and `parse_shortlog`.
     SSH-style remotes (`git@github.com:user/repo.git`) and
     `ssh://git@host:port/path.git` are rewritten to HTTPS; trailing
     `.git` is stripped so operators can paste the URL into a browser.
   - `panel_from_snapshot` adds an `origin` detail line and a
     `Contributors` block (`<commits>  <name>` per row, capped at the
     top 5; remainder summarised as `+N more (M commits)`).
   - 9 new lib tests under `git_info::tests` lock the helper contracts
     and the panel rendering. Lib 1294 → 1306 (+12 incl. snapshot
     coverage).

3. **Drag-bar IME indicator + terminal bell** (new `src/app/ime_state.rs`,
   `src/ui/dashboard.rs`, `src/app/tui_loop.rs`):
   - New module `src/app/ime_state.rs` holds a pure heuristic state
     machine: `ImeState::observe(c, now)` → `ImeObservation` enum
     (`Ignored` for non-alphabetic, `AsciiCleared` for ASCII letters,
     `NonAsciiSet { transitioned_on }` for non-ASCII letters). The
     `transitioned_on: true` edge fires the bell exactly once per
     activation. `is_active(now)` honours a 3-second TTL
     (`IME_INDICATOR_TTL`) so the indicator dims naturally during
     idle without needing an ASCII keystroke to clear it.
   - `Context` (`src/app/bootstrap.rs`) gains `ime_state: ImeState`.
   - `tui_loop` observes every `KeyCode::Char(c)` press BEFORE the
     overlay dispatcher chain so modals don't mask the signal. On
     inactive→active transitions a single ASCII BEL (`0x07`) is
     written directly to stdout via the new `ring_terminal_bell`
     helper.
   - `render_split_divider` (`src/ui/dashboard.rs`) replaces the
     normal `drag resize alerts/panes …` text with a high-visibility
     banner `⚠ HANGUL/IME ACTIVE — press 영문/English key to disable ⚠`
     when `ime_active` is true. Color comes from
     `theme::severity_color(Severity::Warning)` + `BOLD | REVERSED`
     for prominent contrast on the badge background.
   - 10 new `ime_state::tests` cover the state-machine matrix
     (transition signalling, TTL decay, ASCII clear, ignored chars,
     CJK char coverage). 2 new dashboard tests pin the divider
     branching: normal hint when inactive, banner when active.
   - **Heuristic limit accepted by operator**: terminals don't
     expose IME state to TUI apps, so the very first non-ASCII
     keystroke is what flips the indicator on. The user accepted
     this limitation at design time.

4. **Settings overlay editable/reference tab grouping** (`src/ui/settings.rs`):
   - New `SettingsTab::is_editable()` returns `true` for Thresholds /
     Integrations / Parameters and `false` for Rules / Badges.
   - Custom tab strip rendering replaces ratatui's `Tabs` widget so
     the seam between editable and reference groups can show a
     visible `║` divider while same-group tabs keep the existing `│`.
     Labels gain a numeric prefix (`1 Thresholds`, `2 Integrations`,
     …) so the keyboard shortcut is visible in the strip.
     `settings_tab_index_at` recomputed for the new label widths;
     the mouse handler test in `src/app/settings_overlay.rs` updated
     to match.
   - Body title gains an `(editable)` / `(reference)` suffix so the
     operator can tell at a glance whether arrow / Enter / `e` will
     mutate config or just navigate.
   - 1 new lib test (`settings_tab_strip_renders_inter_group_seam_…`)
     pins the seam glyph + position contract.
   - `anomaly enabled` parameter remains in the Parameters tab as
     `ParameterField::AnomalyEnabled` — already toggleable via
     space/Enter; the new tab grouping makes it discoverable as part
     of the editable cluster rather than buried in a flat list.

### Concurrent change folded into v1.51.0

5. **Anomaly observation surface default-on** (`src/app/config.rs`):
   - `AnomalyConfig::default().enabled` flips from `false` → `true`.
     Phase 7 v2/v3 detectors stabilised + ErrorBurst dominant_kind
     evidence + CrossPane fold-back closure (all v1.50.0) made every
     detector observable to operators on first launch.
   - Existing operators with `enabled = false` written into their
     `qmonster.toml` keep that override; the deserialization contract
     is untouched — only the unconfigured / first-run experience
     changes.
   - 4 config-tests updated in `src/app/config.rs`:
     `anomaly_config_enabled_by_default`,
     `anomaly_config_missing_section_keeps_default_enabled`, plus a
     new `anomaly_config_explicit_disable_is_honoured` test that
     pins the override-respect contract. 2 integration tests in
     `tests/event_loop_integration.rs` (`event_loop_anomalies_off_baseline_regression`,
     `event_loop_no_promotion_when_anomaly_disabled`) explicitly
     opt back out via `config.anomaly.enabled = false` since they
     exercise the disabled-mode regression.
   - 2 settings-overlay tests in `src/ui/settings.rs` force the
     starting state explicitly so the toggle mechanic is exercised
     regardless of default polarity.

## Post-tag polish (folded into v1.51.0)

These commits were on `main` after the `v1.50.0` tag; they ride v1.51.0:

1. **Phase 7 v2 ErrorBurst evidence enrichment** (`1862c46`).
   `adapters::common::classify_error_hint(tail) -> Option<&'static str>`
   returns one of `traceback` / `jvm_exception` / `rust_panic` /
   `rust_compiler_error` / `error_prefix` / `fatal_prefix`.
   `SignalSet.error_hint_kind` populated in lockstep with the existing
   `error_hint` bool surface (contract test guards the invariant).
   `AnomalyHistory.error_hint_kinds: VecDeque<Option<&'static str>>`
   parallel to `error_hints` (session-only — SQLite restart-replay
   intentionally does not carry the kind so the storage schema stays
   unchanged; detector gracefully omits the dominant_kind row when
   the deque is empty). `detect_error_burst` evidence vec now carries
   a second `AnomalyEvidence` row with `metric_name = "dominant_kind"`,
   `before = <label>`, `after = <count>/<total>` so operators reading
   the `n` overlay see what kind of error spiked, not just a rate
   delta. Closes the audit's load-bearing 'error_burst is high-FP and
   uninterpretable' finding without touching threshold / window-size
   config — Codex's parameter-edit affordance round owns that
   surface. Tests: 1285 → 1289 lib (+4).

2. **`n` overlay reason column joins every evidence row** (`4e4a517`).
   Pairs with #1 above. The in-memory `dominant_kind` row was being
   silently dropped by `event_loop`'s reason builder, which read only
   `sig.evidence.first()`. Extracted a small
   `pub(crate) fn anomaly_event_reason(sig)` helper that joins every
   row with ` | `; the inline builder is now a one-line call. Three
   tests lock the contract: ErrorBurst's two-row output produces
   `error_rate: 0.30 → 0.65 | dominant_kind: rust_panic → 8/13`;
   single-row detectors stay byte-identical so SQLite
   `anomaly_events.reason` readers observe no format drift; empty
   evidence preserves the prior `unwrap_or_default` contract. Closes
   the audit's ErrorBurst-evidence-enrichment loop end-to-end: the
   kind label is collected (#1), aggregated into a dominant row (#1),
   and now visible in the `n` overlay + persisted to
   `anomaly_events.reason`. Lib tests 1289 → 1292 (+3).

3. **Gemini adapter cost activation** (`3353707`). The audit's
   "cost is Claude/Codex-only by code, not just by config" finding.
   The Gemini adapter has parsed model + input + output tokens since
   v1.41.x but never called `pricing.lookup()` — even when the
   operator filled in real Gemini rates in `pricing.toml`, the COST
   badge stayed blank and the `cost_slope` anomaly detector never
   fired on Gemini panes. Added the parallel cost branch from
   `src/adapters/codex.rs:121-133`: when the pane carries
   `model_name` + both token counts AND `cost_usd` is `None`, look
   the model up in `ctx.pricing` and compute
   `(input × rate + output × rate) / 1M` with `SourceKind::Estimated`.
   Two contract tests cover the activation path (status table model +
   /stats model tokens + NamedTempFile pricing.toml → cost = $3.75)
   and the v1.49.0 baseline (empty PricingTable → cost_usd stays
   None). Pairs with #4 above (`config/pricing.example.toml` Gemini
   rows): template seeds the edit path, adapter honours the rates.
   Provider-coverage matrix flips Gemini `cost_slope` from "deferred
   until pricing is curated" to "operator-tunable". Lib tests
   1292 → 1294 (+2).

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 / v1.46.0 / v1.47.0 / v1.48.0 / v1.49.0 / v1.50.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` / `25478893257` / `25485133895` / `25490555532` / `25491535211` / `25498621086` / `25505209461` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47,48,49,50}.0`; `npm view qmonster versions` lists `1.37.0` through `1.50.0` with `dist-tags.latest = 1.50.0`.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
2. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent v1.55.0 validation at the release commit:

- `cargo fmt --all --check`
- `cargo test --all-targets` — 1373 lib tests + 65 integration tests + supporting suites, all green.
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `git diff --check`

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v1.54.0 still apply when the v1.55.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. Phase 7 D3 stays provider-blocked; tag protection / ruleset activation is operator-side. Otherwise pick the next slice from the post-v1.49.0 audit's recommended list (`.docs/claude/Qmonster-v0.4.0-2026-05-08-claude-feature-audit-and-slice-1-r1.md`): `error_burst` evidence enrichment (signal shape change), an `m`-overlay smoke render test that exercises a Gemini pane with pricing entered to lock the cost_slope activation contract.
