# MDR DRAFT — P2-1: Anomaly subsystem keep / trim / default-on

**Status:** DRAFT — measurement-pending. Do not promote to active decision until P1-3 telemetry has accumulated at least 2 weeks of operator engagement data.

**Authored:** 2026-05-11 (post v2.2.0 P1-3 implementation)

**Author:** Claude (post-evaluation improvement pass)

---

## Context

The v2.2.0 critical evaluation flagged the anomaly subsystem (8 detectors + edge-triggered dedup + persistence + UI overlays) as the largest "high-engineering / unmeasured-value" surface in Qmonster. The detector × provider eligibility matrix surfaced in P1-1 made the asymmetry explicit:

| Detector             | Supported providers     | Asymmetry                             |
| -------------------- | ----------------------- | ------------------------------------- |
| IdentityChurn        | Claude, Codex, Gemini   | symmetric                             |
| ErrorBurst           | Claude, Codex, Gemini   | symmetric                             |
| CacheDiscontinuity   | Claude, Codex           | Gemini OAuth structurally unsupported |
| CrossPaneEditCluster | Claude, Codex           | Gemini no tool-marker surface         |
| CostSlope            | all 3 (pricing-curated) | Claude/Gemini require pricing.toml    |
| TokenSlope           | Codex only              | provider gap                          |
| MemoryGrowth         | Gemini only             | provider gap                          |
| SubagentSideEffect   | Claude only             | provider gap + correlation-only       |

`anomaly_enabled` defaults to **false**, so most operators have never seen the detection system fire. Even when enabled, the per-detector value to a single operator on a single primary pane is unverified — the 1024 MB / 20 USD/hr / 20,000 tokens / 3 flips thresholds are all intuition-driven (P0-3 provenance audit).

P1-3 added `RecommendationOutcome::Copied` plus a ledger write on `y`-copy so operator engagement with each rule can be measured for the first time.

## Decision options

### Option A — Default-on, conservative thresholds (recommended if engagement is positive)

Flip `anomaly_enabled = true` in `[anomaly]` default and keep current thresholds. Operators who find detectors noisy can lower individual sensitivity via per-detector promote thresholds. Net effect: detection becomes visible to the median operator without requiring discovery.

**Engagement criterion to support this option:** ≥30% of fired anomaly recommendations receive a Copied/Accepted outcome within 5 minutes of surface time. Below this, the subsystem is generating more noise than insight.

### Option B — Trim asymmetric detectors

Remove `MemoryGrowth`, `TokenSlope`, `SubagentSideEffect` from the codebase entirely. They each fire on only one provider, with `MemoryGrowth` and `TokenSlope` thresholds that have no measurement basis. Net effect: ~600 LOC removal, 4 detectors covering all 3 providers symmetrically remain.

**Engagement criterion to support this option:** asymmetric detectors show <10% Copied/Accepted rate across the 2-week window.

### Option C — Status quo (`anomaly_enabled = false` default)

Keep the subsystem as opt-in advanced surface; rely on Settings overlay discoverability and operator self-tuning. No code changes. Net effect: cognitive load on operators discovering the surface remains the same as today.

**Engagement criterion to support this option:** mixed signal — some detectors useful, others noisy, no clear way to prune asymmetrically.

## Measurement plan

Before promoting this draft to an active decision:

1. **Run for 2 weeks** with P1-3 ledger writes active and `anomaly_enabled = true` on the author's primary tmux session.
2. **Query** `recommendation_outcomes` table for each `action` whose pattern matches `anomaly: %`:
   ```sql
   SELECT action, outcome, COUNT(*) as n
     FROM recommendation_outcomes
    WHERE action LIKE 'anomaly:%'
    GROUP BY action, outcome
    ORDER BY action, n DESC;
   ```
3. **Compute** engagement rate per detector kind:
   - copied / total_surfaced
   - accepted / total_surfaced (via correlated PromptSend acceptance)
   - ignored / total_surfaced (TTL expiry without ack)
4. **Compare** against the criterion thresholds above and select Option A / B / C accordingly.

## Risks

- **2-week minimum is non-negotiable.** Below that, the sample is dominated by author bias (the author of this MDR is also the operator).
- **Single-operator data is statistically thin.** This is a known limitation of a single-user local tool; the decision is necessarily author-influenced.
- **Default-on flip is operator-visible.** If Option A passes the gate, the v2.3.0 release notes must call out the new default explicitly.

## What this MDR does NOT decide

- Per-detector threshold tuning. Each threshold (e.g., `anomaly_cost_slope_usd_per_hour = 20.0`) should be revisited individually based on its own copied/ignored ratio. That's a follow-up MDR per detector.
- Whether to add new detectors. New detectors must follow the P1-1 supports_providers contract from day one.
- UI consolidation (m/n/i overlays). Explicitly out of scope per operator instruction during v2.2.0 implementation pass (P2-2 deferred).
