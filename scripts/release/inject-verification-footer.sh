#!/usr/bin/env bash
# Append (or replace) the qmonster-release-verification footer in a
# GitHub release body. Idempotent: re-running on the same tag replaces
# the existing footer instead of stacking duplicates.
#
# Usage:
#   inject-verification-footer.sh <tag>
#
# Requires: gh CLI authenticated (env GH_TOKEN or gh auth status).

set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: inject-verification-footer.sh <tag>" >&2
  exit 2
fi

TAG_NAME="$1"

footer_file="$(mktemp)"
trap 'rm -f "$footer_file" "${body_file:-}" "${base_file:-}" "${notes_file:-}"' EXIT

cat > "$footer_file" <<EOF
<!-- qmonster-release-verification -->
## Verification

Download the release assets, then verify checksums:

\`\`\`sh
sha256sum -c checksums.txt
\`\`\`

Verify build provenance for the Linux binary tarball:

\`\`\`sh
gh attestation verify qmonster-${TAG_NAME}-linux-x86_64.tar.gz \\
  --repo chquandogong/qmonster
\`\`\`

Verify the independent SBOM attestation for that tarball:

\`\`\`sh
gh attestation verify qmonster-${TAG_NAME}-linux-x86_64.tar.gz \\
  --repo chquandogong/qmonster \\
  --predicate-type https://spdx.dev/Document/v2.3
\`\`\`

The release also includes \`qmonster-${TAG_NAME}-sbom.spdx.json\`
and \`sbom-diff-summary.txt\`. See \`docs/RELEASE_VERIFICATION.md\`
for the full verification guide.
EOF

body_file="$(mktemp)"
gh release view "$TAG_NAME" --json body --jq .body > "$body_file"

# Strip any prior verification footer (everything from the marker
# onwards) so re-running this script replaces rather than stacks.
base_file="$(mktemp)"
awk '/<!-- qmonster-release-verification -->/{exit} {print}' "$body_file" > "$base_file"

notes_file="$(mktemp)"
{
  cat "$base_file"
  printf '\n\n'
  cat "$footer_file"
} > "$notes_file"

gh release edit "$TAG_NAME" --notes-file "$notes_file"
