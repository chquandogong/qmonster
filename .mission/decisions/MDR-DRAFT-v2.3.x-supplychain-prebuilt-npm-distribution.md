# MDR DRAFT — npm primary distribution: source + local cargo build vs prebuilt binary

**Status:** DRAFT — decision-pending. Capture the trade-offs now while the v2.3.x supply-chain hardening context is fresh; final pick deferred to the maintainer.

**Authored:** 2026-05-14 (Phase D follow-up to the 2026-05-14 supply-chain incident-response report §10.4)

**Author:** Claude (Phase D scoping pass)

---

## Context

`npm install qmonster` currently fetches a source tarball that wraps `Cargo.toml` + `Cargo.lock` + `src/`; the first invocation of `qmonster` runs `cargo build --release` locally, compiles ~270 transitive crates, and produces a binary under the user's Cargo cache. The npm package itself ships zero lifecycle scripts and zero JS dependencies — the `npm/bin/qmonster` wrapper is a thin shell that invokes `cargo`.

This shape was chosen deliberately:

- Zero registry-time code execution. `ci.yml`'s "Verify npm package has no lifecycle scripts or runtime deps" guard enforces the shape on every push / PR.
- Reproducible source distribution. The published tarball is exactly what was tagged.
- Maintainer trust surface = the source + lockfile only. We do not ship a binary the maintainer compiled on a particular machine.

The 2026-05-14 supply-chain incident-response report (`.docs/claude/v2.3.x-supplychain-incident-response-report.md` §10.4) flags this as the largest remaining user-side risk: every `npm install qmonster` execution exposes the user to the transitive crate graph, including each crate's `build.rs`. Even with `cargo audit` + Dependabot, a 0-day `build.rs` backdoor in any of ~270 crates would execute on every fresh install.

The v2.3.x release pipeline already produces a prebuilt Linux x86_64 binary tarball (`qmonster-vX.Y.Z-linux-x86_64.tar.gz`) with SLSA build provenance attached. The question is whether to make that binary the primary npm distribution.

## Decision options

### Option A — Source-only npm (status quo)

Keep `qmonster` as a source distribution. README notice (Phase D1.3) tells operators how to download the verified prebuilt tarball if they prefer.

- **Pro:** zero ci-side build complexity. Zero lifecycle-script surface. Cross-platform out of the box (any platform with cargo).
- **Pro:** consistent with the principle "Qmonster's trust surface = the source the maintainer signs".
- **Con:** every user pays the full transitive-crate trust cost on install. cargo-vet (D3.2) is the natural complement here but is itself a multi-day initiative.
- **Con:** `npm install` mental model mismatch — most npm packages do not invoke a Rust toolchain. New users hit "rustc 1.88+ required" friction.

### Option B — Sibling `qmonster-bin` prebuilt package

Publish a second npm package, `qmonster-bin`, that ships prebuilt binaries (Linux x86_64 first; macOS/Windows later if needed). `qmonster-bin` is a thin npm wrapper that resolves the binary at install time via `postinstall` script OR via a binary-resolution npm pattern like `optionalDependencies` with per-platform packages (the `esbuild` / `napi-rs` pattern).

Implementation sketch:

- `qmonster-bin/package.json` declares `optionalDependencies` for each platform-specific sibling (`@qmonster/linux-x64`, `@qmonster/darwin-arm64`, …).
- Each sibling package contains exactly one prebuilt binary + `package.json` with `os` / `cpu` constraints. npm picks the right sibling automatically.
- `qmonster-bin/bin/qmonster` is a node wrapper that exec's the platform-specific binary.
- No `postinstall` script anywhere — preserves the lifecycle-script guard.
- Release workflow extends to publish 1 + N packages (the meta package + each platform binary) all through Trusted Publishers OIDC with provenance.

- **Pro:** users opt into prebuilt by choosing `qmonster-bin` instead of `qmonster`. Original source distribution stays available.
- **Pro:** binaries carry SLSA provenance verifiable with `gh attestation verify` or `npm audit signatures`.
- **Pro:** no conflict with the existing lifecycle-script guard.
- **Con:** publishes ~5 packages per release (meta + per-platform). Each needs its own Trusted Publisher entry on npmjs.com.
- **Con:** Linux-only at first means non-Linux users either keep installing `qmonster` (source) or wait for macOS/Windows builds. Adds cross-platform CI matrix.
- **Con:** new packages means new attack surface for typosquatting (e.g. `qmonster-bin` vs `qmonster_bin`).

