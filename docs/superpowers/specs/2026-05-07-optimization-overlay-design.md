# Optimization Insights Overlay Design

Status: reviewed and merged into
`docs/superpowers/specs/2026-05-07-token-insights-overlay-design.md`.

This Gemini-authored draft is retained as an input artifact. The merged
design keeps the operations-insight goal and the three-part dashboard
shape, but changes several details:

- Final key is `i` (Insights), not `o`, to match the accepted Token
  Insights product shape.
- Final module names use `insights`, not `optimization`, to avoid
  implying automatic optimization actions.
- "Total token savings" is not claimed. The merged design shows
  measured cache reuse, token growth, and estimated opportunity with
  SourceKind labels.
- SQLite reads must not block render; the merged design requires a
  cached `InsightsSnapshot` refreshed outside `ui/` rendering.

## Original Draft (superseded)

## Purpose
To visualize Qmonster's token optimization and policy actions across
6 key situations, giving operators insight into what actions are being
taken (and ignored) and the resulting token/cache reuse / cost trend.
This addresses the need to expose the "invisible" work Qmonster does to
optimize context and cost.

## Architecture
- **Trigger**: Original proposal used the `o` key. The merged design uses `i`.
- **State Management**: Original proposal used
  `src/app/optimization_overlay.rs` with asynchronous or background
  SQLite queries. The merged design uses `src/app/insights_overlay.rs`
  plus a cached `InsightsSnapshot` refreshed outside `ui/` rendering.
- **Rendering**: Original proposal used `src/ui/optimization.rs`. The
  merged design uses `src/ui/insights.rs`.

## Data Integration
- **Cache & Token Reuse**: Queried from the existing SQLite
  `token_usage_samples` table. The merged design shows cache hit ratio
  trends and measured token growth; it does not calculate exact total
  token savings.
- **The 6 Situations (Policy Actions)**: Qmonster evaluates 11 policy rules. The overlay will map these evaluations into the 6 core design situations:
  1. **Log Storm**: Repeated output detection (Phase 1·3).
  2. **Code Exploration**: Deep code-graph heuristics (Advisories).
  3. **Context Pressure**: Cache hot/cold rules + snapshot/reset (F-7).
  4. **Verbose Review**: Verbose answer detection.
  5. **Permission/Input Wait**: Input/permission block detection.
  6. **Quota-Tight Mode**: USD budget tracking and profile switching (F-9).
  *Note: These will be aggregated from the `AuditDb` or policy evaluation logs to show counts.*

## UI Layout (Dashboard/Aggregate)
The TUI overlay will be structured into three main vertical sections:

1. **Top: Token & Optimization Summary**
   - High-level metric: cache reuse / token growth (not exact total token savings).
   - Sparkline: Cache hit ratio trend over time (e.g., `▂▃▄▅▇`).

2. **Middle: Situations Triggered (24h)**
   - A grid or list of counters for the 6 situations.
   - Shows how many times Qmonster detected the signal and evaluated an action.

3. **Bottom: Recent Actions Feed**
   - A short chronological list (e.g., last 3-5 events) of tangible actions taken.
   - Example: `[Log Storm] Prompted /compact (10m ago)` or `[Quota-Tight] Switched to review (2h ago)`.
