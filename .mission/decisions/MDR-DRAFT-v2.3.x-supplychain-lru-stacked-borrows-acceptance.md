# MDR DRAFT — Accept lru `IterMut` Stacked Borrows advisory; defer fix to ratatui 0.30 bump

**Status:** ACTIVE — accept now; revisit when ratatui 0.30+ bump enters a release cycle.

**Authored:** 2026-05-14 (Phase B close of v2.3.x supply-chain hardening)

**Author:** Claude (Phase B supply-chain hardening pass)

---

## Context

GitHub Dependabot opened alert #1 on chquandogong/qmonster's default branch the moment dependabot_security_updates was enabled in [v2.3.x supply-chain hardening Phase A](../../.docs/claude/v2.3.x-supplychain-hardening-plan.md). The advisory:

| Field           | Value                                                                     |
| --------------- | ------------------------------------------------------------------------- |
| Advisory        | `GHSA-rhfx-m35p-ff5j` / `RUSTSEC-2026-0002`                               |
| Title           | `IterMut` violates Stacked Borrows by invalidating internal pointer       |
| Affected        | `lru >= 0.9.0, < 0.16.3`                                                  |
| Patched         | `0.16.3`                                                                  |
| GitHub severity | **low**                                                                   |
| CVSS 3.0        | **0.0**                                                                   |
| CVSS 4.0        | **2.7** (Vulnerability Impact: Low, Exploit Potential: Unreported)        |
| CWE             | CWE-476 (NULL Pointer Dereference)                                        |
| Class           | Soundness (Stacked Borrows / Miri model) — not a reported exploitable bug |
| Published       | 2026-01-07                                                                |

### qmonster exposure

- `lru 0.12.5` enters the dependency graph **transitively** via `ratatui 0.28.1 → qmonster 2.3.0`. `cargo tree -i lru` confirms there is exactly one path.
- `grep -rn "use lru\|LruCache\|lru::" src/` returns no hits — qmonster's first-party code does not touch the `lru` crate or `LruCache::iter_mut()` directly.
- The vulnerable code path requires a caller to invoke `LruCache::iter_mut().next()` or `next_back()`. Reaching it requires ratatui's internal styled-line / glyph cache to expose `iter_mut` along a path qmonster actually drives.

### Fix path

- Patched range starts at `0.16.3`. Our current pin `ratatui = "0.28"` resolves to a ratatui line that consumes `lru ^0.12`. `cargo update -p lru` cannot select a patched version inside `^0.12`.
- ratatui crates.io currently has `0.30.0` available. ratatui has accumulated breaking changes across 0.28 → 0.29 → 0.30 in widget API, backend traits, and style representation, so the bump is a non-trivial code change across `src/ui/` rather than a one-line manifest edit.
- `cargo update -p ratatui --precise 0.30.0 --dry-run` correctly errors out under the current `^0.28` constraint, confirming the upgrade is gated by a deliberate Cargo.toml edit.

## Decision

**Accept the advisory** for the duration of the v2.3.x line. Do not:

- Force `[patch.crates-io] lru = "0.16"` — semver-breaking patch with high regression risk against ratatui 0.28's `lru 0.12` API expectations.
- Dismiss the Dependabot alert — leaving it open is the cheapest passive reminder, and dismissing risks losing the signal once the ratatui bump finally lands.

The justification for acceptance:

1. **Severity is functionally negligible.** CVSS 3.0 = 0.0 says the traditional scoring system finds no exploit path; CVSS 4.0 = 2.7 is the lowest non-zero band. The advisory is a Stacked-Borrows soundness issue, not a reported security bug. Stable rustc does not enforce Stacked Borrows at runtime.
2. **No reachable call site.** qmonster does not call `LruCache::iter_mut` directly. Reachability via ratatui internals is plausible but unverified; even if reachable, the consequence is UB-as-defined-by-Miri, not exploitable corruption.
3. **The fix is disproportionate to the risk.** A ratatui 0.30 bump is real engineering work (widget / backend / style API churn). Doing it solely to close a CVSS-2.7 soundness advisory inverts the cost/benefit ratio.
4. **The fix is naturally scheduled.** ratatui is qmonster's primary rendering dependency; a major bump is a normal late-cycle item every few releases. Carrying lru along comes for free with that bump.

## Alternatives considered

| Option                                                 | Why rejected                                                                                                                                                                      |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Force-patch `lru = "0.16"` via `[patch.crates-io]`** | API churn between 0.12 and 0.16 likely breaks ratatui 0.28's call sites. Adds maintenance debt for negligible risk reduction.                                                     |
| **Dismiss the alert with `tolerable_risk`**            | Removes the dashboard reminder before the underlying fix lands. In a 1-person operation, that means the alert is more likely to be forgotten than re-evaluated.                   |
| **Immediate ratatui 0.30 bump as a v2.3.x hot-fix**    | High disruption to ongoing UX workstreams (alerts polish, sectioned pane panel, related-context rail). No business pressure justifies inserting a major UI dep bump out of cycle. |
| **Add a `risks:` or `deferred:` key to mission.yaml**  | mission.yaml has no such top-level structure today; ad-hoc keys would be informal and easy to drift. This MDR is the canonical record.                                            |

## Revisit triggers

This MDR returns to active discussion when **any one** of the following occurs:

1. A ratatui 0.30+ bump enters a release cycle (the cleanest natural close-out).
2. The advisory is upgraded to medium+ severity, gets a new CVE / CVSS revision, or accumulates evidence of a reachable exploit on stable rustc.
3. A second `lru`-related advisory lands in the same dependency graph and the combined risk warrants a forced upgrade.
4. cargo-audit fails the release gate on this advisory (the current `cargo audit` in `.github/workflows/{ci,release}.yml` does fire on GHSAs, so a future severity bump would surface here automatically).

## Risks of acceptance

- **Reachability assumption may be wrong.** If a ratatui internal does expose `iter_mut` along a qmonster-driven path, this acceptance is technically incorrect even though the practical risk stays near zero. The mitigation is that the consequence is Miri-defined UB, not exploitable code execution.
- **Future severity revision.** A CVSS upgrade post-publication would silently make this MDR stale until the next mission cycle reviews open MDRs.
- **Acceptance creep.** This is the first acceptance MDR for the supply-chain workstream. The pattern must not normalize accepting medium/high severity advisories with the same template.

## What this MDR does NOT decide

- The timing of the ratatui 0.30+ bump itself. That is a separate scoping decision (likely a v2.4.0 feature item).
- Whether to enable cargo-audit's `--severity` gating to auto-fail builds on this specific GHSA. Current `cargo audit` invocation in CI / release does not stratify by severity; revisiting the gate behavior is its own decision.
- Whether to add a tracked `risks:` section to mission.yaml. mission.yaml's current shape (goal narrative + done_when binders) does not naturally host risk acceptance, and this MDR fills that role for now.
