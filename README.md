<p align="center">
  <img src="docs/assets/qmonster-banner.svg" alt="Qmonster: observe-first TUI for Claude, Codex, and Gemini tmux panes" width="100%">
</p>

<h1 align="center">Qmonster</h1>

<p align="center">
  <strong>Observe-first Rust TUI for multi-CLI tmux development.</strong>
</p>

<p align="center">
  <a href="https://github.com/chquandogong/qmonster/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/chquandogong/qmonster/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/chquandogong/qmonster/releases"><img alt="GitHub release" src="https://img.shields.io/github/v/release/chquandogong/qmonster?display_name=tag&sort=semver"></a>
  <a href="https://www.npmjs.com/package/qmonster"><img alt="npm version" src="https://img.shields.io/npm/v/qmonster?label=npm"></a>
  <a href="https://github.com/chquandogong/qmonster/pkgs/npm/qmonster"><img alt="GitHub Packages mirror" src="https://img.shields.io/badge/GitHub%20Packages-%40chquandogong%2Fqmonster-2f81f7?logo=github"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/github/license/chquandogong/qmonster"></a>
  <img alt="Rust 1.88+" src="https://img.shields.io/badge/Rust-1.88%2B-b7410e?logo=rust">
  <img alt="tmux" src="https://img.shields.io/badge/tmux-observe--first-1bb91f">
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a>
  · <a href="#what-it-shows">What It Shows</a>
  · <a href="#data-contracts">Data Contracts</a>
  · <a href="https://github.com/chquandogong/qmonster/releases">Releases</a>
  · <a href="https://github.com/chquandogong/qmonster/discussions">Discussions</a>
  · <a href="docs/ai/UI_MANUAL.md">UI Manual</a>
</p>

Qmonster watches a tmux workspace that runs Claude Code, Codex, Gemini,
and Qmonster side by side. It surfaces pane state, token pressure,
provider facts, reset timing, safety alerts, and recommendations without
taking destructive action by default.

| Surface             | Current                                                |
| ------------------- | ------------------------------------------------------ |
| Release             | `v3.2.0`                                               |
| npm                 | `qmonster@3.2.0`                                       |
| Rust                | `1.88+`                                                |
| Runtime version     | `git describe --tags --always --dirty` from `build.rs` |
| Cargo crate version | `3.2.0`                                                |

## Why

Multi-agent tmux work is powerful, but easy to lose track of. Qmonster
answers the operational questions that usually require constant manual
checking:

- Which pane is waiting, blocked, stale, or still working?
- Which session is approaching context or quota pressure?
- Which provider facts are official, estimated, or heuristic?
- Which reset window is close enough to wait or snapshot first?
- Which recommendation is worth acting on now?

The design contract is intentionally conservative:

1. Observe first.
2. Alert first.
3. Recommend first.
4. No destructive automation by default.

See [PROJECT_BRIEF.md](docs/ai/PROJECT_BRIEF.md) for the full project
intent.

## Quick Start

**Install** — pick one. Either path needs a working Rust toolchain
(`rustc 1.88+`) because the npm package compiles the binary on first run.

```bash
# 1) From npmjs (recommended for operators)
npm install -g qmonster
qmonster --help

# 2) From source (recommended for contributors)
git clone https://github.com/chquandogong/qmonster
cd qmonster
cargo build --release
```

> **Notice — the npm package is a source distribution, not a prebuilt binary.**
> `npm install qmonster` downloads a tarball that wraps `Cargo.toml` +
> `Cargo.lock` + `src/`; first invocation runs `cargo build --release`
> locally and compiles ~270 transitive crates, each of which may execute
> a `build.rs` at compile time. Trust extends to your local Rust toolchain
> and the crates.io packages it fetches.
>
> If you would rather run the maintainer-built binary (with SLSA build
> provenance) instead of compiling locally:
>
> ```sh
> gh release download v3.2.0 --pattern '*-linux-x86_64.tar.gz' --repo chquandogong/qmonster
> gh attestation verify qmonster-v3.2.0-linux-x86_64.tar.gz --owner chquandogong
> tar -xzf qmonster-v3.2.0-linux-x86_64.tar.gz
> ./qmonster-v3.2.0-linux-x86_64/qmonster --help
> ```

