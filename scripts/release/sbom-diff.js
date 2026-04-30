#!/usr/bin/env node
// Diff two SPDX-JSON SBOMs and emit a human-readable summary plus
// metadata/risk review signals.
//
// Usage:
//   node scripts/release/sbom-diff.js <previous.json> <current.json> <previousTag> <currentTag>
//
// Behavior:
// - Filters out SPDX root document entries (SPDXRef-DOCUMENT) and
//   per-tag directory descriptors so the diff reflects only actual
//   dependency churn — not the SBOM root document name change that
//   happens every release tag (v1.36.4-v1.36.5 false-positive fix).
// - Surfaces metadata-quality metrics: package URL (purl) coverage,
//   versioned coverage, missing license/supplier assertions.
// - Highlights packages added/removed in a security/runtime attention
//   set (openssl, ring, rustls, hyper, etc.) for reviewer focus.
//
// Used by:
// - .github/workflows/release.yml "Compare SBOM with previous release"
// - scripts/release/dry-run.sh local pre-tag mirror

const fs = require("fs");

const [previousPath, currentPath, previousTag, currentTag] =
  process.argv.slice(2);
if (!previousPath || !currentPath || !previousTag || !currentTag) {
  console.error(
    "usage: sbom-diff.js <previous.json> <current.json> <previousTag> <currentTag>",
  );
  process.exit(2);
}

const isRootDocument = (pkg) => {
  // SPDX root descriptor: SPDXID is "SPDXRef-DOCUMENT" or the package
  // name encodes the scanned directory. Syft emits one root entry per
  // scan whose name embeds the release tag, so a naive set-diff
  // reports bogus Added: 1 / Removed: 1 every cycle.
  if (pkg.SPDXID === "SPDXRef-DOCUMENT") return true;
  if (
    typeof pkg.name === "string" &&
    /qmonster-v[0-9]+\.[0-9]+\.[0-9]+/.test(pkg.name)
  ) {
    return true;
  }
  return false;
};

const read = (path) => {
  const doc = JSON.parse(fs.readFileSync(path, "utf8"));
  return (doc.packages || []).filter((p) => !isRootDocument(p));
};

const key = (pkg) =>
  `${pkg.name || "(unnamed)"}@${pkg.versionInfo || "(no-version)"}`;

const externalRefs = (pkg) =>
  Array.isArray(pkg?.externalRefs) ? pkg.externalRefs : [];
const hasPurl = (pkg) =>
  externalRefs(pkg).some(
    (ref) =>
      String(ref.referenceType || "")
        .toLowerCase()
        .includes("purl") ||
      String(ref.referenceLocator || "").startsWith("pkg:"),
  );
const emptyish = (value) =>
  !value || ["NOASSERTION", "NONE"].includes(String(value).toUpperCase());
const missingVersion = (pkg) => emptyish(pkg?.versionInfo);
const missingLicense = (pkg) =>
  emptyish(pkg?.licenseConcluded) && emptyish(pkg?.licenseDeclared);
const missingSupplier = (pkg) => emptyish(pkg?.supplier);

// Security/runtime-sensitive deps that warrant reviewer attention when
// they enter or leave the dependency set. Lowercase names; matched
// case-insensitively.
const ATTENTION_NAMES = new Set([
  "openssl",
  "openssl-sys",
  "native-tls",
  "ring",
  "rustls",
  "webpki",
  "tokio",
  "hyper",
  "reqwest",
  "curl",
  "rusqlite",
  "libsqlite3-sys",
  "sqlite",
  "serde_json",
  "clap",
  "crossterm",
  "ratatui",
  "mio",
  "nix",
  "libc",
  "zeroize",
]);
const isAttentionPackage = (pkg) =>
  ATTENTION_NAMES.has(String(pkg?.name || "").toLowerCase());

const metrics = (packages) => ({
  total: packages.length,
  purl: packages.filter(hasPurl).length,
  versioned: packages.filter((pkg) => !missingVersion(pkg)).length,
  missingVersion: packages.filter(missingVersion).length,
  missingLicense: packages.filter(missingLicense).length,
  missingSupplier: packages.filter(missingSupplier).length,
});

const previousPackages = read(previousPath);
const currentPackages = read(currentPath);
const previousByKey = new Map(previousPackages.map((pkg) => [key(pkg), pkg]));
const currentByKey = new Map(currentPackages.map((pkg) => [key(pkg), pkg]));
const previousKeys = new Set(previousByKey.keys());
const currentKeys = new Set(currentByKey.keys());
const added = [...currentKeys].filter((item) => !previousKeys.has(item)).sort();
const removed = [...previousKeys]
  .filter((item) => !currentKeys.has(item))
  .sort();
const addedPackages = added.map((item) => currentByKey.get(item));
const removedPackages = removed.map((item) => previousByKey.get(item));

const previousMetrics = metrics(previousPackages);
const currentMetrics = metrics(currentPackages);

const preview = (items) =>
  items.length === 0
    ? ["- none"]
    : items
        .slice(0, 25)
        .map((item) => `- ${item}`)
        .concat(items.length > 25 ? [`- ... ${items.length - 25} more`] : []);
const named = (packages, predicate) =>
  packages.filter(predicate).map(key).sort();

const lines = [];
lines.push("# SBOM Diff Summary");
lines.push("");
lines.push(`Previous tag: ${previousTag}`);
lines.push(`Current tag: ${currentTag}`);
lines.push(`Previous package count: ${previousKeys.size}`);
lines.push(`Current package count: ${currentKeys.size}`);
lines.push(`Added packages: ${added.length}`);
lines.push(`Removed packages: ${removed.length}`);
lines.push("");
lines.push("## Metadata / Risk Summary");
lines.push("");
lines.push("| Metric | Previous | Current |");
lines.push("| --- | ---: | ---: |");
lines.push(
  `| Package URL (purl) coverage | ${previousMetrics.purl}/${previousMetrics.total} | ${currentMetrics.purl}/${currentMetrics.total} |`,
);
lines.push(
  `| Versioned package coverage | ${previousMetrics.versioned}/${previousMetrics.total} | ${currentMetrics.versioned}/${currentMetrics.total} |`,
);
lines.push(
  `| Missing license assertion | ${previousMetrics.missingLicense} | ${currentMetrics.missingLicense} |`,
);
lines.push(
  `| Missing supplier assertion | ${previousMetrics.missingSupplier} | ${currentMetrics.missingSupplier} |`,
);
lines.push("");
lines.push(
  "This summary is a dependency-review aid, not a vulnerability scan.",
);
lines.push("");
lines.push("## Added Metadata Attention");
lines.push("");
lines.push("### Added without purl");
lines.push(preview(named(addedPackages, (pkg) => !hasPurl(pkg))).join("\n"));
lines.push("");
lines.push("### Added without version");
lines.push(preview(named(addedPackages, missingVersion)).join("\n"));
lines.push("");
lines.push("### Added without license assertion");
lines.push(preview(named(addedPackages, missingLicense)).join("\n"));
lines.push("");
lines.push("### Added in security/runtime attention set");
lines.push(preview(named(addedPackages, isAttentionPackage)).join("\n"));
lines.push("");
lines.push("## Removed from security/runtime attention set");
lines.push(preview(named(removedPackages, isAttentionPackage)).join("\n"));
lines.push("");
lines.push("## Added");
lines.push(preview(added).join("\n"));
lines.push("");
lines.push("## Removed");
lines.push(preview(removed).join("\n"));

process.stdout.write(lines.join("\n") + "\n");
