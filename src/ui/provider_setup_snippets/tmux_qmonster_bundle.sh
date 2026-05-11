#!/usr/bin/env bash
set -euo pipefail

# Qmonster recommended tmux bundle.
# This installer writes:
#   ~/ts.sh
#   ~/.tmux/qmonster.tmux.conf
#
# Run after copying:
#   bash /path/to/copied-script.sh
# Then:
#   tmux source-file ~/.tmux/qmonster.tmux.conf
#   ~/ts.sh qmonster ~/Qmonster

mkdir -p "$HOME/.tmux"

cat > "$HOME/ts.sh" <<'QMONSTER_TS_SH'
#!/usr/bin/env bash

set -euo pipefail

usage() {
  echo "Usage: $0 <session-name> <directory>" >&2
  exit 1
}

if [[ $# -ne 2 ]]; then
  usage
fi

session_name="$1"
raw_dir="$2"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"

case "$raw_dir" in
  "~")
    target_dir="$HOME"
    ;;
  "~/"*)
    target_dir="$HOME/${raw_dir#~/}"
    ;;
  /*)
    target_dir="$raw_dir"
    ;;
  *)
    target_dir="$script_dir/$raw_dir"
    ;;
esac

if ! command -v tmux >/dev/null 2>&1; then
  echo "tmux is not installed." >&2
  exit 1
fi

if [[ ! -d "$target_dir" ]]; then
  echo "Directory not found: $target_dir" >&2
  exit 1
fi

target_dir="$(cd "$target_dir" && pwd -P)"

is_shell_command() {
  case "$1" in
    bash|sh|zsh|fish|dash|ksh|mksh|ash)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

apply_canonical_pane_titles() {
  local expected_count

  expected_count="$(tmux list-panes -t "${session_name}:0" | wc -l | tr -d ' ')"
  if [[ "$expected_count" -lt 4 ]]; then
    return
  fi

  tmux select-pane -t "${session_name}:0.0" -T "claude:1:main"
  tmux select-pane -t "${session_name}:0.1" -T "codex:1:review"
  tmux select-pane -t "${session_name}:0.2" -T "gemini:1:research"
  tmux select-pane -t "${session_name}:0.3" -T "qmonster:1:monitor"
}

sync_pane_directories() {
  local pane_id pane_cmd pane_path quoted_dir

  quoted_dir="$(printf '%q' "$target_dir")"

  while IFS=$'\t' read -r pane_id pane_cmd pane_path; do
    if [[ "$pane_path" == "$target_dir" ]]; then
      continue
    fi

    if is_shell_command "$pane_cmd"; then
      tmux send-keys -t "$pane_id" "cd -- $quoted_dir" C-m
    fi
  done < <(tmux list-panes -t "${session_name}:0" -F $'#{pane_id}\t#{pane_current_command}\t#{pane_current_path}')
}

if ! tmux has-session -t "$session_name" 2>/dev/null; then
  tmux new-session -d -s "$session_name" -c "$target_dir"
  if [[ -f "$HOME/.tmux/qmonster.tmux.conf" ]]; then
    tmux source-file "$HOME/.tmux/qmonster.tmux.conf"
  fi
  tmux split-window -h -c "$target_dir" -t "${session_name}:0"
  tmux split-window -v -c "$target_dir" -t "${session_name}:0.0"
  tmux split-window -v -c "$target_dir" -t "${session_name}:0.1"

  apply_canonical_pane_titles
  tmux select-layout -t "${session_name}:0" tiled
elif [[ "$(tmux list-panes -t "${session_name}:0" | wc -l)" -eq 4 ]]; then
  apply_canonical_pane_titles
  tmux select-layout -t "${session_name}:0" tiled
fi

apply_canonical_pane_titles
sync_pane_directories

if [[ -n "${TMUX:-}" ]]; then
  exec tmux switch-client -t "$session_name"
else
  exec tmux attach -t "$session_name"
fi
QMONSTER_TS_SH

chmod 0755 "$HOME/ts.sh"

cat > "$HOME/.tmux/qmonster.tmux.conf" <<'QMONSTER_TMUX_CONF'
# qmonster.tmux.conf.example
# Version: v0.4.0
# Date: 2026-04-20

# Keep default prefix
set -g prefix C-b
bind C-b send-prefix

# Add secondary prefix
set -g prefix2 C-g
bind C-g send-prefix -2

set -g mouse on
set -g history-limit 200000
#set -g base-index 1
#setw -g pane-base-index 1
set -sg escape-time 0
set -g renumber-windows on
set -g set-titles on
set -g allow-rename off
setw -g automatic-rename off
setw -g mode-keys vi
set -g status-keys vi

# Helper: rename current pane title
bind T command-prompt -p "Pane title" "select-pane -T '%%'"

# Helper: rename current window
bind W command-prompt -p "Window name" "rename-window '%%'"

# Navigate panes quickly
bind -r h select-pane -L
bind -r j select-pane -D
bind -r k select-pane -U
bind -r l select-pane -R

# Resize panes quickly
bind -r H resize-pane -L 5
bind -r J resize-pane -D 3
bind -r K resize-pane -U 3
bind -r L resize-pane -R 5
QMONSTER_TMUX_CONF

chmod 0644 "$HOME/.tmux/qmonster.tmux.conf"

cat <<'QMONSTER_NEXT_STEPS'
Installed:
  ~/ts.sh
  ~/.tmux/qmonster.tmux.conf

Next:
  tmux source-file ~/.tmux/qmonster.tmux.conf
  ~/ts.sh qmonster ~/Qmonster
QMONSTER_NEXT_STEPS
