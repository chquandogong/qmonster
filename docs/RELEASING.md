# Releasing Qmonster

Qmonster keeps the public operator version aligned across the mission
ledger, npm package metadata, and Cargo package metadata.

## Release Surfaces

- Git tag: `vX.Y.Z`
- npmjs package: `qmonster@X.Y.Z`
- Cargo package: `qmonster X.Y.Z`
- GitHub Packages mirror: `@chquandogong/qmonster@X.Y.Z`
- GitHub Release assets: Linux x86_64 binary tarball, npm package
  tarball, SPDX-JSON SBOM, SBOM diff summary, and checksums.

## Automated Flow

The `Release and Package Mirror` workflow runs when a `v*` tag is pushed
or manually through `workflow_dispatch`.

It validates:

```bash
cargo fmt --all --check
git diff --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

It then builds the release binary, creates or updates the GitHub Release,
publishes `qmonster` to npmjs via Trusted Publishers OIDC (npmjs.com
side is registered for chquandogong/qmonster against this workflow),
and publishes the scoped GitHub Packages mirror using `GITHUB_TOKEN`.
Release assets are also signed with GitHub artifact attestations, and
the Linux tarball gets a dedicated SBOM attestation.

Generated GitHub Release notes use `.github/release.yml`. Keep README
focused on the current product surface; put patch-level implementation
detail in GitHub Releases, `mission-history.yaml`, and canonical docs.

## Manual Checklist

1. Update `README.md`, `VERSION.md`, `package.json`, `Cargo.toml`,
   `Cargo.lock`, `mission.yaml`, and `mission-history.yaml`.
2. Run the validation gates from `docs/ai/VALIDATION.md`.
3. Run the local release dry-run before tagging:

   ```bash
   scripts/release/dry-run.sh vX.Y.Z
   ```

   This mirrors the CI release-asset job (build → assemble → SBOM scan
   → package-count guard → SBOM diff vs previous tag → checksum
   manifest) and writes everything under `dist-dryrun/`. Catches the
   class of regressions that hit v1.36.0 (missing dist/), v1.36.2 (SBOM
   1-package collapse), and v1.36.5 (checksums.txt absolute paths).

4. Commit and push `main`.
5. Create and push the annotated tag:

   ```bash
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin vX.Y.Z
   ```

6. Confirm the release workflow published the GitHub Release and packages.

7. Verify release provenance for downloaded assets when needed:

   ```bash
   gh version
   gh attestation verify qmonster-vX.Y.Z-linux-x86_64.tar.gz \
     --repo chquandogong/qmonster
   gh attestation verify qmonster-vX.Y.Z-linux-x86_64.tar.gz \
     --repo chquandogong/qmonster \
     --predicate-type https://spdx.dev/Document/v2.3
   gh attestation verify qmonster-X.Y.Z.tgz \
     --repo chquandogong/qmonster
   ```

   Use GitHub CLI 2.49.0 or newer. If `gh attestation verify` is not
   recognized, upgrade `gh` before running release-attestation checks.

## Supply-Chain Controls

These controls back the `Release and Package Mirror` workflow above.
Keep them aligned whenever the workflow or release surface changes.

### Action pin discipline

All `uses:` entries in `.github/workflows/` are pinned to the full
commit SHA with a trailing `# vX.Y.Z` comment so Dependabot's
`github-actions` ecosystem can bump both at once. Floating major tags
like `@v6` would let an upstream action vendor replay attestations
without notice. Sweep with:

```bash
grep -nE 'uses: [^#]+@v[0-9]+' .github/workflows/*.yml
```

Non-empty output means a floating pin has slipped in. Resolve the
exact commit SHA for the desired tag and replace:

```bash
gh api /repos/<owner>/<action>/commits/<tag> --jq .sha
```

### Tag protection ruleset

`v*` tag creation/update/deletion is bound by an active ruleset that
only the repository admin role bypasses. The source of truth is
`.github/rulesets/release-tags.json`. To register or refresh after
edits:

```bash
gh api -X POST /repos/chquandogong/qmonster/rulesets \
  --input .github/rulesets/release-tags.json
gh api /repos/chquandogong/qmonster/rulesets \
  --jq '.[] | {id, name, enforcement, target}'
```

### npmjs publish auth — Trusted Publishers

`release.yml`'s `publish` job carries `id-token: write`, and `npm
publish` runs exclusively through Trusted Publishers OIDC. The
matching entry on npmjs.com:

- Provider: GitHub Actions
- Organization or user: `chquandogong`
- Repository: `qmonster`
- Workflow filename: `release.yml`
- Environment name: (blank — the workflow does not use a GitHub Environment)

`NPM_TOKEN` was retired in v2.3.2 after the Trusted Publisher entry
was verified end-to-end. There is no fallback path: if the TP entry
on the npmjs.com side is removed or its fields drift from the
workflow signature, the publish step fails hard instead of silently
re-authenticating. Recovery is to fix the TP entry (or temporarily
re-introduce a `NODE_AUTH_TOKEN` env block + secret pair) and
re-tag.

### Workflow permissions surface

The release workflow declares `permissions: {}` at the workflow level
so any new job must declare its own narrower set. Current allocation:

| Job       | Permissions                                                                                 |
| --------- | ------------------------------------------------------------------------------------------- |
| `release` | `contents: write` (gh release), `id-token: write`, `attestations: write` (actions/attest\*) |
| `publish` | `contents: read` (checkout), `id-token: write` (npm OIDC), `packages: write` (GHP mirror)   |

Never widen back to workflow-level write — it expands the blast
radius of any compromised step.

### npm package surface

CI fails (`Verify npm package has no lifecycle scripts or runtime
deps` in `ci.yml`) if `package.json` grows any of
`preinstall`/`install`/`postinstall`/`prepare`/`prepublishOnly`/`prepack`
or a non-empty `dependencies` / `devDependencies` list. The npm
wrapper must stay a thin cargo-run shim with zero registry-time code
execution. If a real change is needed, justify it in the PR and
update this guard with the same PR.

## SNS Preview

GitHub social preview upload is a repository settings action. Use
`docs/assets/qmonster-social-preview.png` for the upload. The SVG source
is kept beside it as editable artwork.

`Settings -> Social preview -> Edit -> Upload an image`
