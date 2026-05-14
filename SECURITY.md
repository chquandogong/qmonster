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

## Advisory Response SLO

`cargo audit` runs in `.github/workflows/ci.yml` and
`.github/workflows/release.yml` against the live RustSec advisory
database, and GitHub Dependabot security updates is enabled at the
repo level (alert dashboard:
https://github.com/chquandogong/qmonster/security/dependabot).
Response targets:

| Severity     | Acknowledge   | Mitigate or accept     |
| ------------ | ------------- | ---------------------- |
| Critical     | within 24h    | within 72h             |
| High         | within 48h    | within 7 days          |
| Medium / Low | within 7 days | next scheduled release |

"Acknowledge" means one of:

- a fix PR opened against `main` (or a direct commit in the
  single-maintainer mode), or
- a Mission Decision Record under `.mission/decisions/`
  documenting an accept-with-revisit decision (see the
  v2.3.x lru advisory MDR for the template:
  `.mission/decisions/MDR-DRAFT-v2.3.x-supplychain-lru-stacked-borrows-acceptance.md`).

`cargo audit`'s exit code remains the build gate: non-zero is reserved
for advisories classified as `vulnerability`, while `unsound` and
`unmaintained` advisories pass as warnings. Severity-based gating is a
deliberate trade-off — informational advisories that turn out to need
acceptance via MDR (like RUSTSEC-2026-0002) should not require a CI
config change to ship the next release.

## Maintainer Continuity

Qmonster is operated by a single maintainer at present
(`@chquandogong`). Bus-factor mitigation:

- **Signing keys**. Any GPG signing key used for tag signatures should
  have its private key encrypted (`gpg --export-secret-keys` piped
  through `gpg --symmetric`) and stored offline — for example on an
  encrypted USB stick or as a paper backup — kept separately from the
  maintainer's daily workstation.
- **Release tag protection bypass**. The active ruleset (id 16375762)
  allows `RepositoryRole: 5` (admin) bypass on `refs/tags/v*`. An
  emergency-recovery path exists once a co-maintainer with admin
  access is added; until then the recovery surface is one person.
- **npm publish auth**. While the v2.3.x release line is on the
  `NPM_TOKEN` fallback, the secret is the single point of failure for
  npmjs.com publish. Once the npmjs.com Trusted Publisher entry is
  configured, publish auth becomes workflow-bound via OIDC and the
  token can be deleted; the bus-factor surface shrinks to GitHub admin
  access only.
- **Repo admin**. `chquandogong` is currently the sole repo admin.
  Adding a second admin would reduce bus-factor to two, at the cost of
  expanding the bypass surface for the tag protection ruleset
  (`RepositoryRole` admins bypass it). This is a deliberate
  trade-off pending a real co-maintainer relationship.
- **Mission state**. `.mission/CURRENT_STATE.md` plus `mission.yaml`
  plus `mission-history.yaml` form the day-end ledger; a new
  maintainer taking over would read these (in that order) to
  reconstruct the current goal, surface, and rationale without
  depending on the outgoing maintainer's auto memory.
