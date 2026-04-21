#!/usr/bin/env bash
set -euo pipefail

if command -v mission-spec >/dev/null 2>&1; then
  ms_cmd=(mission-spec)
elif [[ -n "${MISSION_SPEC_CLI:-}" ]]; then
  ms_cmd=(node "$MISSION_SPEC_CLI")
else
  echo "mission-spec is required in PATH or via MISSION_SPEC_CLI=/abs/path/to/mission-spec.js" >&2
  exit 1
fi

cargo build
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
"${ms_cmd[@]}" validate .
