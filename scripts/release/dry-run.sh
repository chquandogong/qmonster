#!/usr/bin/env bash
# Local mirror of .github/workflows/release.yml's release-asset job.
# Builds the binary, assembles the dist directory, generates the SBOM,
# runs the same package-count and diff-vs-previous-release guards as
# CI, and computes checksums. Skips the GitHub-side attestation and
# release-creation steps (those need OIDC + gh CLI release-write).
#
# Run this BEFORE pushing a release tag to catch the class of
# regressions that hit v1.36.0 (missing dist/), v1.36.2 (SBOM
# 1-package collapse), and v1.36.5 (checksums.txt absolute paths).
#
# Usage:
#   scripts/release/dry-run.sh                # uses git describe for tag
#   scripts/release/dry-run.sh v1.36.6        # explicit tag override
#   TAG_NAME=v1.36.6 scripts/release/dry-run.sh
#
# Side effects:
# - writes ./dist-dryrun/ (gitignored) — kept for inspection
# - downloads syft into ~/.cache/qmonster-release-tools/ if missing
# - downloads previous release's SBOM into a tempdir for diff

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

TAG_NAME="${TAG_NAME:-${1:-}}"
if [ -z "$TAG_NAME" ]; then
  TAG_NAME="$(git describe --tags --abbrev=0 2>/dev/null || true)"
  if [ -z "$TAG_NAME" ]; then
    echo "ERROR: no TAG_NAME supplied and git describe found no tag." >&2
    echo "Pass a tag explicitly: scripts/release/dry-run.sh v1.36.6" >&2
    exit 2
  fi
fi
echo "[dry-run] Using TAG_NAME=${TAG_NAME}"

# 1. Verify package.json version matches the tag (release.yml does this too).
tag_version="${TAG_NAME#v}"
package_version="$(node -p "require('./package.json').version")"
if [ "$package_version" != "$tag_version" ]; then
  echo "ERROR: package.json version $package_version does not match tag $TAG_NAME" >&2
  exit 1
fi

# 2. Run release validation (subset that doesn't require CI env).
echo "[dry-run] Validation: cargo fmt + git diff --check + cargo test + cargo clippy"
cargo fmt --all --check
git diff --check
cargo test --all-targets >/dev/null
cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args 2>&1 | tail -3

# 3. Build release binary.
echo "[dry-run] Build: cargo build --release --bin qmonster"
cargo build --release --bin qmonster

# 4. Assemble release artifacts mirroring release.yml.
DIST="${ROOT_DIR}/dist-dryrun"
rm -rf "$DIST" && mkdir -p "$DIST"
ARTIFACT_DIR="${DIST}/qmonster-${TAG_NAME}-linux-x86_64"
mkdir -p "$ARTIFACT_DIR"
cp target/release/qmonster "$ARTIFACT_DIR/"
cp README.md LICENSE VERSION.md "$ARTIFACT_DIR/"
cp config/qmonster.example.toml "$ARTIFACT_DIR/"
cp Cargo.lock "$ARTIFACT_DIR/"
tar -C "$DIST" -czf "${DIST}/qmonster-${TAG_NAME}-linux-x86_64.tar.gz" "qmonster-${TAG_NAME}-linux-x86_64"
npm pack --pack-destination "$DIST" >/dev/null

# 5. Get syft.
SYFT_DIR="${HOME}/.cache/qmonster-release-tools"
SYFT_VERSION="v1.42.3"
SYFT_BIN="${SYFT_DIR}/syft"
if [ ! -x "$SYFT_BIN" ]; then
  echo "[dry-run] Installing syft ${SYFT_VERSION} into ${SYFT_DIR}"
  mkdir -p "$SYFT_DIR"
  ARCHIVE="${SYFT_DIR}/syft.tar.gz"
  curl -sSfLo "$ARCHIVE" \
    "https://github.com/anchore/syft/releases/download/${SYFT_VERSION}/syft_${SYFT_VERSION#v}_linux_amd64.tar.gz"
  tar -C "$SYFT_DIR" -xzf "$ARCHIVE" syft
  rm -f "$ARCHIVE"
