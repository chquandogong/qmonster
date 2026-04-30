# Releasing Qmonster

Qmonster keeps the public operator version in the mission ledger and npm
package metadata. `Cargo.toml` stays at its internal crate version.

## Release Surfaces

- Git tag: `vX.Y.Z`
- npmjs package: `qmonster@X.Y.Z`
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
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args
```

It then builds the release binary, creates or updates the GitHub Release,
publishes `qmonster` to npmjs when `NPM_TOKEN` is configured, and publishes
the scoped GitHub Packages mirror using `GITHUB_TOKEN`. Release assets are
also signed with GitHub artifact attestations, and the Linux tarball gets a
dedicated SBOM attestation.

Generated GitHub Release notes use `.github/release.yml`. Keep README
focused on the current product surface; put patch-level implementation
detail in GitHub Releases, `mission-history.yaml`, and canonical docs.

## Manual Checklist

1. Update `README.md`, `VERSION.md`, `package.json`, `mission.yaml`, and
   `mission-history.yaml`.
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

## SNS Preview

GitHub social preview upload is a repository settings action. Use
`docs/assets/qmonster-social-preview.png` for the upload. The SVG source
is kept beside it as editable artwork.

`Settings -> Social preview -> Edit -> Upload an image`
