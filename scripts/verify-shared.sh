#!/usr/bin/env bash
set -euo pipefail

ms_cmd=()
if command -v mission-spec >/dev/null 2>&1; then
  ms_cmd=(mission-spec)
elif [[ -n "${MISSION_SPEC_CLI:-}" ]]; then
  ms_cmd=(node "$MISSION_SPEC_CLI")
fi

cargo build
cargo test --all-targets
cargo clippy --all-targets -- -D warnings

if [[ ${#ms_cmd[@]} -gt 0 ]]; then
  "${ms_cmd[@]}" validate .
else
  echo "mission-spec not found; skipping official schema validation." >&2
  echo "Install it in PATH or set MISSION_SPEC_CLI=/abs/path/to/mission-spec.js." >&2
  echo "Running lite ledger structure check instead." >&2
  test -f mission.yaml
  test -f mission-history.yaml
  grep -q '^mission:' mission.yaml
  grep -q '^  version:' mission.yaml
  grep -q '^  title:' mission.yaml
  grep -q '^meta:' mission-history.yaml
  grep -q '^  latest_version:' mission-history.yaml
  grep -q '^timeline:' mission-history.yaml
  echo "lite ledger structure check passed; official mission-spec validation still recommended." >&2
fi
