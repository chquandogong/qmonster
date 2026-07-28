#!/usr/bin/env bash
# hs.sh — idempotent herdr launcher for the Qmonster operator layout.
#
#   hs.sh <project-dir> [--name <workspace>] [--no-agents] [--dry-run]
#
# One run ensures:
#   1. a global "0-Monitor" workspace exists and runs Qmonster
#      (created once; an existing 0-Monitor is NEVER touched), and
#   2. a project workspace (label = --name or the directory basename)
#      with tabs 1-Claude / 2-Codex / 3-Agy, each split into top/bottom
#      panes, the top pane running the operator's shell alias
#      (ctcd / ccd / cgd — resolved by the interactive shell, so
#      ~/.bashrc stays the single source of truth) and labeled
#      claude:1:main / codex:1:review / agy:1:research.
#
# Idempotent: re-runs create only what is missing; panes of existing
# tabs are never sent any input. Text is only ever sent to a pane
# whose foreground process is verified to be an interactive shell.
#
# tmux users: ts.sh remains the tmux-flavored companion script.

set -euo pipefail

usage() {
  echo "Usage: $0 <project-dir> [--name <workspace>] [--no-agents] [--dry-run]" >&2
  exit 1
}

MONITOR_LABEL="${HS_MONITOR_LABEL:-0-Monitor}" # env override is a test hook
AGENT_TABS=("1-Claude" "2-Codex" "3-Agy")
AGENT_ALIASES=("ctcd" "ccd" "cgd")
PANE_LABELS=("claude:1:main" "codex:1:review" "agy:1:research")

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd "$script_dir/.." && pwd -P)"

raw_dir=""
ws_name=""
no_agents=0
dry_run=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name)
      [[ $# -ge 2 ]] || usage
      ws_name="$2"
      shift 2
      ;;
    --no-agents)
      no_agents=1
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    -*)
      usage
      ;;
    *)
      [[ -z "$raw_dir" ]] || usage
      raw_dir="$1"
      shift
      ;;
  esac
done
[[ -n "$raw_dir" ]] || usage

