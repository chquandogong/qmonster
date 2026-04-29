# Security Policy

Qmonster is an observe-first local TUI. It reads tmux pane output, local
provider config, and optional provider sidefiles; it should not submit
prompts or mutate observed panes unless an explicit operator gate allows
that path.

## Supported Versions

Security fixes target `main` and the latest tagged release line.

## Reporting a Vulnerability

Please use GitHub private vulnerability reporting:

https://github.com/chquandogong/qmonster/security/advisories/new

Do not open a public issue with raw transcripts, tokens, API keys,
provider account details, or private project paths.

## Sensitive Data Expectations

- Raw tmux tails must not be written to SQLite.
- Runtime writes must stay under the resolved Qmonster root,
  `~/.qmonster/` by default.
- Provider-derived facts must preserve honest source labels.
- New automation must keep observe-first behavior as the default.
