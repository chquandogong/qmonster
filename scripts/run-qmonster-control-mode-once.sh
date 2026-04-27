#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

for arg in "$@"; do
  case "$arg" in
    --config | --config=*)
      printf 'qmonster: this helper owns --config; pass --root/--set only\n' >&2
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