case "$raw_dir" in
  "~") target_dir="$HOME" ;;
  "~/"*) target_dir="$HOME/${raw_dir#~/}" ;;
  /*) target_dir="$raw_dir" ;;
  *) target_dir="$PWD/$raw_dir" ;;
esac
if [[ ! -d "$target_dir" ]]; then
  echo "hs.sh: directory not found: $target_dir" >&2
  exit 1
fi
target_dir="$(cd "$target_dir" && pwd -P)"
[[ -n "$ws_name" ]] || ws_name="$(basename "$target_dir")"
if [[ "$ws_name" == "$MONITOR_LABEL" ]]; then
  echo "hs.sh: project workspace may not be named $MONITOR_LABEL" >&2
  exit 1
fi

command -v herdr >/dev/null 2>&1 || { echo "hs.sh: herdr not installed" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "hs.sh: jq is required" >&2; exit 1; }
if ! herdr status 2>/dev/null | grep -q "status: running"; then
  echo "hs.sh: herdr server is not running (try: herdr)" >&2
  exit 1
fi

# hx — mutating herdr command: echoed in dry-run, executed otherwise.
# Read-only queries always run so the plan is computed from live state.
hx() {
  if [[ "$dry_run" -eq 1 ]]; then
    echo "DRY-RUN: herdr $*"
  else
    herdr "$@" >/dev/null
  fi
}

ws_id_by_label() {
  local ids count
  ids="$(herdr workspace list 2>/dev/null |
    jq -r --arg l "$1" '.result.workspaces[] | select(.label == $l) | .workspace_id')"
  count="$(printf '%s' "$ids" | grep -c . || true)"
  if [[ "$count" -gt 1 ]]; then
    # Labels are display strings, not unique keys — refusing beats
    # silently picking one duplicate and mutating the wrong workspace.
    echo "hs.sh: ERROR workspace label '$1' is ambiguous ($count matches: $(echo "$ids" | tr '\n' ' '))— rename duplicates or pass a unique --name" >&2
    exit 1
  fi
  printf '%s\n' "$ids" | head -1
}

tab_id_by_label() { # <workspace_id> <label>
  herdr tab list 2>/dev/null |
    jq -r --arg w "$1" --arg l "$2" \
      '.result.tabs[] | select(.workspace_id == $w and .label == $l) | .tab_id' |
    head -1
}

first_unlabeled_tab() { # <workspace_id> — herdr default-labels tabs "1","2",…
  herdr tab list 2>/dev/null |
    jq -r --arg w "$1" \
      '.result.tabs[] | select(.workspace_id == $w and ((.label // "") | test("^[0-9]*$"))) | .tab_id' |
    head -1
}

panes_in_tab() { # <tab_id> → pane ids, herdr order (first = original/top)
  herdr pane list 2>/dev/null |
    jq -r --arg t "$1" '.result.panes[] | select(.tab_id == $t) | .pane_id'
}

panes_in_workspace() { # <workspace_id>
  herdr pane list 2>/dev/null |
    jq -r --arg w "$1" '.result.panes[] | select(.workspace_id == $w) | .pane_id'
}

pane_fg_name() { # <pane_id>
  herdr pane process-info --pane "$1" 2>/dev/null |
    jq -r '.result.process_info.foreground_processes[0].name // ""'
}

pane_is_shell() { # <pane_id> — interactive-shell PROOF, not just a name
  # A shell running a script or `-c` command has extra argv entries;
  # only a bare interactive invocation (argv = [shellname] or
  # [-shellname] for login shells) may receive text. Same name
  # allowlist as ts.sh.
  local line name argc
  line="$(herdr pane process-info --pane "$1" 2>/dev/null |
    jq -r '.result.process_info.foreground_processes[0] | "\(.name // "")\t\((.argv // []) | length)"')"
  name="${line%%$'\t'*}"
  argc="${line##*$'\t'}"
  case "$name" in
    bash | sh | zsh | fish | dash | ksh | mksh | ash) ;;
    *) return 1 ;;
  esac
  [[ "${argc:-99}" =~ ^[0-9]+$ ]] && [[ "$argc" -le 1 ]]
}

wait_for_shell() { # <pane_id> — new panes need a beat to spawn the shell
  local i
  for i in $(seq 1 20); do
    pane_is_shell "$1" && return 0
    sleep 0.25
  done
  return 1
}

wait_for_agent() { # <pane_id> <kind> — poll herdr's own agent detection
  local i agent
  for i in $(seq 1 24); do
    agent="$(herdr pane list 2>/dev/null |
      jq -r --arg p "$1" '.result.panes[] | select(.pane_id == $p) | .agent // ""')"
    [[ "$agent" == "$2" ]] && return 0
    sleep 2.5
  done
  return 1
}

# Send an alias line into a pane — ONLY after proving the foreground
# process is a bare interactive shell (hard guard against the
# historical send-keys-into-a-running-TUI incident). The alias names
# are fixed identifiers from the table above, never interpolated
# operator input, so they need no quoting; the monitor launch path
# %q-quotes its path because that IS derived from the filesystem.
launch_alias_in_pane() { # <pane_id> <alias> <kind>
  local pane_id="$1" alias_cmd="$2" kind="$3"
  if [[ "$dry_run" -eq 1 ]]; then
    hx pane send-text "$pane_id" "$alias_cmd"
    hx pane send-keys "$pane_id" enter
    return 0
  fi
  if ! wait_for_shell "$pane_id"; then
    echo "hs.sh: WARN pane $pane_id foreground is '$(pane_fg_name "$pane_id")', not a shell — skipping $alias_cmd" >&2
    return 0
  fi
  hx pane send-text "$pane_id" "$alias_cmd"
  hx pane send-keys "$pane_id" enter
  if ! wait_for_agent "$pane_id" "$kind"; then
    echo "hs.sh: WARN $kind not detected in $pane_id after launch ($alias_cmd) — check the pane" >&2
  fi
}

ensure_monitor() {
  local mid pane
  mid="$(ws_id_by_label "$MONITOR_LABEL")"
  if [[ -n "$mid" ]]; then
    echo "hs.sh: $MONITOR_LABEL exists ($mid) — untouched"
    return 0
  fi
  echo "hs.sh: creating $MONITOR_LABEL workspace + Qmonster"
  hx workspace create --label "$MONITOR_LABEL" --cwd "$repo_root"
  if [[ "$dry_run" -eq 1 ]]; then
    echo "DRY-RUN: herdr pane send-text «monitor-pane» $repo_root/scripts/run-qmonster.sh"
    echo "DRY-RUN: herdr pane send-keys «monitor-pane» enter"
    echo "DRY-RUN: herdr pane rename «monitor-pane» qmonster:1:monitor"
    return 0
  fi
  mid="$(ws_id_by_label "$MONITOR_LABEL")"
  if [[ -z "$mid" ]]; then
    echo "hs.sh: ERROR $MONITOR_LABEL did not appear after create" >&2
    return 1
  fi
  pane="$(panes_in_workspace "$mid" | head -1)"
  if [[ -z "$pane" ]] || ! wait_for_shell "$pane"; then
    echo "hs.sh: WARN no shell pane in fresh $MONITOR_LABEL — launch Qmonster manually ($repo_root/scripts/run-qmonster.sh)" >&2
    return 0
  fi
  # %q-quote the path: a checkout under a directory with spaces or
  # shell metacharacters must arrive as ONE safe word, never as
  # something the interactive shell could reinterpret.
  local launch_q
  launch_q="$(printf '%q' "$repo_root/scripts/run-qmonster.sh")"
  hx pane send-text "$pane" "$launch_q"
  hx pane send-keys "$pane" enter
  hx pane rename "$pane" "qmonster:1:monitor"
}

ensure_project() {
  local wid tab_id top_pane pane_count i
  wid="$(ws_id_by_label "$ws_name")"
  if [[ -z "$wid" ]]; then
    echo "hs.sh: creating workspace $ws_name ($target_dir)"
    hx workspace create --label "$ws_name" --cwd "$target_dir"
    if [[ "$dry_run" -eq 0 ]]; then
      wid="$(ws_id_by_label "$ws_name")"
      if [[ -z "$wid" ]]; then
        echo "hs.sh: ERROR workspace $ws_name did not appear after create" >&2
        return 1
      fi
    fi
  fi

  for i in 0 1 2; do
    local label="${AGENT_TABS[$i]}" alias_cmd="${AGENT_ALIASES[$i]}" pane_label="${PANE_LABELS[$i]}"
    local kind="${pane_label%%:*}"

    if [[ "$dry_run" -eq 1 && -z "$wid" ]]; then
      # Fresh workspace in dry-run: ids unknown, print the plan shape.
      echo "DRY-RUN: herdr tab create --workspace «$ws_name» --cwd $target_dir --label $label"
      echo "DRY-RUN: herdr pane split «$label-top» --direction down"
      echo "DRY-RUN: herdr pane rename «$label-top» $pane_label"
      [[ "$no_agents" -eq 1 ]] || {
        echo "DRY-RUN: herdr pane send-text «$label-top» $alias_cmd"
        echo "DRY-RUN: herdr pane send-keys «$label-top» enter"
      }
      continue
    fi

    tab_id="$(tab_id_by_label "$wid" "$label")"
    if [[ -n "$tab_id" ]]; then
      echo "hs.sh: tab $label exists — untouched"
      continue
    fi

    # First expected tab may claim the workspace's default tab instead
    # of creating a 4th one (herdr always creates tab 1 with a pane).
    if [[ "$i" -eq 0 ]]; then
      tab_id="$(first_unlabeled_tab "$wid")"
      if [[ -n "$tab_id" ]]; then
        hx tab rename "$tab_id" "$label"
      fi
    fi
    if [[ -z "$tab_id" ]]; then
      hx tab create --workspace "$wid" --cwd "$target_dir" --label "$label"
      if [[ "$dry_run" -eq 1 ]]; then
        echo "DRY-RUN: (split/rename/launch for $label as above)"
        continue
      fi
      tab_id="$(tab_id_by_label "$wid" "$label")"
      if [[ -z "$tab_id" ]]; then
        echo "hs.sh: WARN tab $label did not appear — skipping" >&2
        continue
      fi
    fi

    top_pane="$(panes_in_tab "$tab_id" | head -1)"
    if [[ -z "$top_pane" ]]; then
      echo "hs.sh: WARN tab $label has no pane — skipping" >&2
      continue
    fi
    pane_count="$(panes_in_tab "$tab_id" | wc -l | tr -d ' ')"
    if [[ "$pane_count" -lt 2 ]]; then
      hx pane split "$top_pane" --direction down
    fi
    hx pane rename "$top_pane" "$pane_label"
    if [[ "$no_agents" -eq 0 ]]; then
      launch_alias_in_pane "$top_pane" "$alias_cmd" "$kind"
    fi
  done
}

restore_focus() {
  # workspace/tab creation moves focus; put the operator back where
  # they started when hs.sh was run from inside herdr.
  if [[ "$dry_run" -eq 0 && -n "${HERDR_WORKSPACE_ID:-}" ]]; then
    herdr workspace focus "$HERDR_WORKSPACE_ID" >/dev/null 2>&1 || true
  fi
}

ensure_monitor
ensure_project
restore_focus
echo "hs.sh: done — workspace '$ws_name' + '$MONITOR_LABEL' ensured"
