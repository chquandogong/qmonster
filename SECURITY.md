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
- **npm publish auth**. npmjs.com publish is workflow-bound via Trusted
  Publishers OIDC — the `NPM_TOKEN` secret was retired after v2.3.5 and is
  no longer present anywhere (every v2.4.0+ release publishes OIDC-only).
  The bus-factor surface is GitHub admin access only (plus the workflow
  identity the Trusted Publisher entry trusts).
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

## AI Tooling Expectations

Qmonster is developed with AI-assisted local tooling (Claude Code,
Codex, Gemini CLI). All AI-generated output today flows through the
maintainer's local workstation, is reviewed before commit, and
appears under the maintainer's GitHub identity. There is no
GitHub OAuth-delegated AI agent on this repository.

Guardrails for future automation, codified now so they cannot drift
in if a GitHub App or bot is introduced later:

- **Never grant `workflow:write` to an AI agent.** Workflow file edits
  must remain a human-reviewed action. An agent token with
  `workflow:write` would let a single prompt-injection rewrite
  `release.yml` and bypass every supply-chain gate documented above.
- **Bot identity must be separate from maintainer identity.** Any
  future automation runs under a dedicated GitHub user (e.g.
  `qmonster-bot`) with its own SSH/PAT, scoped to a `bot/*` branch
  prefix. The maintainer's identity stays attached to human-reviewed
  commits.
- **Bot commits land via PR, not direct push.** Branch protection on
  `main` already requires PR for non-admin actors; the bot account
  inherits that gate even if a future co-maintainer admin is added.
- **No AI agent may bypass the tag protection ruleset.** Ruleset
  `bypass_actors` is intentionally pinned to `RepositoryRole: 5`
  (admin) only. Bot accounts are not admins.
- **External LLM calls must not echo raw tmux content unmasked.**
  Qmonster's runtime principle ("raw tmux tails must not be written
  to SQLite") extends to outbound prompts: any future automation that
  forwards pane content to a hosted model must strip provider
  secrets, OAuth tokens, and prompts before sending. The local AI
  tools used today are the maintainer's responsibility to vet.
- **PAT scope separation.** The maintainer's daily-use PAT should
  hold the minimum scopes needed for normal push (`repo`, `gist`).
  The `workflow` scope — needed only when editing
  `.github/workflows/` — should live in a separate fine-grained
  token, not co-resident with the AI tooling's day-to-day
  credentials.
