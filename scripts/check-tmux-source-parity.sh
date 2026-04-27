#!/usr/bin/env bash
set -euo pipefail

cargo run --quiet --bin qmonster-tmux-parity -- "$@"
