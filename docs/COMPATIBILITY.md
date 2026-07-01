# Compatibility Matrix

This matrix records the versions Qmonster is designed and tested around.
It is intentionally conservative: provider CLIs change their terminal
surfaces often, so every official parser claim should be rechecked after
major provider updates.

## Required Runtime

| Component | Supported                  | Notes                                                                   |
| --------- | -------------------------- | ----------------------------------------------------------------------- |
| OS        | Ubuntu/Linux               | Development and CI target Linux. Other Unix-like systems are unclaimed. |
| Rust      | 1.88+                      | CI and release workflows install Rust 1.88.0.                           |
| Node.js   | 18+                        | Required for the npm wrapper package. Release workflow uses Node 20.    |
| tmux      | 3.x recommended            | Polling source is the default; control-mode remains opt-in.             |
| SQLite    | bundled through `rusqlite` | Used for local audit and token sample storage.                          |

## Provider Surfaces

| Provider            | Surface                                             | Current contract                                                                                                                                                                  |
| ------------------- | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude Code         | statusLine + optional sidefile                      | Context, quota, cost, cache read/create counts, reset timestamps, session id, transcript path.                                                                                    |
| Codex               | bottom status + codex-tui rollout JSONL             | Context, tokens, cache reads; 5h/weekly reset ETA from the rollout `rate_limits` (v3.1.0). The old `codex app-server` reset channel was removed in the narrow reduction (v3.0.0). |
| Gemini CLI          | status table                                        | Context, quota, memory, model — status-table-core only. The `/stats` + `/model` interactive enrichment was removed in the narrow reduction (v3.0.0).                              |
| Antigravity (`agy`) | canonical title / command; opt-in footer + sidefile | ObserveOnly identification; optional ctx / quota / reset via an operator-wired sidefile export (Provider Setup → `agy` tab) — never promotes to alerts or actuation.              |
| Qmonster            | own pane                                            | Observed but never treated as a provider session.                                                                                                                                 |

## Recheck Triggers

Re-run parser and UI validation when any of these changes:

- Claude Code statusLine JSON shape.
- Codex bottom status or codex-tui rollout JSONL shape.
- Gemini status table rendering.
- tmux pane metadata fields or control-mode event behavior.
- Rust MSRV, GitHub Actions runner image, or npm provenance flow.

## Local Verification

```bash
cargo fmt --all --check
git diff --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
npm pack --dry-run
```
