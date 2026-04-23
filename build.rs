//! Build script: capture a git-based version string at compile time and
//! expose it to the crate via `env!("QMONSTER_GIT_VERSION")`. The TUI
//! footer badge renders this instead of `CARGO_PKG_VERSION` so the
//! version operators see reflects the actual code in the binary — not
//! the rarely-bumped package version in Cargo.toml.
//!
//! Fallback: if `git` is unavailable or the repo isn't a git tree
//! (e.g. shipped from a tarball), the label falls back to
//! `v{CARGO_PKG_VERSION}-nogit` so the binary still builds and runs.

use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo");
    let repo = Path::new(&manifest_dir);

    // Ask Cargo to re-run this script whenever HEAD moves or the
    // currently-checked-out ref is updated; otherwise the embedded
    // version would go stale across commits without a `cargo clean`.
    println!("cargo:rerun-if-changed=build.rs");
    let head_file = repo.join(".git/HEAD");
    if head_file.is_file() {
        println!("cargo:rerun-if-changed=.git/HEAD");
        if let Ok(head) = std::fs::read_to_string(&head_file)
            && let Some(refname) = head.strip_prefix("ref: ").map(|s| s.trim())
        {
            let ref_path = repo.join(".git").join(refname);
            if ref_path.is_file() {
                // `ref: refs/heads/main` → watch .git/refs/heads/main
                println!("cargo:rerun-if-changed=.git/{refname}");
            }
        }
    }

    let version = git_describe(repo).unwrap_or_else(|| {
        let pkg = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into());
        format!("v{pkg}-nogit")
    });
    println!("cargo:rustc-env=QMONSTER_GIT_VERSION={version}");
}

fn git_describe(repo: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty=-dirty"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
