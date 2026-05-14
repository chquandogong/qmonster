# CURRENT_STATE

_Last updated: 2026-05-14 (Claude, v2.3.4 publication verified — GitHub Release + npm 2.3.4 live; micro-patch bundles workflow npm CLI pin to 11.14.1 + metrics overlay card polish on top of v2.3.3; publish auth path unchanged from v2.3.3 — NPM_TOKEN env fallback still in place pending future OIDC-only experiment; v2.3.x supply-chain hardening Phase A+B closed, Phase C C2+C4 docs landed, C1 deferred by maintainer, C3 SBOM-diff guard observed clean)_

## Mission

- Title: Qmonster v2.3.0 — post-v2.2.0 stabilization + UX bundle on top of v2.2.0. **(a) Path-row worktree-role hint** (PR #11 merged): pane card path row appends ` · wt of <parent>` for linked git worktrees via the new TTL-cached `WorktreeRoleCache`. **(b) Sectioned pane panel layout with tree branches**: expanded pane card body groups under NOW / WHERE / PRESSURE / RUNTIME / RECOMMENDATIONS section headers and renders each row with `├ / └ / │` tree glyphs at depth-2-cell indent; recommendation sub-details (`next`, `run`, `lever`, `effect`, `profile`) indent another level. **(c) Alert related-context rail**: alerts overlay surfaces related action lifecycle / family chips per item on a dedicated rail with a projection gate against stale context. **(d) Post-v2.2.0 critical-review 9-item bundle + footer status chip mouse-click actions** (`2dcb5b5` + `afe37ee`). **(e) Copied-outcome lifecycle stabilization + done_when_refs 100 % coverage** (`dc31b28`): `ActionLedgerRow.copied` non-terminal outcome, SubagentSideEffect label normalization, 23/23 done_when binders + `cargo_fmt_check_passes` eval. **(f) Settings module split**: `src/ui/settings.rs` → `src/ui/settings/{mod,model,tests}.rs`. **(g) Dependabot dep-bump bundle**: thiserror 1→2, crossterm 0.28→0.29, rusqlite 0.32→0.39, toml 0.8→1.1, toml_edit 0.22→0.25, attest-build-provenance v3→v4; rusqlite 0.39 `u64: FromSql` workaround at `src/store/insights.rs:845`.
- v2.2.0 baseline (preserved verbatim below for handoff): critical-eval-driven improvement bundle. **P0** dead-code purge (`SignalSet.repeated_output` + alerts/advisories rule + `TaskType::{CodeExploration,LogTriage,Summary}` variants); `log_storm` alert/advisory dedup (alert path removed, advisory retained with quota_tight escalation); `quota_pressure_recommendations` collapsed from three windows to one most-urgent rec; inline threshold provenance tags (`measured`/`intuition`/`mirrors`/`safety`) on every `PolicyGates::default()` magic number. **P1-1** `AnomalyKind::supported_providers()` matrix gates `eval_anomalies` per provider; Settings Rules tab renders `supports: …` chips for asymmetric detectors. **P1-2** SubagentSideEffect reason leads with `⚠ correlation only —`; evidence metric_name encodes the disclaimer; new `cooccurring_kinds` evidence row. **P1-3** `RecommendationOutcome::Copied` lifecycle variant + ledger-aware `y`-copy helper. **P2** decisions deferred to MDR drafts (`.mission/decisions/MDR-DRAFT-v2.2.0-*`) pending measurement and operator approval; **P2-2** overlay consolidation excluded per explicit operator instruction.
- v2.1.0 baseline (preserved verbatim below for handoff): r2 cross-validated synthesis-slice implementation bundle. Slice 0 Attribution Lock, Slice 1 pane-bucketed Insights, Slice 2 ROI loop closure, Slice 3 anomaly time-normalization, Slice 4 data completeness, Slice 6 live-smoke + `--once` fixtures, plus UI synthesis slices A-I. No new modals, no new detectors, no automatic actuation (3-way Claude/Codex/Gemini consensus).
- Version surfaces: npm package metadata `qmonster@2.3.0`; local Git tag `v2.3.0` to be created at this ledger sync commit; GitHub Release publication is CI-owned.
- Branch / worktree at handoff start: `main`, tag `v2.3.0` to be created at the ledger sync commit. v2.2.0 is the immediate prior tagged baseline.
- Release publication state: **v2.3.4 is published** (verified 2026-05-14). Tag push at commit `d956249` triggered `Release and Package Mirror` workflow run `25845955107`, which completed success; GitHub Release `v2.3.4` (published 2026-05-14T06:46:37Z) carries the full asset set (`qmonster-v2.3.4-linux-x86_64.tar.gz` 4.39 MB, `qmonster-2.3.4.tgz` 1.38 MB, `qmonster-v2.3.4-sbom.spdx.json` 580 KB, `sbom-diff-summary.txt` 805 B, `checksums.txt` 372 B); `npm view qmonster@2.3.4 dist.attestations.provenance.predicateType` is `https://slsa.dev/provenance/v1` and `dist.signatures` carries the same `SHA256:DhQ8wR5APBvFHLF/+Tc+AYvPOdTpcIDqOhxsBHRwC7U` Sigstore key as prior releases; `npm view qmonster dist-tags` shows `latest = 2.3.4`. v2.3.4 bundles `bce982f` (Upgrade npm step pinned to `npm@11.14.1`, was floating `@11`) and `b9e2caf` (metrics overlay card rendering polish in `src/ui/metrics.rs` + `docs/ai/UI_MANUAL.md`); lib tests grew 1563 → 1564 with the new metrics regression. **v2.3.3 is published** (verified 2026-05-14). Tag push at commit `3fcef51` triggered `Release and Package Mirror` workflow run `25844474365`, which completed success: release-asset job built the full artifact set, the new `Upgrade npm for Trusted Publishers publish-auth` step installed `npm@11.14.1`, and `npm publish --access public --provenance` returned 0 with a Sigstore-logged provenance statement (log index `1531660936`); `npm view qmonster dist-tags` shows `latest = 2.3.3`; SBOM diff vs v2.3.2 shows 272 → 272 packages, only delta is the `qmonster` crate version. v2.3.3 is the hotfix release for the v2.3.2 npm publish failure described next. **v2.3.2 is partially published** (cut 2026-05-14, commit `b38d96e`, workflow run `25843940371`): release-asset job + GitHub Release `v2.3.2` succeeded with the full asset set (binary tarball + SBOM + diff + checksums + attestations); the publish step's `npm publish` attempted the first OIDC-only auth path on this package and the provenance signing succeeded (Sigstore log `1531483713`), but the registry PUT returned `404 'qmonster@2.3.2' is not in this registry` because `setup-node@v6.4.0` + node 20 ships npm 10.x and Trusted Publishers publish-auth (distinct from provenance signing) needs npm 11.5+; `npm owner ls qmonster` confirms `chquandogong` is the sole package owner so the TP registrant identity is correct. npm registry stays at 2.3.1 → jumps to 2.3.3; the 2.3.2 npm slot remains absent (GitHub Release `v2.3.2` stays published for archaeological reference). The v2.3.3 hotfix adds the `npm install -g npm@11` upgrade step before `Publish canonical npm package` and re-introduces `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}` plus the NPM_TOKEN-empty early-exit as a transitional fallback so any future TP misconfiguration cannot block the release again. Whether v2.3.3's publish actually authenticated via OIDC TP (npm 11.5+ feature) or via the restored NPM_TOKEN fallback is not distinguishable from the workflow log — both auth paths emit the same `npm notice` output. The next release cycle that removes the env block while keeping the npm@11 upgrade will be the actual OIDC-only verification. **v2.3.1 is published** (verified 2026-05-14). Tag push at commit `70c87e3` triggered `Release and Package Mirror` workflow run `25843134839`, which completed success in 7m22s at 2026-05-14T05:22:57Z; GitHub Release `v2.3.1` (published 2026-05-14T05:22:26Z) carries the full asset set (`qmonster-v2.3.1-linux-x86_64.tar.gz` 4.39 MB, `qmonster-2.3.1.tgz` 1.37 MB, `qmonster-v2.3.1-sbom.spdx.json` 580 KB, `sbom-diff-summary.txt` 805 B, `checksums.txt` 372 B); `npm view qmonster@2.3.1 dist.attestations.provenance.predicateType` reports `https://slsa.dev/provenance/v1` and `dist.signatures` carries an npm Sigstore signature; SBOM diff vs v2.3.0 shows 272 → 272 packages, the only added/removed entry being the `qmonster` crate itself. v2.3.1 is the supply-chain hardening patch on top of v2.3.0 (active tag protection ruleset id 16375762, 8 GitHub Actions `uses:` SHA-pinned, ci.yml npm package surface guard, Dependabot security updates enabled, release.yml per-job permissions, Trusted Publishers migration target prepared on the publish job, RELEASING / RELEASE_VERIFICATION / README user-side verification docs; cli_version tail parser requires `v` prefix; alerts UX polish auto-commits 942a822 + 64c89fd; lru `IterMut` Stacked Borrows advisory accepted via `.mission/decisions/MDR-DRAFT-v2.3.x-supplychain-lru-stacked-borrows-acceptance.md` pending the ratatui 0.30 bump). The active tag protection ruleset blocked the v2.3.1 tag push at the GitHub API layer ("Cannot create ref due to creations being restricted") and admitted it only via the admin bypass actor — first end-to-end verification that the ruleset is enforcing the design. **v2.3.0 is published** (verified 2026-05-13). Tag push at commit `c8f20fe` triggered `Release and Package Mirror` workflow run `25790698215`, which completed success in 7m30s at 2026-05-13T09:39:14Z; GitHub Release `v2.3.0` (published 2026-05-13T09:38:49Z) carries the full asset set (`qmonster-v2.3.0-linux-x86_64.tar.gz` 4.39MB, `qmonster-2.3.0.tgz` 1.37MB, `qmonster-v2.3.0-sbom.spdx.json` 580KB, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 2.3.0`. The companion `CI` workflow run `25790687284` on the same commit completed success. The prior CI run `25789113538` on the alert-related-context-rail merge had failed on a `cargo fmt --check` drift in `src/ui/panels/mod.rs` (group_into_section_rows + cmd row + parent_lines rewrap); the v2.3.0 ledger sync commit absorbs that rustfmt fix. **v2.2.0 is published** (verified 2026-05-12). The first tag push at commit `94df2f6` triggered run `25712627303` which failed at 1m23s in `Run release validation`: the new P1-3 `ledger_helper_writes_copied_outcome_on_successful_copy` test called `arboard::Clipboard::new()` directly and the GitHub Actions runner has no display server. Hotfix `44946e8` introduces a generic `copy_selected_alert_command_with_ledger<F>` inner so tests can inject a deterministic closure; `to_clipboard_with_ledger` stays a thin wrapper for production. The v2.2.0 tag was deleted from origin and re-created at `44946e8` (no force-push), mirroring the v1.55.0 fmt+clippy re-tag pattern. The successful re-tag run `25712761819` completed in 7m29s at 2026-05-12T04:16:09Z; GitHub Release `v2.2.0` (published 2026-05-12T04:15:40Z) carries the full asset set (`qmonster-v2.2.0-linux-x86_64.tar.gz` 4.29MB, `qmonster-2.2.0.tgz` 1.34MB, `qmonster-v2.2.0-sbom.spdx.json` 535KB, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 2.2.0`. The companion `CI` workflow run `25712760989` on the hotfix commit completed success in 3m51s. **v2.1.0 is published** (verified 2026-05-11). Run `25649969794` completed success in 7m18s at 2026-05-11T04:23:14Z; GitHub Release `v2.1.0` (published 2026-05-11T04:22:52Z) carries the full asset set (`qmonster-v2.1.0-linux-x86_64.tar.gz`, `qmonster-2.1.0.tgz`, `qmonster-v2.1.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 2.1.0`. The companion `CI` workflow run `25649968220` on the v2.1.0 ledger sync commit `6c49536` also completed success. **v2.0.0 is published** (verified 2026-05-08). Run `25550704578` completed success in 7m15s at 2026-05-08T10:37:44Z; GitHub Release `v2.0.0` (published 2026-05-08T10:37:18Z) carries the full asset set (`qmonster-v2.0.0-linux-x86_64.tar.gz`, `qmonster-2.0.0.tgz`, `qmonster-v2.0.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 2.0.0`. **v1.60.0 is published** (run `25548715050` completed success in 7m44s at 2026-05-08T09:51:00Z). **v1.59.0 is published** (verified 2026-05-08). Run `25546295476` completed success in 6m53s at 2026-05-08T08:53:19Z; GitHub Release `v1.59.0` (published 2026-05-08T08:52:59Z) carries the full asset set (`qmonster-v1.59.0-linux-x86_64.tar.gz`, `qmonster-1.59.0.tgz`, `qmonster-v1.59.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`); `npm view qmonster dist-tags` shows `latest = 1.59.0`. **v1.58.0 + v1.58.1 are published**: v1.58.0 run `25544151924` (7m22s, 2026-05-08T08:02:49Z), v1.58.1 run `25544690855` (7m23s, 2026-05-08T08:16:08Z). **v1.55.0 is published** (re-tag after fmt+clippy fixes). `Release and Package Mirror` workflow run `25541168214` (2026-05-08, 6m57s, success) created GitHub Release `v1.55.0` (published 2026-05-08T06:47:00Z) with full asset set (`qmonster-v1.55.0-linux-x86_64.tar.gz`, `qmonster-1.55.0.tgz`, `qmonster-v1.55.0-sbom.spdx.json`, `sbom-diff-summary.txt`, `checksums.txt`) and published `qmonster@1.55.0` to npm + GitHub Packages mirror. The original v1.55.0 push (run `25539312814`) failed in 22s on `cargo fmt --check` (anomaly_overlay.rs:718 multi-line saturating_sub chain), and a follow-up clippy `manual_clamp` lint failure on hover_help.rs was fixed in `2b79e88`; the v1.55.0 tag was deleted from origin and re-created at the fixed HEAD `2b79e88` (no force-push). **v1.54.0 is also published** (run `25537887533`, 7m5s, GitHub Release published 2026-05-08T05:11:41Z). v1.53.0 is published (run `25537122599`, 6m48s). v1.52.0 is published (run `25535686447`, 7m2s). v1.51.0 is published (run `25535433723`, 7m2s). v1.50.0 is published (run `25505209461`, 7m26s).
- Current phase: All prior phases complete. v1.50.0–v2.0.0 publication baselines still apply; v2.1.0 ships the r2 cross-validated synthesis-slice implementation on top of the v2.0.0 milestone — no new phase is opened, the work is bundled as the first feature minor on the v2.x line.

## Post-v2.2.0 Stabilization (unreleased)

On top of the published v2.2.0 bundle, two follow-up bundles land in tree
before the next tag is cut:

1. **`2dcb5b5` — post-v2.2.0 critical-review improvement bundle (9 items)**
   plus **`afe37ee` — footer status chip mouse-click actions**. These close
   secondary findings from the v2.2.0 critical evaluation that did not
   block release.

2. **`dc31b28` — copied-outcome lifecycle stabilization + done_when_refs
   binding (10 files, +254 / -31)**. Three coordinated axes:
   - *Copied as a non-terminal lifecycle outcome.* `ActionLedgerRow.copied`,
     `apply_lifecycle_outcome("copied")`, `is_terminal_lifecycle_outcome`,
     and `rec_engagement_snapshot`'s CTE all treat clipboard copies as
     engagement telemetry that does **not** suppress TTL-ignored counting
     on the originating event. The CTE attaches unlinked copy outcomes
     (written with `recommendation_event_id = None`) to the latest prior
     event on the same `(pane_id, action)` inside the reporting window.
   - *SubagentSideEffect label normalization.* The emitted action string
     carries a leading `⚠` (U+26A0) marker; the canonical literal lives in
     `domain::anomaly::SUBAGENT_SIDE_EFFECT_ACTION` and is referenced by
     `policy::rules::anomaly`, `store::insights::KNOWN_ACTION_PREFIXES`,
     and the integration tests. `situation_for_action` matches both the
     legacy and current labels via `starts_with("anomaly: subagent
     activity")` so older audit DBs keep grouping under "Code exploration".
   - *Mission `done_when_refs` 100 % coverage.* All 23 `done_when` items
     bind to a concrete validator (eval-ref / file-exists / file-contains /
     command), and `cargo_fmt_check_passes` joins `cargo_build_passes`,
     `cargo_test_passes`, and `cargo_clippy_clean` in the eval set. The
     clippy gate flag (`-A clippy::uninlined_format_args`) is mirrored in
     `scripts/verify-shared.sh`.

   Validation at this snapshot: `npx mission-spec eval` reports **23/23
   done_when passed**; `cargo test --all-targets` is **1648 green** (lib
   1511 + integration 68 + 69 supporting); `cargo fmt --all --check` and
   `cargo clippy --all-targets -- -D warnings -A
   clippy::uninlined_format_args` clean.

These stabilization bundles do not change the v2.2.0 release surface and
are not user-visible behaviour changes — they harden the lifecycle
attribution model and tighten mission verifiability before the next minor
bump.

## 2026-05-13 Path-row worktree-role hint (branch — unmerged)

Branch `worktree-feat-path-row-worktree-role` (worktree at
`.claude/worktrees/feat-path-row-worktree-role`, branched from `ad5f351`).
Seven commits, 16 files, +507/-3. Adds a ` · wt of <parent-repo-root>`
suffix to the pane-card `path` row when the pane's cwd resolves to a
linked git worktree (`git worktree add`-style sibling checkout). Primary
worktrees and non-git cwds render unchanged.

- **`ab54ae1` — pure resolver.** `pub(crate) enum WorktreeRole { Primary,
  Linked { parent_repo_root: PathBuf } }` plus
  `resolve_worktree_role(current_path: &str) -> Option<WorktreeRole>`
  in `src/app/worktree_info.rs`. Runs `git rev-parse --git-common-dir`
  / `--git-dir` once and canonicalizes both before equality; returns
  `None` on empty/missing/non-git cwd or any git failure. Lives next to
  `suggest_worktree_split_command` so all worktree-shaped git knowledge
  stays in one module. `WorktreeRole` is deliberately kept off
  `SignalSet` — this is a derived local-fs fact, not a provider signal,
  so the `SourceKind::ProviderOfficial` contract on
  `signals.worktree_path` (the provider statusline value) stays
  untouched.

- **`7f64af6` — test isolation fix.** Initial linked-worktree test
  hardcoded `/tmp/linked-wt` (a shared path under `tempfile`'s `/tmp`
  parent), which survived across runs and raced under parallel `cargo
  test`. Rewrapped both the primary and the linked checkout inside a
  single parent tempdir.

- **`3257190` + `7097fad` — TTL cache.** `WorktreeRoleCache` memoizes
  both `Some` and `None` results with a 10s TTL (`WORKTREE_ROLE_TTL`)
  and 128-key insertion-order eviction (`WORKTREE_ROLE_CACHE_CAPACITY`).
  At nominal 2s tmux poll cadence this means ~5 ticks per distinct cwd
  per resolution. `lookup_at` takes an explicit `Instant` so tests can
  drive TTL boundaries without sleeping; `lookup` wraps it with
  `Instant::now()` for production. `3257190` was a transient bridge
  with `#[cfg_attr(not(test), expect(dead_code, reason = "..."))]` to
  keep the clippy gate green between the resolver commit and the cache
  commit; `7097fad` removes that bridge as the cache consumes the
  resolver from production code.

- **`f9f0cc7` — plumbing through `PaneReport`.** Adds `pub(crate)
  worktree_role: Option<WorktreeRole>` to `PaneReport` (next to
  `current_path`) and `pub(crate) worktree_role_cache:
  WorktreeRoleCache` to `Context<P, N>` (initialized via
  `WorktreeRoleCache::default()` in `Context::new`). Both
  `PaneReport` construction sites in `event_loop.rs` (dead pane and
  live pane) call `ctx.worktree_role_cache.lookup(&pane.current_path)`.
  Test fixtures at 12 sites across `src/app/` and `src/ui/` get
  `worktree_role: None,` literals. Visibility is `pub(crate)` on both
  fields to match the `pub(crate)` types they hold — same precedent as
  `Context::cli_version_cache: pub(crate)` (a `pub` field would have
  tripped `clippy::private_interfaces` under `-D warnings`).
  A new `event_loop::tests::pane_report_worktree_role_is_some_linked_for_added_worktree_cwd`
  test confirms the cache type is reachable from
  `event_loop`'s namespace and resolves a real linked worktree
  fixture. The field carried a `#[cfg_attr(not(test), expect(dead_code,
  reason = "...Task 4..."))]` bridge until the renderer commit landed.

- **`8b37ebb` — rustfmt cleanup.** Pre-existing fmt issues from the
  resolver/cache commits that the gate caught at the Task 3 endpoint.
  Mechanical rustfmt rewrap of two long-arg calls — no semantic
  change.

- **`95f3ecf` — renderer.** New `path_row_value(report)` helper in
  `src/ui/panels/mod.rs` (next to `display_path` / `display_command`)
  returns the base `display_path(&report.current_path)` for `Primary`
  and `None` roles, or appends ` · wt of
  <display_path(parent_repo_root)>` for `Linked`. Three call sites
  switch to the helper: `pane_list_lines_with_flash` (`:413-417`),
  `panel_body_with_width` (`:709-712`), and the `HelpTopic::PanePath`
  topic-count at `:230-234`. The parent repo root passes through the
  same `display_path` (which does `ellipsize(_, 80)` but no `$HOME`
  collapse — matching the primary cwd's behaviour). Three tests in
  `src/ui/panels/tests.rs` cover Linked / Primary / None; established
  pattern is to call `pane_list_lines_with_flash` and assert via
  `line_text` extraction. Bridge attribute on `PaneReport.worktree_role`
  removed at this commit (renderer is the production consumer).

  `metric_badge_lines` / `context_metric_row` / `runtime_text_groups`
  are deliberately untouched — they render the orthogonal
  `signals.worktree_path` metric (provider statusline value), which is
  a separate `SourceKind::ProviderOfficial` contract.

Validation at branch HEAD (`95f3ecf`): `cargo fmt --all --check` clean;
`cargo clippy --all-targets -- -D warnings -A
clippy::uninlined_format_args` clean; `cargo test --all-targets` green
(lib **1460** = +14 from worktree base 1446, +3 over Task-3 baseline
1457; integration 69; supporting suites unchanged). `npx mission-spec
eval` reports **21/23** — `v2.2.0 P0-3` (gates.rs inline provenance) and
`v2.2.0 P1-2` (anomaly.rs SubagentSideEffect prose) both fail because
the worktree base `ad5f351` predates the 16 main commits that landed
the alert-flow-timeline-rail work plus `5d466d6` (which restored those
two v2.2.0 surfaces). On main HEAD `5ccf040` the same eval reports
**22/23**; only the P1-2 prose check fails there. **These two failures
are pre-existing relative to the worktree's base, not caused by the
path-row feature.** A rebase onto main before merge will close the
P0-3 gap (and may surface conflicts on `src/ui/panels/mod.rs`
where the alert-flow work also lives).

No release surface change; the path-row hint is local UX polish that
will ride into the next tagged minor. No mission contract change.

## 2026-05-13 Dependabot dep-bump cleanup (unreleased)

A focused pass on the seven open PRs that had accumulated against
the post-v2.2.0 working set. All seven are now closed or merged
without a release surface change:

- **PR #10** (`feat/worktree-split-advisory`) closed without merge.
  The worktree-split hint commits (bb90620 + ac62264 + 9ecb6c5 +
  6814d63 + 59c0a43) already lived on local main with byte-identical
  diffs for `src/policy/rules/concurrent.rs`. The PR branch's CI was
  red on a `clippy::too_many_arguments` lint against
  `footer_status_chip_at`; main had independently refactored that
  signature into the `FooterChipQuery` struct (afe37ee), so the lint
  fix never propagated back to the PR branch.

- **PR #9** merged (`83a7af6`). `actions/attest-build-provenance`
  v3 → v4 — a thin wrapper over `actions/attest`; existing
  release.yml wiring continues to work.

- **PR #6** merged (`ecb90ed`). `thiserror` 1.0.69 → 2.0.18. The
  crate is only consumed via `#[derive(Error)]`, so the major bump
  required no source change.

- **PRs #4, #5, #7, #8** closed in favour of bundle commit
  `33252f8`. The four bumps — `crossterm` 0.28.1 → 0.29.0,
  `rusqlite` 0.32.1 → 0.39.0, `toml` 0.8.23 → 1.1.2+spec-1.1.0,
  `toml_edit` 0.22.27 → 0.25.11+spec-1.1.0 — are independent
  Cargo.toml/Cargo.lock changes whose individual dependabot-CI
  runs had all gone green against the post-thiserror-2 base, but
  each sequential merge would have forced the other three into a
  full rebase + recreate + CI cycle. Bundling collapses
  ≈ 25 min of sequential churn into a single ~5 min CI
  verification.

- **rusqlite 0.39 `u64: FromSql` workaround** (`d4694fc`)
  preemptively applied to `src/store/insights.rs:845` *before* the
  bundle landed. rusqlite 0.38 disabled the default `u64`/`usize`
  `ToSql`/`FromSql` impls, so reading `COUNT(*)` as `u64` directly
  fails the trait bound. Switching the column read to
  `row.get::<_, i64>(2)?.max(0) as u64` compiles on both 0.32 and
  0.39 with identical behaviour on the values SQLite actually
  produces (`COUNT(*) ≥ 0`).

The cleanup also pushed eight unpushed `chore(ai)` commits and a
single `chore(fmt)` rustfmt fix on `src/policy/rules/anomaly.rs`
that had been left behind by the post-stabilization auto-commit
flow. Main is now flush with `origin/main` at `33252f8`.

Validation at this snapshot:

- `cargo fmt --all --check` — clean.
- `cargo clippy --all-targets -- -D warnings -A
  clippy::uninlined_format_args` — clean.
- `cargo test --all-targets` — **1439 lib + 68 integration + 70
  supporting** tests, all green. (The pre-existing flaky
  `app::cli_version::tests::node_cli_probe_uses_exact_interpreter_and_script`
  test — likely an ETXTBSY race on the fake `node` shim under
  parallel cargo test load — was observed to fail intermittently
  on PR #9's initial CI run; a single `gh run rerun --failed`
  cleared it and main's CI on every commit since `83a7af6` has
  been green.)
- Main CI on bundle commit `33252f8` (workflow run
  `25774997063`) completed success in 5m10s at
  2026-05-13T02:48:27Z.

No release surface change; the dep-bump set is post-v2.2.0
unreleased state, to ride into the next tagged minor.

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

## Earlier version sections — archived

Detailed feature-state prose for **v1.50.0 through v1.60.0** (and the
v1.51.0 post-tag polish) was archived to `.mission/CURRENT_STATE.history.md`
during the post-v2.2.0 documentation diet (2026-05-12). The live file
stays focused on the current operating window (v2.0.0 milestone +
v2.1.0 synthesis-slice baseline + v2.2.0 mission summary above).

For per-tag deltas and prose narratives older than v2.0.0, consult:
- `.mission/CURRENT_STATE.history.md` — archived narrative sections
- `mission-history.yaml` `change_sequence` — authoritative per-tag deltas

## Known External State

- v1.37.0 / v1.38.0 / v1.39.0 / v1.40.0 / v1.41.0 / v1.42.0 / v1.43.0 / v1.44.0 / v1.45.0 / v1.46.0 / v1.47.0 / v1.48.0 / v1.49.0 / v1.50.0 are all published (workflow runs `25159598038` / `25305201597` / `25311723861` / `25421376056` / `25424418078` / `25472444159` / `25474748447` / `25476534645` / `25478893257` / `25485133895` / `25490555532` / `25491535211` / `25498621086` / `25505209461` all completed success). GitHub Release pages live at `https://github.com/chquandogong/qmonster/releases/tag/v1.{37,38,39,40,41,42,43,44,45,46,47,48,49,50}.0`; `npm view qmonster versions` lists `1.37.0` through `1.50.0` with `dist-tags.latest = 1.50.0`.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Phase 7 D3 per-subagent token attribution remains blocked until providers expose structured per-subagent counters.
2. Tag protection / ruleset activation remains a GitHub Settings task.

## Validation Baseline

Most recent validation at the live `origin/main` HEAD (`33252f8`,
2026-05-13 post-dep-bump-bundle):

- `cargo fmt --all --check` — clean.
- `cargo test --all-targets` — **1439 lib + 68 integration + 70 supporting**, all green locally; CI run `25774997063` confirmed the same on the Ubuntu runner in 5m10s.
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` — clean.
- `git diff --check` — clean.

Lib-test count is slightly lower than v2.1.0 (1478 → 1439) because
the v2.2.0 P0 dead-code purge removed several detector branches and
their unit tests; integration count is unchanged. See `mission-history.yaml` for the per-tag delta.

Earlier v2.1.0 validation at the release commit (for handoff):

- `cargo fmt --all --check` — clean.
- `cargo test --all-targets` — **1478 lib tests + 68 integration tests + supporting suites**, all green in 1.12s. +33 lib / +3 integration since v2.0.0 cover Slice 0 attribution-lock scenarios, Slice 1 pane-bucket aggregation, Slice 2 payoff window families, Slice 3 timestamp normalization + evidence_json migration round-trip, and Slice A-I UI synthesis surface tests.
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` — clean.
- `git diff --check` — clean.

The release pipeline gates (`scripts/release/dry-run.sh`, SBOM diff guard, etc.) inherited from v1.37.0 through v2.0.0 still apply when the v2.1.0 release workflow runs.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

Pick the next follow-up from the Active Follow-Ups list. Phase 7 D3 stays provider-blocked; tag protection / ruleset activation is operator-side. Otherwise pick the next slice from the post-v1.49.0 audit's recommended list (`.docs/claude/Qmonster-v0.4.0-2026-05-08-claude-feature-audit-and-slice-1-r1.md`): `error_burst` evidence enrichment (signal shape change), an `m`-overlay smoke render test that exercises a Gemini pane with pricing entered to lock the cost_slope activation contract.
