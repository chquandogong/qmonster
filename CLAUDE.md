@AGENTS.md

## Claude Code routing
Authoritative shared docs:
- docs/ai/PROJECT_BRIEF.md
- docs/ai/ARCHITECTURE.md
- docs/ai/VALIDATION.md
- docs/ai/WORKFLOWS.md
- docs/ai/REVIEW_GUIDE.md
- docs/ai/UI_MANUAL.md

Local contract / ledger:
- mission.yaml
- mission-history.yaml
- .mission/CURRENT_STATE.md

Working docs:
- .docs/claude/
- .docs/codex/
- .docs/gemini/
- .docs/final/

Rules:
- Do not treat auto memory as the primary state store.
- Read mission + current state before non-trivial planning or coding.
- Save raw planning/review docs under .docs first, then promote stable rules into docs/ai.
