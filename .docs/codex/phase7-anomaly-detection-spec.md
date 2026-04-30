# Phase 7 Preliminary Spec: Anomaly Detection and Subagent Analysis (D2/D3)

Date: 2026-04-30
Author: Codex
Status: preliminary planning spec; implementation not started

## Context

Qmonster already has the first D2/D3 primitives:

- D2 identity drift: per-pane `IdentitySnapshot` history detects provider/worktree drift when `[security] identity_drift_findings = true`.
- D3 subagent detection: `subagent_hint` catches provider-visible subagent markers, including Claude `Task(...)`.
- F-stack telemetry: token samples, cache samples, reset windows, process memory, agent memory files, provider runtime facts, and cost observations.
- v1.37.0 additions: file-level cross-pane findings, profile-switch error history, and normalized USD cost ledger.

Phase 7 should combine these into higher-level anomaly analysis without inventing data providers do not expose.

## Goals

1. Surface pane-level anomalies that are visible from existing telemetry:
   identity churn, repeated error bursts, unusual cost/token slope, cache/reset discontinuities, memory growth, and suspicious cross-pane edit overlap.
2. Explain subagent activity in terms of confidence and observable side effects, not fake attribution.
3. Produce recommendations through the existing pure-policy model and existing UI/effect paths.
4. Keep all thresholds operator-tunable or clearly ProjectCanonical defaults.

## Non-Goals

- No per-subagent input/output/cost split unless Claude, Codex, or Gemini exposes structured per-subagent counters.
- No raw-tail storage in audit tables.
- No automatic provider reconfiguration, `/compact`, `/clear`, or prompt sending.
- No ML model dependency in v1. The first implementation should be deterministic rules over existing samples.

## Proposed Data Model

Add a pure domain summary type, likely under `src/domain/anomaly.rs`:

```rust
pub struct AnomalySignal {
    pub kind: AnomalyKind,
    pub confidence: AnomalyConfidence,
    pub evidence: Vec<AnomalyEvidence>,
    pub window_polls: usize,
}

pub enum AnomalyKind {
    IdentityChurn,
    ErrorBurst,
    CostSlope,
    TokenSlope,
    CacheDiscontinuity,
    MemoryGrowth,
    SubagentSideEffect,
    CrossPaneEditCluster,
}
```

Evidence should reference summarized facts only: pane id, provider,
metric names, sample counts, before/after values, and SourceKind. It
must not embed raw tail text.

## Rule Sketch

- Identity churn: build on D2 history, but detect repeated provider/path flips within a rolling window instead of only one drift edge.
- Error burst: generalize F-9 profile-switch history into a reusable recent-error window.
- Cost slope: compare `cost_usage_events` deltas over a configurable window; fire when spend rate exceeds a per-hour or per-N-polls budget threshold.
- Token slope: use `recent_token_samples` deltas; detect sudden input/output growth not explained by cache hit ratio.
- Cache discontinuity: reuse F-7b cache drift logic, but classify it as an anomaly when repeated or paired with high token slope.
- Memory growth: compare `process_memory_mb` and `agent_memory_bytes` across samples when available.
- Subagent side effect: when `subagent_hint` appears, annotate nearby token/cost/error changes as correlated, not attributed.
- Cross-pane edit cluster: treat repeated `ConcurrentFileEdit` findings across a short window as an elevated coordination anomaly.

## Configuration

Add a future `[anomaly]` section with conservative defaults:

```toml
[anomaly]
enabled = false
window_polls = 20
min_confidence = "medium"
cost_slope_usd_per_hour = 20.0
token_slope_input_per_poll = 20000
error_rate_threshold = 0.5
memory_growth_mb = 1024
```

Keep the first release opt-in. Defaults should be documented in
`config/qmonster.example.toml` and surfaced read-only in Settings before
editing support is considered.

## UI and Policy Contract

- Severity defaults to `Concern`; promote to `Warning` only when a rule
  has strong evidence and the operator has opted into the anomaly layer.
- Use one concise recommendation per pane per anomaly kind per window.
- Show evidence rows in the pane detail view, not in the alert title.
- Reuse `RequestedEffect::Notify` only for Warning+ anomalies.
- Dedup by `(pane_id, anomaly_kind, window_start)` or an equivalent
  stable key.

## Storage

Prefer no new table in the first slice. Use existing in-memory rolling
state plus existing SQLite samples:

- `token_usage_samples` for token/cache/cost cumulative observations;
- `cost_usage_events` for normalized spend deltas;
- current in-memory identity/profile-switch histories for edge and
  error-burst state.

Add a table only if the UI needs cross-session anomaly continuity. If a
table is added later, it should store summaries and evidence metadata,
not raw tail bytes.

## Acceptance Criteria

- Pure rule tests for each anomaly kind implemented in Phase 7 v1.
- Event-loop integration proving one anomaly reaches `PaneReport`
  without notifying below Warning.
- Subagent analysis tests explicitly assert correlation language and no
  per-subagent token/cost attribution.
- Config tests prove `[anomaly]` defaults and sparse TOML overrides.
- `cargo test --all-targets`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --all --check`, and `git diff --check` pass.

## Open Questions

- Should cost-slope thresholds be absolute USD/hour or percentage of
  `[cost] budget_usd` per hour?
- Should identity churn include pane title changes or only resolved
  provider/worktree drift?
- Should anomaly summaries persist across restarts, or is per-session
  analysis enough for Phase 7 v1?
- Should cross-pane edit clusters require same branch, same file, or
  same file plus same Git diff hunk once a diff parser exists?
