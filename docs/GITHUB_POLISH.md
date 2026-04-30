# GitHub Polish Checklist

This document tracks the repository-facing work that makes Qmonster look
maintained, trustworthy, and easy to evaluate from the GitHub page.

## Already In Place

| Area                    | Status                                                                           |
| ----------------------- | -------------------------------------------------------------------------------- |
| README first impression | Banner, badges, short product summary, quick start, data contract                |
| Release automation      | GitHub Release assets, npm publish, GitHub Packages mirror                       |
| CI                      | fmt, whitespace, tests, clippy, npm package dry-run                              |
| Community routing       | Issue forms, PR template, Discussion templates, support doc                      |
| Security routing        | `SECURITY.md` and private advisory link                                          |
| Dependency automation   | Dependabot for Cargo, npm, and GitHub Actions                                    |
| Social preview          | Uploaded in GitHub Settings; source asset at `docs/assets/qmonster-social-preview.png` |
| Package metadata        | npm keywords, repository, homepage, license, files allow-list                    |
| Ownership routing       | `.github/CODEOWNERS`                                                             |
| Release notes           | `.github/release.yml` generated-notes categories                                 |
| Release provenance      | GitHub artifact attestations, SBOM attestation, SPDX-JSON SBOM, SBOM diff + risk summary |
| Compatibility docs      | `docs/COMPATIBILITY.md`                                                          |
| Demo fixtures           | `examples/demo/` sanitized provider tails                                        |
| Architecture visual     | `docs/assets/qmonster-architecture.svg`                                          |
| Dashboard screenshot    | `docs/assets/qmonster-dashboard.png` (embedded in README hero)                   |
| Community conduct       | `CODE_OF_CONDUCT.md`                                                             |
| Branch protection       | `main` requires `Rust, docs, and package checks`; admin bypass remains available |

## Recommended Next Steps

1. Protect release tags.
   Restrict `v*` tag creation/deletion to maintainers so publish events
   cannot be triggered accidentally. An evaluate-mode starter payload
   lives in `.github/rulesets/release-tags.example.json`; add the right
   maintainer bypass path before switching it to active.

2. Refresh the dashboard screenshot when the TUI changes substantively.
   The current asset lives at `docs/assets/qmonster-dashboard.png` and
   is embedded in the README "What It Shows" section. To recapture:
   - PNG: take a screenshot of the running TUI (system screenshot tool)
     and overwrite `docs/assets/qmonster-dashboard.png`.
   - SVG / animated alternative: `asciinema rec` for a short cast, then
     `svg-term --in cast.json --out docs/assets/qmonster-dashboard.svg`
     and update the README `<img src=...>` accordingly.
   - Sanitization: prefer the `examples/demo/` fixture set or a scratch
     tmux session — production paths and session IDs should not appear
     in a public asset.

3. Expand release provenance over time.
   Release assets now get GitHub artifact attestations, an independent
   SBOM attestation for the Linux tarball, a SPDX-JSON SBOM
   (`anchore/sbom-action@v0`), an SBOM diff summary with purl coverage
   and metadata/risk review signals, and a release-body verification
   footer. Further hardening could add vulnerability-policy checks backed
   by an advisory database.

4. Keep compatibility evidence fresh.
   Update `docs/COMPATIBILITY.md` after provider CLI rendering changes or
   major tmux/Rust/GitHub runner changes.

## Manual GitHub Settings

These are repository settings, not normal Git-tracked files:

- Branch protection: `Settings -> Branches -> Add branch protection rule`
- Tag protection/rulesets: `Settings -> Rules -> Rulesets`
- Social preview: `Settings -> Social preview`
- Discussions categories: `Settings -> Features -> Discussions`
- Package visibility: package page settings after first GitHub Packages publish

## Maintenance Principle

The GitHub front page should answer three questions in under a minute:

- What does Qmonster do?
- How do I run it?
- Can I trust the numbers and automation boundaries?
