# Compatibility Matrix

This matrix records the versions Qmonster is designed and tested around.
It is intentionally conservative: provider CLIs change their terminal
surfaces often, so every official parser claim should be rechecked after
major provider updates.

## Required Runtime

| Component | Supported | Notes |
| --- | --- | --- |
| OS | Ubuntu/Linux | Development and CI target Linux. Other Unix-like systems are unclaimed. |
| Rust | 1.88+ | CI and release workflows install Rust 1.88.0. |
| Node.js | 18+ | Required for the npm wrapper package. Release workflow uses Node 20. |
| tmux | 3.x recommended | Polling source is the default; control-mode remains opt-in. |
| SQLite | bundled through `rusqlite` | Used for local audit and token sample storage. |

## Provider Surfaces

| Provider | Surface | Current contract |
| --- | --- | --- |
| Claude Code | statusLine + optional sidefile | Context, quota, cost, cache read/create counts, reset timestamps, session id, transcript path. |
| Codex | bottom status + optional app-server | Context, tokens, cache reads, rate-limit reset timestamps when app-server is enabled. |
| Gemini CLI | status table + `/stats model` + `/stats session` + idle `/model` capture | Context, quota, memory, token totals, tool calls, and display-only model reset rows. |
| Qmonster | own pane | Observed but never treated as a provider session. |

## Recheck Triggers

Re-run parser and UI validation when any of these changes:

- Claude Code statusLine JSON shape.
- Codex bottom status or `codex app-server` JSON-RPC shape.
- Gemini status table, `/stats`, or `/model` screen rendering.
- tmux pane metadata fields or control-mode event behavior.
- Rust MSRV, GitHub Actions runner image, or npm provenance flow.

## Local Verification

```bash
cargo fmt --all --check
git diff --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
npm pack --dry-run
```
