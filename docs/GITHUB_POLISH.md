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

## Recommended Next Steps

1. Enable branch protection for `main`.
   Require the `Rust, docs, and package checks` status check before merge.

2. Protect release tags.
   Restrict `v*` tag creation/deletion to maintainers so publish events
   cannot be triggered accidentally.

3. Add a repository social preview image.
   Upload `docs/assets/qmonster-social-preview.png` in GitHub
   `Settings -> Social preview`.

4. Add screenshots or a short terminal recording.
   A TUI project benefits from a real dashboard image. Prefer a sanitized
   screenshot under `docs/assets/` and link it from README.

5. Add a `CODEOWNERS` file.
   Route changes in `src/adapters/`, `.github/`, and `docs/ai/` to the
   right reviewer once more collaborators are active.

6. Add release notes discipline.
   Keep README current-state only; put patch detail in GitHub Releases,
   `mission-history.yaml`, and canonical docs.

7. Add a minimal architecture diagram image.
   The README text pipeline is enough for developers, but a small visual
   diagram improves quick evaluation.

8. Add signed release provenance when the package flow supports it end to
   end.
   npm provenance is already enabled in the workflow; binary attestation
   can be added later with GitHub artifact attestations.

9. Add a compatibility matrix.
   Track tested versions of tmux, Rust, Claude Code, Codex, and Gemini CLI.

10. Add curated demo fixtures.
    Provide sanitized sample tmux captures under `examples/` so reviewers
    can understand the UI without connecting real provider panes.

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
