#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
qmonster_root="${QMONSTER_ROOT:-"$HOME/.qmonster"}"
config_dir="$qmonster_root/config"
config_path="${QMONSTER_CONFIG:-"$config_dir/qmonster.toml"}"
pricing_path="${QMONSTER_PRICING:-"$config_dir/pricing.toml"}"

mkdir -p "$config_dir"

if [[ ! -f "$config_path" ]]; then
  cp "$repo_root/config/qmonster.example.toml" "$config_path"
  printf 'qmonster: created default config at %s\n' "$config_path" >&2
fi

if [[ ! -f "$pricing_path" ]]; then
  cp "$repo_root/config/pricing.example.toml" "$pricing_path"
  printf 'qmonster: created pricing template at %s; fill non-zero rates to enable COST badges\n' "$pricing_path" >&2
fi

if [[ -n "${QMONSTER_BIN:-}" ]]; then
  exec "$QMONSTER_BIN" --config "$config_path" "$@"
elif [[ -x "$repo_root/target/release/qmonster" ]]; then
  cd "$repo_root"
  cargo build --release >/dev/null
  exec "$repo_root/target/release/qmonster" --config "$config_path" "$@"
elif [[ -x "$repo_root/target/debug/qmonster" ]]; then
  cd "$repo_root"
  cargo build >/dev/null
  exec "$repo_root/target/debug/qmonster" --config "$config_path" "$@"
else
  cd "$repo_root"
  exec cargo run -- --config "$config_path" "$@"
fi
