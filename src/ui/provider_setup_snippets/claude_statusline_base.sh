#!/usr/bin/env bash
# Claude Code statusline.
#
# Outputs (visible in the pane):
#   <model>·<effort>  CTX <pct>%  cache <pct>%  5h <pct>%  7d <pct>%  <worktree>
#
# Wire in ~/.claude/settings.json:
#   {
#     "statusLine": {
#       "type": "command",
#       "command": "$HOME/.claude/statusline.sh"
#     }
#   }
#
# Requires `jq`. Missing fields render dim "—".

set -u
command -v jq >/dev/null 2>&1 || { printf 'statusline: jq not installed' >&2; exit 0; }

input=$(cat)

DIM=$'\033[2m'
RESET=$'\033[0m'

abbrev_path() {
  case "$1" in
    "$HOME") printf '~' ;;
    "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;;
    *) printf '%s' "$1" ;;
  esac
}

get() { printf '%s' "$input" | jq -r "$1 // empty" 2>/dev/null; }

fmt_pct() {
  if [ -z "$1" ]; then printf '%s—%s' "$DIM" "$RESET"; return; fi
  case "$1" in
    ''|*[!0-9.]*) printf '%s%s%s' "$DIM" "$1" "$RESET" ;;
    *) printf '%.0f%%' "$1" 2>/dev/null || printf '%s%s%s' "$DIM" "$1" "$RESET" ;;
  esac
}

model=$(get '.model.display_name')
effort=$(get '.effort.level')
ctx=$(get '.context_window.used_percentage')
r5h=$(get '.rate_limits.five_hour.used_percentage')
r7d=$(get '.rate_limits.seven_day.used_percentage')
worktree=$(get '.worktree.path'); [ -n "$worktree" ] || worktree=$(get '.workspace.current_dir')

# F-7-config note: cache hit ratio is what Qmonster's CACHE badge shows.
# Computed as cache_read / (input + cache_read) per F-4 contract.
cache_read=$(get '.context_window.current_usage.cache_read_input_tokens')
input_tokens=$(get '.context_window.current_usage.input_tokens')

cache_pct=""
if [ -n "$cache_read" ] && [ -n "$input_tokens" ]; then
  cache_pct=$(awk -v r="$cache_read" -v i="$input_tokens" '
    BEGIN {
      total = r + i
      if (total > 0) printf "%.0f", (r / total) * 100
    }')
fi

head=""
if [ -n "$model" ]; then
  head="$model"
  [ -n "$effort" ] && head="$head·$effort"
fi

printf '%s' "$head"
printf '  %sCTX%s %s' "$DIM" "$RESET" "$(fmt_pct "$ctx")"
[ -n "$cache_pct" ] && printf '  %scache%s %s%%' "$DIM" "$RESET" "$cache_pct"
printf '  %s5h%s %s'  "$DIM" "$RESET" "$(fmt_pct "$r5h")"
printf '  %s7d%s %s'  "$DIM" "$RESET" "$(fmt_pct "$r7d")"
[ -n "$worktree" ] && printf '  %s' "$(abbrev_path "$worktree")"
