# Demo Fixtures

These sanitized pane tails show the kinds of provider surfaces Qmonster
parses. They are not complete provider transcripts and contain no real
workspace paths, user prompts, API keys, or model output.

Use them for screenshots, parser discussion, and README examples when a
live tmux session is not appropriate.

| File | Purpose |
| --- | --- |
| `claude-main-tail.txt` | Claude statusLine plus sidefile-style reset/cost/cache context |
| `codex-review-tail.txt` | Codex bottom status plus token/cache usage line |
| `gemini-research-tail.txt` | Gemini status table, `/stats`, and `/model` reset rows |
| `qmonster-monitor-tail.txt` | Qmonster monitor pane summary |

The real regression fixtures used by tests live under `tests/fixtures/`.
