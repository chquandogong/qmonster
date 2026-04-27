# Contributing

Qmonster is a local-first Rust TUI for observing AI CLI panes in tmux.
Keep changes scoped, observable, and consistent with the existing module
boundaries.

## Local Setup

Required tools:

- Rust 1.85+
- tmux
- Node.js 18+ and npm, only for npm package checks or publishing

Common checks:

```bash
cargo fmt --check
git diff --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
./scripts/verify-shared.sh
```

For tmux transport work, also run:

```bash
./scripts/check-tmux-source-parity.sh --repeat 2
./scripts/check-tmux-source-parity.sh --all-targets --repeat 2
```

## Documentation Contract

Canonical shared docs live in `docs/ai/`. The mission ledger lives in
`mission.yaml` and `mission-history.yaml`. Local working notes under
`.docs/` and `.mission/CURRENT_STATE.md` are useful for handoff but are
not the shared contract.

When a behavior changes, update the closest canonical doc at the same
time as the code. Do not leave README, `config/qmonster.example.toml`,
and `docs/ai/UI_MANUAL.md` describing different operator behavior.

## Safety Rules

- Qmonster observes first and recommends by default.
- Do not add destructive automation without an explicit operator gate.
- Runtime writes must stay under the resolved Qmonster root
  (`~/.qmonster/` by default).
- Provider-derived values need honest `SourceKind` labels.
- Do not infer missing provider data just to fill a UI field.

## Release Notes

Use `VERSION.md` for the version surface map. The npm package is a
source package that runs the Rust binary through Cargo, so users need a
working Rust toolchain after npm install.
