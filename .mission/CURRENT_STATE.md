# CURRENT_STATE

_Last updated: 2026-05-12 (Claude, v2.2.0 critical-eval-driven improvement bundle on top of v2.1.0)_

## Mission

- Title: Qmonster v2.2.0 — critical-eval-driven improvement bundle. Implements the P0/P1 items from the v2.2.0 critical evaluation (Claude main-pane). **P0** dead-code purge (`SignalSet.repeated_output` + alerts/advisories rule + `TaskType::{CodeExploration,LogTriage,Summary}` variants); `log_storm` alert/advisory dedup (alert path removed, advisory retained with quota_tight escalation); `quota_pressure_recommendations` collapsed from three windows to one most-urgent rec; inline threshold provenance tags (`measured`/`intuition`/`mirrors`/`safety`) on every `PolicyGates::default()` magic number. **P1-1** `AnomalyKind::supported_providers()` matrix gates `eval_anomalies` per provider (closes MemoryGrowth-silent-on-Claude honesty gap); Settings Rules tab renders `supports: …` chips for asymmetric detectors. **P1-2** SubagentSideEffect reason leads with `⚠ correlation only —`; evidence metric_name encodes the disclaimer; new `cooccurring_kinds` evidence row. **P1-3** `RecommendationOutcome::Copied` lifecycle variant + ledger-aware `y`-copy helper. **P2** decisions deferred to MDR drafts (`.mission/decisions/MDR-DRAFT-v2.2.0-*`) pending measurement and operator approval; **P2-2** overlay consolidation excluded per explicit operator instruction.
- v2.1.0 baseline (preserved verbatim below for handoff): r2 cross-validated synthesis-slice implementation bundle. Slice 0 Attribution Lock, Slice 1 pane-bucketed Insights, Slice 2 ROI loop closure, Slice 3 anomaly time-normalization, Slice 4 data completeness, Slice 6 live-smoke + `--once` fixtures, plus UI synthesis slices A-I. No new modals, no new detectors, no automatic actuation (3-way Claude/Codex/Gemini consensus).
- Version surfaces: npm package metadata `qmonster@2.2.0`; local Git tag `v2.2.0` to be created at this ledger sync commit; GitHub Release publication is CI-owned.
- Branch / worktree at handoff start: `main`, tag `v2.2.0` to be created at the ledger sync commit. v2.1.0 is the immediate prior tagged baseline.
- Release publication state: **v2.2.0 is published** (verified 2026-05-12). The first tag push at commit `94df2f6` triggered run `25712627303` which failed at 1m23s in `Run release validation`: the new P1-3 `ledger_helper_writes_copied_outcome_on_successful_copy` test called `arboard::Clipboard::new()` directly and the GitHub Actions runner has no display server. Hotfix `44946e8` introduces a generic `copy_selected_alert_command_with_ledger<F>` inner so tests can inject a deterministic closure; `to_clipboard_with_ledger` stays a thin wrapper for production. The v2.2.0 tag was deleted from origin and re-created at `44946e8` (no force-push), mirroring the v1.55.0 fmt+clippy re-tag pattern. The successful re-tag run `25712761819` completed in 7m29s at 2026-05-12T04:16:09Z; GitHub Release `v2.2.0` (published 2026-05-12T04:15:40Z) carries the full asset set (`qmonster-v2.2.0-linux-x86_64.tar.gz` 4.29MB, `qmonster-2.2.0.tgz` 1.34MB, `qmonster-v2.2.0-sbom.spdx.json` 535KB, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 2.2.0`. The companion `CI` workflow run `25712760989` on the hotfix commit completed success in 3m51s. **v2.1.0 is published** (verified 2026-05-11). Run `25649969794` completed success in 7m18s at 2026-05-11T04:23:14Z; GitHub Release `v2.1.0` (published 2026-05-11T04:22:52Z) carries the full asset set (`qmonster-v2.1.0-linux-x86_64.tar.gz`, `qmonster-2.1.0.tgz`, `qmonster-v2.1.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 2.1.0`. The companion `CI` workflow run `25649968220` on the v2.1.0 ledger sync commit `6c49536` also completed success. **v2.0.0 is published** (verified 2026-05-08). Run `25550704578` completed success in 7m15s at 2026-05-08T10:37:44Z; GitHub Release `v2.0.0` (published 2026-05-08T10:37:18Z) carries the full asset set (`qmonster-v2.0.0-linux-x86_64.tar.gz`, `qmonster-2.0.0.tgz`, `qmonster-v2.0.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 2.0.0`. **v1.60.0 is published** (run `25548715050` completed success in 7m44s at 2026-05-08T09:51:00Z). **v1.59.0 is published** (verified 2026-05-08). Run `25546295476` completed success in 6m53s at 2026-05-08T08:53:19Z; GitHub Release `v1.59.0` (published 2026-05-08T08:52:59Z) carries the full asset set (`qmonster-v1.59.0-linux-x86_64.tar.gz`, `qmonster-1.59.0.tgz`, `qmonster-v1.59.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 1.59.0`. **v1.58.0 + v1.58.1 are published**: v1.58.0 run `25544151924` (7m22s, 2026-05-08T08:02:49Z), v1.58.1 run `25544690855` (7m23s, 2026-05-08T08:16:08Z). **v1.55.0 is published** (re-tag after fmt+clippy fixes). `Release and Package Mirror` workflow run `25541168214` (2026-05-08, 6m57s, success) created GitHub Release `v1.55.0` (published 2026-05-08T06:47:00Z) with full asset set (`qmonster-v1.55.0-linux-x86_64.tar.gz`, `qmonster-1.55.0.tgz`, `qmonster-v1.55.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`) and published `qmonster@1.55.0` to npm + GitHub Packages mirror. The original v1.55.0 push (run `25539312814`) failed in 22s on `cargo fmt --check` (anomaly_overlay.rs:718 multi-line saturating_sub chain), and a follow-up clippy `manual_clamp` lint failure on hover_help.rs was fixed in `2b79e88`; the v1.55.0 tag was deleted from origin and re-created at the fixed HEAD `2b79e88` (no force-push). **v1.54.0 is also published** (run `25537887533`, 7m5s, GitHub Release published 2026-05-08T05:11:41Z). v1.53.0 is published (run `25537122599`, 6m48s). v1.52.0 is published (run `25535686447`, 7m2s). v1.51.0 is published (run `25535433723`, 7m2s). v1.50.0 is published (run `25505209461`, 7m26s).
- Current phase: All prior phases complete. v1.50.0–v2.0.0 publication baselines still apply; v2.1.0 ships the r2 cross-validated synthesis-slice implementation on top of the v2.0.0 milestone — no new phase is opened, the work is bundled as the first feature minor on the v2.x line.

## v2.1.0 Feature State

v2.1.0 tags the autonomous main-pane implementation of the r2 plan that
Claude / Codex / Gemini consolidated in
`.docs/claude/Qmonster-v2.0.0-2026-05-08-claude-init-vs-impl-evaluation-r2.md`.
Implementation landed in commit `a3abe0e` (~3996 line changes across 33
files) plus the two cross-validation working-doc commits `ee6de0d` and
`362fe6f`. Highlights:

1. **Slice 0 — Attribution Lock** (`src/domain/identity.rs`,
   `src/adapters/mod.rs`, `src/adapters/process_memory.rs`,
   `src/store/audit.rs`, `src/domain/audit.rs`):
   - Descendant process walk through `node` / `bash` / `python` / `sh`
     wrappers to find the real entry binary (`codex`, `gemini`,
     `claude`, `qmonster`).
   - New `IdentityConfidence::Conflict` state when canonical title and
     descendant binary disagree. Provider adapter enrichment, Claude
     sidefile boost, CLI version facts, Codex account rate-limit
     enrichment, and provider-profile recommendations are **all
     suppressed** on conflict panes. Pane card header shows an
     `IDENTITY CONFLICT` chip with a one-line "provider says X, command
     says Y" explanation.
   - Claude sidefile boost now requires at least **2 independent
     evidences** out of (session id, transcript path, descendant exe,
     sidefile mtime TTL). Ambiguous matches surface `cache ?` /
     `cost ?` / `reset ?` instead of silently inheriting another pane's
     official numbers.
   - New `IdentitySuppressed` audit-event kind emitted once per pane
     conflict episode (metadata-only, no PII).
   - 3 new integration scenarios in `tests/event_loop_integration.rs`
     (node wrapper + Claude sidefile, stale title + qmonster cmd,
     canonical title vs descendant exe mismatch).

2. **Slice 1 — Pane-bucketed Insights** (`src/store/insights.rs`
   ~1014-line refactor, `src/insights_report.rs` +217 lines):
   - All Insights SQL re-grouped by `pane_id` so `first_input`,
     `latest_input`, `cost_delta`, `cache_ratio` stay pane-coherent.
   - New report sections: "Top contributors" (token growth / cost
     delta / cache drift top panes) and "data completeness" (per-pane
     coverage % + missing-poll count).
   - 4-state labelling: `n/a` (no data), `?` (pending), `suppressed`
     (provider gap or conflict), `unsupported` (structurally
     unavailable for the provider).
   - Counter resets and provider changes are split into separate
     analytical segments so a session restart never inflates token
     growth numbers.

3. **Slice 2 — ROI loop closure** (`src/store/insights.rs`,
   `src/insights_report.rs`, alerts panel):
   - Pre/post sample windows (±5 min around outcome) compute the
     before/after delta for `/compact` and profile-switch
     recommendations. Significance threshold + minimum sample count
     gate noisy estimates as `inconclusive` instead of false-positive
     payoffs.
   - **Outcome families separated**: `avoided` (operator declined when
     it was the right call — cache hot) vs `accepted` (operator
     accepted with cache cold). Same accepted-rate can hide very
     different operational meaning; families surface this.
   - New "Action Payoff" section in the Insights overlay + alerts strip
     "last action" chip:
     `last /compact 12m ago · saved ~8K input · cache cold→warm · $0.03 [Est]`.
   - **Rule tuning candidates** list highlights rules with
     `accepted_rate ≥ 50%` but no metric improvement, or
     `ignored_rate ≥ 50%` with suspected false positives.

4. **Slice 3 — Anomaly time-normalization + evidence enrichment**
   (`src/policy/rules/anomaly.rs` +155 lines, `src/store/anomaly_sink.rs`
   +130 lines, `src/ui/anomaly_overlay.rs` +107 lines,
   `src/store/anomaly_history.rs`):
   - `cost_slope` and `token_slope` detectors now compute elapsed time
     from `sample.ts_unix_ms` differences instead of the previous
     `window_polls × 5s` hard-coded assumption. Changing poll interval
     no longer skews detection.
   - `evidence_json` column added to `anomaly_events` (additive
     migration, nullable, read-side fallback to `reason`). Detectors
     stash their full evidence vector (kind, before/after, coverage)
     here.
   - `n` overlay inline `e` key expands the selected row into 1–3
     evidence sub-rows. Coverage-low samples (sparse polls) drop
     detector confidence one step instead of false-firing.
   - `memory_growth`, `error_burst`, and `subagent_side_effect`
     evidence now carry baseline ratios, dominant kind labels,
     co-occurring anomaly kinds, and time-interval data respectively.

5. **Slice 4 — Insights data completeness**: snapshots surface
   `coverage_pct`, `sample_count`, `elapsed_secs` per pane so the
   operator can tell `n/a` (no data yet) from `?` (data is loading).

6. **Slice 6 — Live-smoke validation enrichment**
   (`docs/ai/VALIDATION.md` +155 lines): identity matrix (provider ×
   wrapper × stale title × sidefile presence) with expected confidence
   + suppression outcomes, plus 3 `--once` golden fixtures under
   `tests/integration/once_fixtures/`.

7. **UI synthesis slices A-I** (`src/ui/dashboard.rs` +425 lines,
   `src/ui/metrics.rs` +197 lines, `src/ui/panels.rs` +204 lines,
   `src/ui/insights.rs`, `src/ui/anomaly_overlay.rs`,
   `src/ui/provider_setup.rs` +28 lines):
   - **Slice A — Now strip** atop the alerts panel: 5-priority queue
     (PermissionWait / InputWait first → Risk severity strong rec →
     quota 5h ≥ 0.85 / cost ≥ 80% → recent promoted anomaly → healthy
     fallback). `IdentityConfidence::Conflict` panes are auto-excluded
     from NBA selection so suppressed numbers can never become a
     prescriptive action.
   - **Slice B — Next-Best-Action header** at the top of the Insights
     overlay: prescriptive "do this next" line backed by recent
     payoff data so the recommendation isn't blind.
   - **Slice C — `/compact` payoff chip** on the alerts strip (closes
     the Slice 2 ROI loop visually).
   - **Slice D — Anomaly evidence row expansion** (closes Slice 3
     evidence persistence visually).
   - **Slice E — ETA chips with timestamp normalization**: new
     `src/policy/eta.rs` pure helper. CTX / 5H / 7D / cost thresholds
     project linear slope through the recent N=12 sample window using
     `ts_unix_ms` differences (Conflict pane auto-suppress so ETA
     never displays on a pane whose attribution is broken).
   - **Slice F — Policy mini-strip** in pane cards: identity
     confidence + metric provenance + suppressed/conflict badges in
     one collapsed row.
   - **Slice G — Cost breakdown** by pane × model × situation in the
     Insights overlay (built directly on Slice 1's pane-bucket SQL).
   - **Slice H — Cross-pane correlation hint**: when two panes show
     similar token-growth patterns within a 5-minute window, Insights
     emits "possible duplicated context — consider sharing
     checkpoints" advisory (post-processing, not a new detector).
   - **Slice I — Dashboard audit severity chip**: surfaces audit
     events at their severity tint inline with the alerts header
     instead of needing the audit overlay.

No new modals were added (3-way Claude/Codex/Gemini consensus).
No new detectors were added — the existing 8 detector set is
stabilized via attribution lock + time normalization first. No
automatic actuation was added — Gemini's "cost-reactive auto profile
switch" stays at the recommendation level only (spec §9.1 Layer 5).

Lib tests **1445 → 1478 (+33)**, integration tests **65 → 68 (+3)**.
fmt + clippy clean. Full suite green in 1.12s.

## v2.0.0 Milestone Marker

v2.0.0 is the operator-chosen marker for the point where every
dashboard surface has been polished and the v1.58.0-deferred Light
theme is closed (by v1.60.0). It is **not** a semver breaking-change
signal — there is no API change, no UI key remap, and no removed
feature. Operators upgrading from v1.x to v2.0.0 should expect the
same dashboard, the same keys, the same config schema, and the same
audit/SQLite contract as v1.60.0.

What this release actually contains:

1. **Version-surface alignment** — `package.json` (`1.60.0` →
   `2.0.0`), `README.md` + `VERSION.md` (version table), the
   `PROJECT_BRIEF.md` header (was last touched at v1.56.0),
   `mission.yaml` (`mission.version` + `mission.title`),
   `mission-history.yaml` (meta + new timeline entry sequence 205),
   `.mission/CURRENT_STATE.md` (this file's mission line +
   Validation Baseline), and `docs/ai/VALIDATION.md` (new release
   row) all align on v2.0.0.

2. **Help-modal `/` prose fix** — `src/ui/dashboard.rs`
   `help_lines_for_width` previously documented `/` only as the
   split-cycle key. v1.59.0 added the dual semantic (when Alerts is
   focused, `/` opens the alert filter input; when Panes is focused,
   it cycles the split). The help body now spells this out.

3. **No other code change.** fmt + clippy + test --all-targets stay
   clean; lib tests 1434 → 1445 (+11 from incremental coverage
   already in tree, not new bodies). 65 integration tests preserved.

Future minor bumps (v2.1.x and beyond) signal refinement on top of
this stable surface.

## v1.60.0 Feature State

v1.60.0 closes five more operator-facing polish slices in one bundle:

1. **Insights overlay freshness chip** (`src/ui/insights.rs`): new
   `loaded_at: Option<Instant>` field on `InsightsOverlay` plus
   `is_stale(now)` (returns true once the snapshot has aged past
   `INSIGHTS_STALENESS_SECS = 300`) and `relative_age_label(now)`
   ("just now", "2m ago", "1h ago"). `render_insights_modal` accepts
   `now: Instant` and surfaces the age in the title chip:
   ` Token Insights · 2m ago ` while fresh, ` Token Insights · 8m ago
   · stale (press r) ` past the threshold. Pressing `r` to refresh now
   also pushes a SystemNotice toast ("insights refresh requested ·
   aggregating fresh token insights …") so the operator sees the
   kick landed during the async load gap.

2. **Metrics overlay delta arrows on the left column**
   (`src/ui/metrics.rs`): new `PressureObservation` tracker mirrors
   the v1.38 `MemObservation` pattern (HashMap keyed by `pane_id`,
   populated each poll in `tui_loop` alongside the existing MEM
   tracker). `card_rows` / `left_row` / `quota_left_row` /
   `cache_left_row` thread the new trend through; `left_row` appends
   a dim `▲ ▼ ─` span next to the percentage. Steady epsilon =
   0.5 percentage points (`PRESSURE_TREND_STEADY_EPSILON = 0.005`)
   so routine cache jitter doesn't flicker the arrow. First-frame
   trends are `None` and the renderer falls back to the dim em-dash.

3. **pricing.toml staleness advisory** (`src/policy/pricing.rs` +
   `src/app/startup.rs`): new pure helper
   `pricing_staleness_days(path, now, threshold) -> Option<u64>`
   returns the file's age in days when `mtime + threshold < now`,
   otherwise `None`. `app::startup` calls it once at session start
   with `PRICING_STALENESS_SECS = 60 * 60 * 24 * 90` (90 days) and
   pushes a Concern-severity SystemNotice nudging the operator to
   refresh rates. Missing files are intentionally silent — that's
   the "operator hasn't curated pricing yet" case, not a stale-data
   risk.

4. **Help modal section jumping** (`src/ui/dashboard.rs` +
   `src/app/modal_state.rs` + `src/app/tui_loop.rs`): new
   `help_section_line_indices(viewport)` public helper returns the
   line index of every section header in document order (Controls →
   Hover Help → Source Labels → State Labels). `ScrollModalState`
   gains `set_scroll(scroll, max_scroll)` for absolute jumps. The
   help-modal handler in `tui_loop` intercepts digit keys 1..9
   (where N ≤ section count) and calls `help_modal.set_scroll()`
   to land on the matching section. Hint text advertises
   "1..N jump section".

5. **Light theme variant** (`src/ui/theme.rs` + `src/app/config.rs` +
   bulk migration across `src/`): closes the deferred work from
   v1.58.0. `ThemeMode::Light` added to both the `app::config`
   enum and `ui::theme::ThemeMode`. `cycle()` is now Dark →
   HighContrast → Light → Dark. `set_theme_mode` + `theme_mode`
   handle the new variant (raw=2). Five new theme-aware accessor
   functions — `text_dim()`, `text_primary()`, `badge_bg()`,
   `border_idle()`, `border_active()` — return mode-specific colors.
   `severity_color`, `severity_badge_style`, `label_style` branch
   on Light with a darker fg / lighter bg palette. The legacy
   `TEXT_DIM` / `TEXT_PRIMARY` / `BADGE_BG` / `BORDER_IDLE` /
   `BORDER_ACTIVE` constants stay as the Dark-mode values for
   backward compat with test assertions, but every runtime call
   site (153 of them across `src/`) was sed-migrated to the
   function form so flipping `[ux] theme = light` actually re-tints
   the dashboard. Settings parameter help now lists `dark |
   high_contrast | light`.

No code-behaviour change beyond the five operator-visible surfaces.
1415 → 1434 lib tests (+19 from new insights / metrics / pricing /
dashboard / theme regression guards). 65 integration tests
preserved. fmt + clippy clean.

## v1.59.0 Feature State

v1.59.0 closes five operator-facing polish slices in one bundle:

1. **Anomaly events row severity color coding** (`src/ui/anomaly_overlay.rs`):
   each row in the `n` overlay's history view now renders in
   `theme::severity_color(event.severity)` so Concern / Warning /
   Critical reads at a glance instead of requiring the operator to
   parse the embedded severity column. Title chip + filter chip stay
   in their canonical colors; only the row body picks up the per-event
   tint.

2. **Settings dirty-row visual marker** (`src/ui/settings.rs`):
   the Parameters tab row prefix now shows two adjacent markers — the
   existing yellow `*` for "differs from baked default" plus a new
   green `●` (U+25CF) for "modified-but-not-saved this session" via a
   new `parameter_is_dirty(field)` accessor against the existing
   `dirty_parameters` set. Header documents both. The 6-character
   prefix width before the label is preserved so the existing
   `parameter_row_text_matches_field` `skip(6)` invariant still holds.

3. **Alerts filter** (`src/app/dashboard_runtime.rs` +
   `src/app/tui_loop.rs` + `src/ui/alerts.rs` + `src/ui/dashboard.rs` +
   `src/app/dashboard_render.rs`): symmetric to the v1.58.0 Settings
   filter. New `alert_filter: Option<String>` field on
   `DashboardRuntimeState` with start / cancel / confirm / type_char /
   backspace methods; `/` on Alerts focus opens the filter; while
   active typed chars narrow the alert list (case-insensitive substring
   against title + headline + details + suggested_command via new
   `alert_item_matches_filter` helper); Esc clears, Enter exits input
   mode keeping filter, Backspace edits, `/` restarts. Filter input
   row paints a `· filter:<buf>` chip in the panel title. The legacy
   `/` split-cycle binding stays bound when Panes is focused so Panes
   operators don't lose their layout shortcut.

4. **Three new fx effects + Sampler meta-effect**
   (`src/app/config.rs` + `src/app/fx_state.rs` + `src/ui/fx.rs`):
   - **Snow** — 60 unicode flakes drift top→bottom with cosine-driven
     horizontal sway; large flakes render bright white, smaller
     particles in pale blue to read against any background. Flakes
     past the bottom respawn at the top with fresh seeds.
   - **Fireworks** — rockets launch from the bottom every ~1s, arc up
     until they reach a randomized burst height, then explode into
     32 radial sparks with gravity + life decay. Sparks fade their
     hue toward black as life drains.
   - **Plasma** — full-screen sin/cos field. Each cell samples
     `sin(x/8 + t/30) + sin(y/8 + t/40) + sin((x+y)/16 + t/50)`,
     normalizes the result, picks a glyph from a 9-step density ramp,
     and tints by hue. The lowest-density step is a literal space
     (omitted) so the dashboard stays visible through the gaps.
   - **Sampler** — meta-scene that auto-rotates through Banner →
     Confetti → Matrix → Snow → Fireworks → Plasma every 5 seconds
     (300 frames at 60 FPS). A top-left chip identifies the active
     sub-effect so the operator knows what they're seeing without
     flipping config. The rotation explicitly skips the Sampler
     variant itself so the meta-scene never recurses.
   - All four use the same `XorShift64` seeding pattern as v1.53.0
     Banner / Confetti / Matrix; `FxScene` enum + `from_effect` /
     `step` / `resize` extended exhaustively. `FxEffect::cycle`
     advances Banner → Confetti → Matrix → Snow → Fireworks →
     Plasma → Sampler → Banner. `FxEffect::as_str` mirrors the
     snake-case TOML labels. Settings parameter help line for
     `[fx] effect` updated to list every variant.

5. **README quickstart refresh** (`README.md`):
   - Version surface table bumped to `v1.59.0` / `qmonster@1.59.0`.
   - Quick Start section restructured into four steps — Install
     (npm or cargo), Set the stage (tmux setup), Run it (run
     script), First launch (what the dashboard looks like) — plus
     a key-cluster table covering `?`, `t`, `S`, `P`, `i`, `n`, `Q`
     so a fresh operator can ramp inside a minute. Smoke checks +
     GitHub Packages mirror remain at the bottom.

No code-behaviour change beyond the five operator-visible surfaces.
1403 → 1415 lib tests (+12 from new fx state / scene / sampler
guards + alert filter buffer / matcher tests). 65 integration tests
preserved. fmt + clippy clean.

## v1.58.1 Hotfix

## v1.58.1 Hotfix

Operator report: after pressing `/` on Parameters and typing a filter
(e.g. `anomaly`), the body appeared to stop scrolling — arrow keys
seemed to do nothing. Root cause: `next_parameter` / `prev_parameter`
in `src/ui/settings.rs` cycled through `all_parameter_fields()`
unfiltered, so they could land on a hidden field. The downstream
`keep_selected_parameter_visible` correctly no-ops when the selection
is hidden, but the visible cursor never moves to a row the operator
can see — perceptually frozen.

Fix:
- New private helper `SettingsOverlay::filtered_parameter_field_list`
  returns the field list narrowed by the active filter (case-
  insensitive substring against `parameter_label`). Empty / no filter
  returns the full list.
- `next_parameter` and `prev_parameter` now iterate that list. When
  the current selection is filtered out, `next` jumps to the first
  visible field; `prev` jumps to the last visible field. When the
  filter matches nothing, both no-op.
- 3 new regression tests in `ui::settings::tests` pin the contract
  (cycles within the filtered set; jumps to first/last when current
  is hidden; no-op when filter empties the set).

Lib tests 1397 → 1400 (+3 regression guards). 65 integration tests
preserved. fmt + clippy clean.

## v1.58.0 Feature State

v1.58.0 closes five operator-facing polish slices in one bundle:

1. **hover_help dashboard expansion** (`src/ui/help_glossary.rs` +
   `src/app/hover_help.rs`): three new `HelpTopic` variants —
   `DashboardDivider`, `DashboardFooter`, `DashboardVersionBadge`.
   Each has Korean + English help body explaining the IME indicator
   semantics, the footer key cluster + ★p/★y counters, and the
   click-to-open-Git version badge. `dashboard_hover_topic` dispatches
   them when the mouse is over the divider, footer (excluding the
   badge sub-rect), or version badge respectively. v1.51 IME +
   v1.53 fx hotkey are now discoverable from hover help instead of
   only from canonical docs.

2. **n-overlay row filter** (`src/ui/anomaly_overlay.rs` +
   `src/app/anomaly_overlay.rs`): new `AnomalyFilter { All /
   PromotedOnly / HighOnly }` enum + cycling. `f` key in the n
   overlay cycles the filter (All → Promoted → High → All) and
   resets scroll. Filter state is shown in the title's
   ` filter: <label> ` chip and the hint advertises the `f filter`
   keybinding. Useful when a busy session populates the ring with
   many low-confidence rows that an operator wants to skip.

3. **Provider-honesty cost chip** (`src/ui/provider_honesty.rs` +
   `src/ui/panels.rs`): new `CostMetricStatus { Value / Pending /
   Hidden }` mirroring `CacheMetricStatus`. Cost chip is no longer
   silently omitted when `cost_usd` is `None`:
   - Has a value → `cost $X.XX [Source]` (existing).
   - Provider has token activity but no value → ` COST ? [pricing] `
     surfaces the missing `pricing.toml` row.
   - No token activity yet (fresh pane) or Qmonster pane → omitted.
   Closes the audit's "missing pricing → silent None" perception
   for Gemini panes especially.

4. **Settings Parameters filter / search** (`src/ui/settings.rs` +
   `src/app/settings_overlay.rs`): `/` key on the Parameters tab
   enters filter input mode. Typed characters narrow the visible
   row set to fields whose label contains the substring (case-
   insensitive). Backspace edits the buffer; Enter exits input mode
   while keeping the filter active; Esc clears the filter and
   returns to the full list. Filter input row shows the current
   buffer + match count; an empty result set hints
   "no parameters match — Backspace to broaden, Esc to clear".
   Section headers without matches are suppressed. With 70+
   parameters this turns the linear-scan UX into a few keystrokes.

5. **Theme high_contrast variant** (`src/ui/theme.rs` +
   `src/app/config.rs` + Settings `UxTheme` parameter): new
   `ThemeMode { Dark / HighContrast }` enum with a process-wide
   `AtomicU8` so every render call site reads the same mode without
   re-plumbing config. `severity_color`, `severity_badge_style`,
   `label_style` branch on mode — HighContrast brightens the
   severity palette, adds BOLD to severity badges and dim-text
   labels for legibility on bright-profile terminals. `[ux] theme`
   defaults to `dark`; `cycle_parameter_enum` flips between modes
   live and `tui_loop` calls `set_theme_mode` at startup.
   Constants (TEXT_DIM, TEXT_PRIMARY, BADGE_BG, BORDER_*) stay
   unchanged so existing call sites compile without rewrite — only
   the function-based theme accessors are mode-aware.

No code-behaviour change beyond the five operator-visible surfaces.
1394 → 1397 lib tests (+3 anomaly filter tests). 65 integration
tests preserved. fmt + clippy clean.

## v1.57.0 Feature State

v1.57.0 closes the cross-cutting drift surfaced by a four-axis audit
(version refs, feature claim drift, in-code label consistency,
README/top-level sync) run after v1.56.0 publication. Six concrete
fixes:

1. **README.md / VERSION.md** — `Mission ledger` and `npm` rows bumped
   from `v1.50.0` to `v1.57.0` (was 7 versions stale).
2. **mission.yaml** — `mission.version` and `mission.title` bumped from
   `1.46.0` (frozen at the last new-phase release, Phase 7 v3 a+b) to
   `1.57.0`. Operator chose to break the prior "polish releases don't
   bump mission.version" pattern so all surfaces stay aligned.
3. **PROJECT_BRIEF.md** — header `Version: v0.4.0` (planning-doc rev)
   bumped to `v1.57.0` with a `(planning doc rev v0.4.0 — header bumped
   2026-05-08 to track ledger)` annotation so the historical planning
   number stays attributable.
4. **UI_MANUAL.md** — three additions documenting features that had
   shipped without operator-facing prose:
   - §1 Alerts/Panes divider: documents the v1.51.0 IME indicator
     (`⚠ HANGUL/IME ACTIVE`), TTL behaviour (3s after the last
     non-ASCII alphabetic key), and the heuristic limit (terminals
     don't expose IME state, so the first non-ASCII keystroke is what
     trips the banner).
   - §1 Overlay list: adds `Q` for the decorative fx overlay alongside
     the existing `t / S / P / m / n / a / i / ?` list.
   - §7 Operations: new `Q` keybinding entry covering the v1.53–v1.56
     fx overlay (banner / confetti / matrix selectability, the three
     triggers, dismiss semantics, performance/non-destructive
     contracts).
   - §8 Overlay: new "Decorative fx (v1.53.0+)" subsection with the
     three effects' visual descriptions, the trigger matrix (`Q`
     hotkey / `p`-accept celebration / idle screensaver), the
     `[fx]` config block, and the matrix-backdrop note.
5. **fx dismiss hint** — `src/ui/fx.rs::paint_hint_inline` text changed
   from `Q to toggle` (incorrect — `Q` only opens; any key dismisses)
   to `Q opens fx`. Closes a HIGH-severity operator-confusion finding.
6. **Dashboard footer key cluster** — `src/ui/dashboard.rs::footer_keys_text`
   now advertises `Q fx` between `i insights` and `? help`, so the
   v1.53.0 Q hotkey is discoverable from the persistent footer instead
   of only via the help modal.
7. **REVIEW_GUIDE.md** — "Empty UI surfaces are not bugs" paragraph
   now cites the matrix as the v1.50.0 baseline plus the v1.56.0
   `CacheMetricStatus` honesty exception, instead of a stale
   "v1.49.0 provider-coverage matrix" reference.

No code-behaviour change beyond the two UI-string fixes (fx hint text
and footer cluster). All 1394 lib tests + 65 integration tests
preserved; fmt + clippy clean.

## v1.56.0 Feature State

v1.56.0 is a polish bundle that closes the remaining recommended slices
from `Qmonster-v0.4.0-2026-05-08-claude-feature-audit-and-slice-1-r1.md`.
Four small surfaces, no new phase:

1. **Provider-honesty cache chips** (new `src/ui/provider_honesty.rs` +
   `src/ui/panels.rs` + `src/ui/metrics.rs`):
   - New `CacheMetricStatus` enum (`Value{ratio,source}` / `Pending`
     / `Unsupported` / `Hidden`) with `cache_metric_status(signals,
     provider)` helper that encodes the per-provider rules:
     - Claude / Codex with a value → `Value` (existing path)
     - Claude / Codex without a value yet → `Pending` (waiting on an
       optional surface, e.g. fresh pane / sidefile that hasn't landed)
     - Gemini with token stats but no `cached_input_tokens` →
       `Unsupported` (proven structurally unavailable)
     - Gemini without stats yet → `Pending` (still in early-pane state)
     - Qmonster / Unknown → `Hidden` (chip omitted)
   - `panels::cache_chip_text` (text variant for `metric_row`,
     `--once` reports) and `panels::cache_badge_text` (Line-styled
     variant for the pane card primary metric row) both delegate to
     this enum, so the UI no longer silently omits the cache chip when
     the value is `None`. Operators see `cache ?` while waiting and
     `cache —` when the surface cannot supply it — removing the
     "metric is silently missing" perception flagged in the audit.
   - `metrics::cache_left_row` in the `m` overlay uses the same enum
     so the per-pane card cache bar renders `?` / `—` consistently.
   - Signature changes: `metric_row(s, provider)`, `metric_badge_lines(
     s, provider, wrap_width)`, `primary_metric_row(s, provider)`
     thread provider through. `src/app/once_report.rs` passes
     `report.identity.identity.provider`. Test sites updated.
   - 14 new tests in `provider_honesty::tests`,
     `panels::tests::cache_chip_text_renders_per_provider_placeholder`,
     `panels::tests::metric_badge_line_emits_cache_placeholder_per_provider`,
     `metrics::tests::metrics_cache_bar_derives_ratio_from_raw_cached_tokens`.

2. **Matrix fx backdrop dim** (`src/ui/fx.rs::render_fx_matrix`):
   - Before painting matrix streams, walk every cell in `area` and
     OR `Modifier::DIM` onto the existing style. The terminal's standard
     "dim" SGR attribute renders the underlying dashboard at ~50%
     brightness; matrix glyphs paint BOLD bright green/white over this
     dim layer, restoring full contrast on exactly the cells they
     touch.
   - Keeps the v1.54.0 non-destructive guarantee: `cell.set_style`
     preserves the symbol; only the style flag changes. The
     `fx_overlay_preserves_underlying_cells_outside_effect_glyphs`
     regression test (Confetti scene) still passes.

3. **Insights Situations rendering regression guard**
   (`src/ui/insights.rs::tests::rendered_overlay_surfaces_populated_situations_section`):
   - Builds an `InsightsSnapshot` with two populated `SituationSummary`
     rows, calls `set_snapshot`, then asserts both
     (a) `overlay.lines()` contains the `Situations` header + each
         `<situation>: <emitted>` row formatted by
         `format_insights_report_lines`, and
     (b) the end-to-end rendered TestBackend buffer also surfaces the
         situation labels — pinning the contract against future
         layout / Paragraph refactors that could silently drop them.
   - Closes the audit's "always 'none'" perception risk.

4. **ARCHITECTURE.md cache-rule provider-gating paragraph**
   (`docs/ai/ARCHITECTURE.md` "How surfaces should treat unavailable
   signals"):
   - Documents the per-signal exception added in v1.56.0: most missing
     structural signals still omit silently (per the v1.50.0
     provider-coverage matrix), but the CACHE chip is the documented
     exception — provider-honest with `?` / `—` placeholders. Names
     `src/ui/provider_honesty.rs::cache_metric_status` as the canonical
     pattern for any future per-signal honesty addition.

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

Most recent v2.1.0 validation at the release commit:

- `cargo fmt --all --check` — clean.
- `cargo test --all-targets` — **1478 lib tests + 68 integration tests + supporting suites**, all green in 1.12s. +33 lib / +3 integration since v2.0.0 cover Slice 0 attribution-lock scenarios, Slice 1 pane-bucket aggregation, Slice 2 payoff window families, Slice 3 timestamp normalization + evidence_json migration round-trip, and Slice A-I UI synthesis surface tests.
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` — clean.
- `git diff --check` — clean.

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v2.0.0 still apply when the v2.1.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. Phase 7 D3 stays provider-blocked; tag protection / ruleset activation is operator-side. Otherwise pick the next slice from the post-v1.49.0 audit's recommended list (`.docs/claude/Qmonster-v0.4.0-2026-05-08-claude-feature-audit-and-slice-1-r1.md`): `error_burst` evidence enrichment (signal shape change), an `m`-overlay smoke render test that exercises a Gemini pane with pricing entered to lock the cost_slope activation contract.
