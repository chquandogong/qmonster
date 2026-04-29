# Place this block IMMEDIATELY AFTER `input=$(cat)` in statusline.sh.
# It writes the full Claude statusLine JSON to a per-session file
# so future tools (or Qmonster's F-5 reader) can read cache_read,
# cache_creation, resets_at, cost, transcript_path directly.

mkdir -p "$HOME/.local/share/ai-cli-status/claude"
session_id=$(printf '%s' "$input" | jq -r '.session_id // "unknown"' 2>/dev/null)
state_path="$HOME/.local/share/ai-cli-status/claude/${session_id}.json"
printf '%s' "$input" > "$state_path"