### Option C — Add `postinstall` to the existing `qmonster` package that fetches the GitHub Release binary + verifies provenance

Single package. `postinstall` script downloads the verified binary from GitHub Releases at install time.

- **Pro:** single package, single TP entry, single dist-tag.
- **Con:** **directly conflicts** with the v2.3.x A6 lifecycle-script guard in `ci.yml`. Reversing that guard widens the registry-time code execution surface.
- **Con:** GitHub Release download requires network at install time. Air-gapped install path breaks.
- **Con:** `postinstall` is a classic supply-chain attack vector — exactly what the guard exists to prevent.
- **Verdict:** rejected on principle. The guard is the right defense; reversing it for a single feature is the wrong trade.

## Decision (proposed)

**Defer to Option A for the v2.3.x line.** Re-open as Option B if user friction over the cargo build path becomes a real signal (downstream issues, slow adoption, repeated install failures on first run), and revisit jointly with the cargo-vet (D3.2) initiative since both address the same risk surface from different angles.

The Phase D1.3 README install notice (committed in `f9cf8a8`) is the minimum viable user education for Option A today: it tells operators exactly what they are downloading and points them at the verified prebuilt tarball if they prefer.

## Trade-offs at the decision boundary

| Axis                                    | Option A (status quo)                                | Option B (prebuilt sibling)                       |
| --------------------------------------- | ---------------------------------------------------- | ------------------------------------------------- |
| User trust surface                      | source + Cargo.lock + local toolchain + all build.rs | prebuilt binary + maintainer's CI environment     |
| Maintainer trust surface                | source the maintainer signs                          | source + CI runner environment per platform       |
| npm packages to publish                 | 1                                                    | 1 + N (where N = platforms)                       |
| Trusted Publisher entries on npmjs.com  | 1 (current)                                          | 1 + N                                             |
| Install time on user side               | ~minutes (cargo build)                               | seconds                                           |
| Air-gapped install                      | ✓ (Cargo lockfile only)                              | optional sibling fallback to source needed        |
| Lifecycle-script CI guard compatibility | ✓                                                    | ✓ (no postinstall)                                |
| SLSA provenance verifiable end to end   | partial (tarball + lockfile, not binary)             | full (binary itself signed)                       |
| Adoption complexity                     | none                                                 | CI matrix + per-platform packaging + TP setup × N |

## Implementation cost (if Option B is selected)

Rough order of magnitude — not a commitment.

- CI matrix expansion: 2-3 days (Linux already done; macOS-arm64 and macOS-x64 require new runners + cross-toolchain). Windows is bigger again.
- Per-platform npm package scaffolding + TP registration on npmjs.com: 1 day.
- Release workflow split (meta package + platform packages, all OIDC-published): 1-2 days.
- Operator docs + migration period (both `qmonster` and `qmonster-bin` published in parallel for at least one minor): rolling.
- Total: ~1 week of focused work.

## Revisit triggers

Re-open this MDR when **any one** of the following occurs:

1. cargo-vet (D3.2) is adopted and the unaudited transitive-crate count drops below a manageable threshold — that alone removes most of the Option A risk, so Option B becomes less urgent.
2. Aggregate user-reported install friction on `npm install qmonster` exceeds a low threshold (e.g. > 3 GitHub issues citing "rustc not found" or first-build failures in a quarter).
3. A real 0-day in a popular Rust crate's `build.rs` lands and the project sees collateral damage during the fix window.
4. The maintainer takes on a co-maintainer (D4.1) and CI matrix expansion stops being a solo time-cost.

## Risks of deferring

- The 270+ transitive-crate `build.rs` risk stays on the user side. Mitigation: cargo-audit + Dependabot catch known-advisory cases; D1.3 README notice sets expectations.
- npm install friction stays. Mitigation: README notice + RELEASE_VERIFICATION docs point users at the prebuilt path.

## What this MDR does NOT decide

- Whether to adopt cargo-vet (that is D3.2, a separate workstream — they address the same risk but are not mutually exclusive).
- The exact cross-platform target list if Option B is selected (Linux-x64 only? + macOS-arm64? + Windows?). Defer to a follow-up MDR scoped to the platform matrix.
- Whether to retire the existing source `qmonster` package after `qmonster-bin` matures. Default: keep both indefinitely; source distribution remains the most reproducible build path.
- Sigstore-vs-`gh attestation verify` end-user UX. The verification commands are documented in `docs/RELEASE_VERIFICATION.md`; tooling-side enforcement is out of scope for any maintainer-owned change.