fi

# 6. Generate SBOM.
SBOM="${DIST}/qmonster-${TAG_NAME}-sbom.spdx.json"
echo "[dry-run] SBOM scan: ${ARTIFACT_DIR}"
"$SYFT_BIN" scan "dir:${ARTIFACT_DIR}" -o "spdx-json=${SBOM}" --quiet

# 7. SBOM package-count guard (mirror release.yml threshold).
# Filters SPDX root document entries so the count matches what
# scripts/release/sbom-diff.js reports — the per-tag root rename must
# not inflate the count and trip the guard later.
count_real_packages() {
  node -e '
    const fs = require("fs");
    const doc = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const isRoot = (p) =>
      p.SPDXID === "SPDXRef-DOCUMENT" ||
      (typeof p.name === "string" && /qmonster-v[0-9]+\.[0-9]+\.[0-9]+/.test(p.name));
    const pkgs = (doc.packages || []).filter((p) => !isRoot(p));
    console.log(pkgs.length);
  ' "$1"
}
pkg_count="$(count_real_packages "$SBOM")"
if [ "$pkg_count" -lt 50 ]; then
  echo "ERROR: SBOM has only $pkg_count packages — too few, suggests dependency cataloger failed." >&2
  exit 1
fi
echo "[dry-run] OK: SBOM contains $pkg_count packages."

# 8. SBOM diff vs previous release tag (best-effort).
PREVIOUS_TAG="$(git tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | grep -vx "$TAG_NAME" | head -n 1 || true)"
if [ -n "$PREVIOUS_TAG" ] && command -v gh >/dev/null 2>&1; then
  PREV_DIR="$(mktemp -d)"
  PREV_ASSET="qmonster-${PREVIOUS_TAG}-sbom.spdx.json"
  if gh release download "$PREVIOUS_TAG" --pattern "$PREV_ASSET" --dir "$PREV_DIR" 2>/dev/null; then
    echo "[dry-run] SBOM diff vs ${PREVIOUS_TAG}"
    node "${ROOT_DIR}/scripts/release/sbom-diff.js" \
      "${PREV_DIR}/${PREV_ASSET}" "$SBOM" "$PREVIOUS_TAG" "$TAG_NAME" \
      > "${DIST}/sbom-diff-summary.txt"
    head -8 "${DIST}/sbom-diff-summary.txt"

    PREV_COUNT="$(count_real_packages "${PREV_DIR}/${PREV_ASSET}")"
    MIN_ALLOWED=$((PREV_COUNT / 2))
    if [ "$MIN_ALLOWED" -lt 50 ]; then MIN_ALLOWED=50; fi
    if [ "$pkg_count" -lt "$MIN_ALLOWED" ]; then
      echo "ERROR: SBOM package count dropped from $PREV_COUNT to $pkg_count; minimum allowed is $MIN_ALLOWED." >&2
      exit 1
    fi
    echo "[dry-run] OK: SBOM package count $pkg_count compared with $PREV_COUNT in $PREVIOUS_TAG."
  else
    echo "[dry-run] Skipping diff: previous SBOM ${PREV_ASSET} not downloadable."
  fi
else
  echo "[dry-run] Skipping diff: previous tag or gh CLI unavailable."
fi

# 9. Compute checksums (mirror v1.36.5 basename behavior).
(
  cd "$DIST"
  find . -maxdepth 1 -type f ! -name checksums.txt -print0 \
    | sort -z \
    | xargs -0 sha256sum \
    | sed 's#  \./#  #' > checksums.txt
)
echo "[dry-run] Wrote ${DIST}/checksums.txt"
echo
echo "[dry-run] All gates passed. Inspect ${DIST}/ to verify before tagging."