**Set the stage** — Qmonster watches the terminal workspace your AI
CLIs already run in. Two backends are supported (v3.2.0):

_herdr_ — one command builds the whole layout AND the global monitor:

```bash
# Ensures a "0-Monitor" workspace running Qmonster (created once,
# never touched again) + a project workspace with 1-Claude / 2-Codex /
# 3-Agy tabs, each split top/bottom, agents launched via your shell
# aliases. Idempotent; --no-agents / --dry-run supported.
./scripts/hs.sh ~/my-project
```

Inside a herdr pane Qmonster auto-selects the herdr backend
(`[mux] backend = "auto"`) and observes agent panes across **all**
workspaces — usage, quota windows, and reset ETAs per agent in one
dashboard.

_tmux_ — unchanged:

```bash
tmux new -s ai
# inside tmux: split into panes for claude / codex / gemini / qmonster
# (or copy the four-pane layout from Provider Setup → Tmux tab → installer)
```

**Run it** — from another shell or pane (hs.sh already does this for
the herdr monitor workspace):

```bash
# Creates ~/.qmonster/config/qmonster.toml + pricing.toml from templates
# when missing, then launches the TUI bound to that config path.
./scripts/run-qmonster.sh
```

**First launch** — Qmonster opens to the split dashboard:

- **Top: Alerts** — notices, recommendations, cross-pane findings.
  Press `/` (v1.59.0) to filter by case-insensitive substring.
- **Bottom: Panes** — one card per attached AI CLI pane. It opens with
  **nothing selected**, so every pane is compact: title + status pill +
  `ctx` / `5h` / `7d` gauge bars. Select a pane (`↓` / `↑` / click) to
  expand it to full detail — state, path/cmd, quota, tokens, cache, cost,
  reset ETA, runtime facts, and recommendations. Step off either end of
  the list to collapse everything back to the compact view.
- **Footer** — current target, key cluster, version badge. Click the
  badge to inspect Git status.

The most useful first keys:

| Key       | Action                                                                                                |
| --------- | ----------------------------------------------------------------------------------------------------- |
| `↑` / `↓` | Select a pane (expands it to full detail); step off either end to collapse everything back to compact |
| `?`       | Help overlay with the full key map and badge legend                                                   |
| `t`       | Pick which tmux session/window to observe (herdr backend: global view across all workspaces)          |
| `S`       | Settings overlay (`/` filters parameters by label)                                                    |
| `P`       | Provider Setup overlay (sidefile + tmux installers)                                                   |

**Smoke checks** if anything looks off:

```bash
cargo run -- --once                                              # one-pass scan, no UI
./scripts/check-tmux-source-parity.sh --all-targets --repeat 3   # polling vs control-mode parity
./scripts/run-qmonster-control-mode-once.sh --root /tmp/qmonster-smoke
```

GitHub Packages mirrors the npm source package:

```bash
npm config set @chquandogong:registry https://npm.pkg.github.com
npm install -g @chquandogong/qmonster
```

## What It Shows

The dashboard launches with **nothing selected** — every pane is compact
(status pill + `ctx` / `5h` / `7d` gauge bars). Select a pane to expand it
to full detail; step off either end of the list to collapse everything back:

```text
 Alerts · 3 ──────────────────────────────────────────────────────────
   ▸ Now   codex:1:review — waiting for approval
   ⚠ CONFLICT  src/ui/panels/mod.rs · claude:1:main ↔ codex:1:review
   ◇ Claude 5h quota 88% — resets in 47m                     [Official]

 Panes · target qwork:0 · ↓ selects & expands · nothing selected ──────
   ● ACTIVE            claude:1:main · CLI 2.1.4 [Official] · %25
   CTX  █████████▉░░░░░░  64%  of 1.00M
   5H   ██████████████▏░  88%  resets 47m
   7D   ███▎░░░░░░░░░░░░  31%  resets 4d 6h

   ⌛ WAIT APPROVAL     codex:1:review · CLI 0.142 [Official] · %27
   CTX  █████▏░░░░░░░░░░  36%  of 258K
   5H   █████████▏░░░░░░  61%  resets 1h05m
   7D   ██████▌░░░░░░░░░  44%  resets 5d

   ◌ IDLE STALE ⏱35s   agy:1:research · CLI 1.0.14 [Official] · %28
   CTX  █████▉░░░░░░░░░░  41%

   ● ACTIVE            qmonster:0:monitor · %24
 ──────────────────────────────────────────────────────────────────────
  ↑/↓ select · Enter/Space action · Tab focus · S settings · ? help · q quit
```

