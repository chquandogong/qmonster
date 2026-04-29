# GitHub Polish Checklist

This document tracks the repository-facing work that makes Qmonster look
maintained, trustworthy, and easy to evaluate from the GitHub page.

## Already In Place

| Area | Status |
| --- | --- |
| README first impression | Banner, badges, short product summary, quick start, data contract |
| Release automation | GitHub Release assets, npm publish, GitHub Packages mirror |
| CI | fmt, whitespace, tests, clippy, npm package dry-run |
| Community routing | Issue forms, PR template, Discussion templates, support doc |
| Security routing | `SECURITY.md` and private advisory link |
| Dependency automation | Dependabot for Cargo, npm, and GitHub Actions |
| Social artwork | `docs/assets/qmonster-social-preview.png` |
| Package metadata | npm keywords, repository, homepage, license, files allow-list |
| Ownership routing | `.github/CODEOWNERS` |
| Release notes | `.github/release.yml` generated-notes categories |
| Release provenance | GitHub artifact attestations for release assets |
| Compatibility docs | `docs/COMPATIBILITY.md` |
| Demo fixtures | `examples/demo/` sanitized provider tails |
| Architecture visual | `docs/assets/qmonster-architecture.svg` |
| Community conduct | `CODE_OF_CONDUCT.md` |
| Branch protection | `main` requires `Rust, docs, and package checks`; admin bypass remains available |

## Recommended Next Steps

1. Protect release tags.
   Restrict `v*` tag creation/deletion to maintainers so publish events
   cannot be triggered accidentally. An evaluate-mode starter payload
   lives in `.github/rulesets/release-tags.example.json`; add the right
   maintainer bypass path before switching it to active.

2. Add a repository social preview image.
   Upload `docs/assets/qmonster-social-preview.png` in GitHub
   `Settings -> Social preview`.

3. Add a real dashboard screenshot or short terminal recording.
   `examples/demo/` now provides sanitized tails, but the best README
   asset would still be a real captured dashboard image under
   `docs/assets/`.

4. Expand release provenance over time.
   Release assets now get GitHub artifact attestations; future hardening
   could add SBOM generation and an explicit verification section in each
   release body.

5. Keep compatibility evidence fresh.
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
