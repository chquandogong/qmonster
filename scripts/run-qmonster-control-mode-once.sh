#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

args=("$@")
i=0
while ((i < ${#args[@]})); do
  arg="${args[$i]}"
  case "$arg" in
    --config | --config=*)
      printf 'qmonster: this helper owns --config; pass --root/--set only\n' >&2
      exit 2
      ;;
    --once)
      printf 'qmonster: this helper owns --once; pass --root/--set only\n' >&2
      exit 2
      ;;
    --root | --set)
      if ((i + 1 >= ${#args[@]})); then
        printf 'qmonster: %s expects a value\n' "$arg" >&2
        exit 2
      fi
      i=$((i + 2))
      ;;
    --root=* | --set=*)
      i=$((i + 1))
      ;;
    *)
      printf 'qmonster: unsupported helper argument %s; pass only --root/--set\n' "$arg" >&2
      exit 2
      ;;
  esac
done

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

config_path="$tmp_dir/qmonster-control-mode-once.toml"
cat >"$config_path" <<'TOML'
[tmux]
source = "control_mode"
poll_interval_ms = 2000
capture_lines = 24
TOML

if [[ -n "${QMONSTER_BIN:-}" ]]; then
  exec "$QMONSTER_BIN" --config "$config_path" --once "$@"
elif [[ -x "$repo_root/target/release/qmonster" ]]; then
  cd "$repo_root"
  cargo build --release >/dev/null
  exec "$repo_root/target/release/qmonster" --config "$config_path" --once "$@"
elif [[ -x "$repo_root/target/debug/qmonster" ]]; then
  cd "$repo_root"
  cargo build >/dev/null
  exec "$repo_root/target/debug/qmonster" --config "$config_path" --once "$@"
else
  cd "$repo_root"
  exec cargo run -- --config "$config_path" --once "$@"
fi
