# CURRENT_STATE

_Last updated: 2026-04-30 (Codex, v1.37.0 documentation sync while CI publication is pending)_

## Mission

- Title: Qmonster v1.37.0 - multi-pane orchestration, dynamic profile switching, cost budget tracking, and Phase 6 Team Mode.
- Version surfaces: mission ledger target `1.37.0`; npm package metadata `qmonster@1.37.0`; latest local Git tag `v1.37.0`.
- Branch / worktree at handoff start: `main` at `1121a21`, tag `v1.37.0`.
- Release publication state: code/tag/package metadata are staged for the v1.37.0 release; external GitHub/npm publication is being handled by CI and may still be in progress.
- Current phase: Phases 1-5, Phase B, Phase C C1/C2/C3, Phase D D1/D2/D3, Phase E E1/E2, Phase F F-1 through F-9/F-9b, Phase G G-1/G-2, and Phase 6 Team Mode are complete.

## v1.37.0 Feature State

Four backwards-compatible features are complete in the v1.37.0 source:

1. **F-8 multi-pane orchestration**: Claude `Edit` / `Write` / `MultiEdit` / related tool-call markers are parsed into file-touch observations. With `[security] cross_pane_file_findings = true`, Qmonster emits a `ConcurrentFileEdit` warning when busy Main/Review panes touch the same absolute file.
2. **F-9 dynamic profile switching**: `[profile_switch]` adds an opt-in sliding-window error-rate recommender. When enabled, repeated `error_hint` observations can recommend switching from the default provider profile to the corresponding script-low-token profile.
3. **F-9b cost budget tracking**: SQLite now records positive USD deltas from cumulative provider `cost_usd` values in `cost_usage_events`, then claims one-shot 80% and 100% budget alerts in `cost_budget_alerts`. Default budget is `$200`; `budget_usd = 0.0` disables budget alerts.
4. **Phase 6 Team Mode**: config loading now merges defaults, shared `qmonster.toml`, then sibling `qmonster.local.toml`. Local overrides are sparse, recursive TOML overlays and stay gitignored.

## Latest Release Notes

- `v1.37.0` is a minor release over the v1.36.x release-pipeline hardening line.
- Local tag: `v1.37.0` points at `1121a21`.
- Commit summary: `v1.37.0 multi-pane orchestration + dynamic profile switching + cost budget + Team Mode`.
- The prior good published line was `v1.36.8`, which synced the cargo-audit pin and contributors metadata after the v1.36.7 release workflow retry.

## Known External State

- CI publication for `v1.37.0` may still be running. Do not claim GitHub Release or npm publication success until the workflow and registry are checked.
- `qmonster@1.36.2` remains deprecated on npm because its GitHub Release SBOM was incomplete.
- GitHub Release `v1.36.2` remains marked prerelease with a warning banner.

## Active Follow-Ups

1. Verify the CI-published `v1.37.0` GitHub Release and npm package after the release workflow completes.
2. Tag protection/ruleset activation remains a GitHub Settings task.
3. MF-2 remains deferred: `evolution_summary.current_phase` length cleanup.
4. Phase H remains unscoped: opt-in auto-snapshot at reset boundary.
5. Phase 7 planning is now opened by `.docs/codex/phase7-anomaly-detection-spec.md` for anomaly detection and subagent analysis.

## Validation Baseline

Most recent v1.37.0 validation reported in the release commit:

- `cargo fmt --all --check`
- `git diff --check`
- `cargo test --all-targets`
- `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args`
- `scripts/release/dry-run.sh v1.37.0` passed npm pack, npm publish dry-run, SBOM 256/256 purl coverage, and SBOM diff guard versus v1.36.8.

Use `docs/ai/VALIDATION.md` for the full gate list before any future tagged release.

## Next First Action

After CI finishes, verify the published `v1.37.0` release assets, npm package, checksums, SBOM, and provenance, then update this state file with the concrete workflow run IDs and publication URLs.
