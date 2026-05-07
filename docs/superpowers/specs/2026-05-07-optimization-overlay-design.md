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

## Purpose
To visualize Qmonster's token optimization and policy actions across 6 key situations, giving operators insight into what actions are being taken (and ignored) and the resulting token/cache savings. This addresses the need to expose the "invisible" work Qmonster does to optimize context and cost.

## Architecture
- **Trigger**: Bound to the `o` key in the main event loop to toggle the overlay.
- **State Management**: `src/app/optimization_overlay.rs` will handle the state, including asynchronous or background SQLite queries to fetch historical data without blocking the render loop.
- **Rendering**: `src/ui/optimization.rs` will handle the `tui` rendering logic, drawing the dashboard layout.

## Data Integration
- **Cache & Token Savings**: Queried from the existing SQLite `token_usage_samples` table. The overlay will calculate total token savings and generate sparklines for cache hit ratios over the selected time window (e.g., last 24h).
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
   - High-level metric: Total Token Savings (Input vs Cached).
   - Sparkline: Cache hit ratio trend over time (e.g., `▂▃▄▅▇`).

2. **Middle: Situations Triggered (24h)**
   - A grid or list of counters for the 6 situations.
   - Shows how many times Qmonster detected the signal and evaluated an action.

3. **Bottom: Recent Actions Feed**
   - A short chronological list (e.g., last 3-5 events) of tangible actions taken.
   - Example: `[Log Storm] Prompted /compact (10m ago)` or `[Quota-Tight] Switched to review (2h ago)`.