| Area            | Operator-visible result                                                                       |
| --------------- | --------------------------------------------------------------------------------------------- |
| Pane state      | Active, work complete, stale, input wait, permission wait, limit hit — shown as a status pill |
| Metrics         | CTX / quota / 5h / weekly as gauge bars, plus tokens, cache, memory, cost, reset ETA          |
| Runtime facts   | Session IDs, transcript paths, tool calls, model reset rows                                   |
| Recommendations | Alert/advisory queue with source-labeled reasons and commands                                 |
| Settings        | `S` overlay — Thresholds / Integrations / Parameters (3 tabs)                                 |
| Git status      | Click the footer version badge to inspect local repo state                                    |

Sanitized provider tails for demos and screenshots live in
[examples/demo](examples/demo/).

Primary keys:

| Key         | Action                                                     |
| ----------- | ---------------------------------------------------------- |
| `q` / `Esc` | Quit or close overlay                                      |
| `Tab`       | Switch alert/pane focus                                    |
| `t`         | Choose tmux session/window target                          |
| `s`         | Write runtime snapshot                                     |
| `u`         | Refresh provider runtime surfaces                          |
| `P`         | Provider Setup overlay                                     |
| `S`         | Settings overlay                                           |
| `?`         | Help / legend                                              |
| Mouse       | Scroll, select, double-click alert hide, footer Git status |

For a matching four-pane tmux layout, open Provider Setup with `P`,
select the `Tmux` tab, and copy the installer. It writes:

- `~/ts.sh`
- `~/.tmux/qmonster.tmux.conf`

## Data Contracts

Qmonster labels every provider-derived value by authority:

| Label        | Meaning                                                           |
| ------------ | ----------------------------------------------------------------- |
| `[Official]` | Emitted by the provider or a provider-owned config/status surface |
| `[Project]`  | Qmonster policy or project-canonical rule                         |
| `[Heur]`     | Local heuristic, such as process RSS or memory-file scan          |
| `[Estimate]` | Derived from local pricing or non-provider calculation            |

Metric contract:

| Metric   | Claude                             | Codex                      | Gemini               |
| -------- | ---------------------------------- | -------------------------- | -------------------- |
| `CTX`    | statusline context used            | bottom status context      | status table context |
| `QUOTA`  | statusline 5h / weekly             | bottom status or rollout   | status table quota   |
| `RESET`  | sidefile timestamps                | rollout `rate_limits`      | —                    |
| `TOKENS` | statusline + sidefile              | bottom status / usage line | —                    |
| `CACHE`  | statusline ratio or sidefile reads | cached input tokens        | —                    |
| `COST`   | sidefile total cost                | pricing estimate           | unset today          |

Codex reset ETA comes from the codex-tui rollout `rate_limits` (v3.1.0);
policy-grade reset advisories require machine-readable timestamps, currently
available from Claude sidefiles and the Codex rollout. Gemini is
status-table-core (context / quota / memory / model); its former `/stats` and
`/model` enrichment was removed in the narrow reduction (v3.0.0).
**Antigravity (`agy`)** is ObserveOnly identification by default — its ctx /
quota / reset appear only when the operator wires the opt-in sidefile export
(Provider Setup → `agy` tab), and never promote to alerts or actuation.

## Releases

The operator-facing version is the Git tag, npm version, and Cargo crate
version kept in lockstep. Release automation publishes:

- GitHub Release with Linux x86_64 binary tarball, npm tarball, and checksums
- npmjs package: `qmonster`
- GitHub Packages mirror: `@chquandogong/qmonster`

Release flow lives in [docs/RELEASING.md](docs/RELEASING.md). Long-form
history lives in `mission-history.yaml`; the README only tracks the
current operator surface.

**Verifying a download** — the released npm tarball and Linux binary
both carry SLSA build provenance and SBOM attestations. After
`npm install -g qmonster`, run:

```sh
npm audit signatures                                           # npm registry signature + provenance chain
gh attestation verify qmonster-X.Y.Z.tgz --owner chquandogong  # GitHub attestation against this repo
```

These prove the _tarball_ came from this repository's release
workflow. The `qmonster` binary itself is compiled by `cargo` on your
machine on first run, so end-to-end trust also depends on your Rust
toolchain and the crates.io packages it fetches. Full command set
(Linux binary tarball, SBOM, scope notes) lives in
[docs/RELEASE_VERIFICATION.md](docs/RELEASE_VERIFICATION.md).

## Architecture

<p align="center">
  <img src="docs/assets/qmonster-architecture.svg" alt="Qmonster observe-first architecture diagram" width="100%">
</p>

```text
tmux::RawPaneSnapshot
   |
domain::IdentityResolver
   |
adapters::ProviderParser
   |
domain::SignalSet
   |
policy::Engine
   |
app::EffectRunner
   |\
ui::ViewModel   store::EventSink
```

Boundaries:

- Identity resolution happens before provider parsing.
- `policy/` performs no IO.
- Runtime writes stay under `~/.qmonster/` by default.
- Raw tmux tails are archived as files, not stored in SQLite.
- Provider values must keep honest source labels.

See [ARCHITECTURE.md](docs/ai/ARCHITECTURE.md) for module-level detail.

## Repository Layout

```text
src/
  app/        bootstrap, config, event loop, effects, overlays
  adapters/   claude / codex / gemini / qmonster parsers
  domain/     identity, origin, signal, recommendation, audit types
  policy/     pure recommendation engine and rules
  store/      sqlite, audit, archive, snapshots, token samples
  tmux/       polling and control-mode pane sources
  ui/         ratatui dashboard, alerts, panels, settings

config/       operator config templates
docs/ai/      canonical project docs
docs/assets/  README and social-preview artwork
.github/      CI, release, issue, PR, and discussion templates
npm/          npm source-package wrapper
scripts/      local run and validation helpers
tests/        integration tests
```

## Development

```bash
cargo fmt --all --check
git diff --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
npm pack --dry-run
```

For tmux transport work:

```bash
./scripts/check-tmux-source-parity.sh --all-targets --repeat 3
```

The integration tests use fixture pane sources and do not require a live
tmux session.

## Documentation

- [PROJECT_BRIEF.md](docs/ai/PROJECT_BRIEF.md) — scope and operating principles
- [ARCHITECTURE.md](docs/ai/ARCHITECTURE.md) — module layout and data flow
- [UI_MANUAL.md](docs/ai/UI_MANUAL.md) — TUI keys, badges, and metric meanings
- [VALIDATION.md](docs/ai/VALIDATION.md) — validation gates
- [WORKFLOWS.md](docs/ai/WORKFLOWS.md) — planning and handoff workflow
- [REVIEW_GUIDE.md](docs/ai/REVIEW_GUIDE.md) — reviewer contract
- [docs/RELEASING.md](docs/RELEASING.md) — release and package mirror flow
- [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) — tested runtime and provider surfaces
- [docs/RELEASE_VERIFICATION.md](docs/RELEASE_VERIFICATION.md) — checksums, build provenance, and SBOM verification
- [docs/GITHUB_POLISH.md](docs/GITHUB_POLISH.md) — repo polish checklist and next steps
- [SECURITY.md](SECURITY.md) / [SUPPORT.md](SUPPORT.md) / [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) — reporting and community routing
- [VERSION.md](VERSION.md) — version surface map

## Community

- Contributors: [CONTRIBUTORS.md](CONTRIBUTORS.md)
- Discussions: setup help, tmux layouts, provider behavior, and workflow ideas
- Issues: reproducible bugs and scoped feature requests
- Security: private vulnerability reports through GitHub Security Advisories
- Social preview asset: `docs/assets/qmonster-social-preview.png`

## Scope

Qmonster is not a provider orchestrator, not a destructive automator, and
not a cloud service. It is a single-user local operating console. Default
action mode is `recommend_only`; refresh policy is `manual_only`; logging
sensitivity is `balanced`.

## License

MIT. See [LICENSE](LICENSE).
