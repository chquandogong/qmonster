use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeSplitSuggestion {
    pub command: String,
    pub branch_name: String,
    pub path: String,
}

pub(crate) fn suggest_worktree_split_command(
    current_path: &str,
    current_branch: &str,
) -> Option<WorktreeSplitSuggestion> {
    let current_path = current_path.trim();
    if current_path.is_empty() {
        return None;
    }

    let repo_root = repo_root(Path::new(current_path))?;
    if !is_clean(&repo_root)? {
        return None;
    }

    let current_branch = if current_branch.trim().is_empty() {
        run_git(&repo_root, &["branch", "--show-current"])?
    } else {
        current_branch.trim().to_string()
    };
    let branch_slug = slug_for_ref(&current_branch)?;
    let branch_name = first_available_branch_name(&repo_root, &format!("{branch_slug}-split"))?;
    let repo_name = repo_root
        .file_name()
        .and_then(|s| s.to_str())
        .map(slug_for_path_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "worktree".into());
    let parent = repo_root.parent()?;
    let path_branch = slug_for_path_component(&branch_name);
    let path = first_available_path(parent.join(format!("{repo_name}-{path_branch}")));

    Some(WorktreeSplitSuggestion {
        command: format!(
            "git -C {} worktree add -b {} {} HEAD",
            shell_quote_path(&repo_root),
            shell_quote(&branch_name),
            shell_quote_path(&path)
        ),
        branch_name,
        path: path.display().to_string(),
    })
}

fn repo_root(current_path: &Path) -> Option<PathBuf> {
    run_git(current_path, &["rev-parse", "--show-toplevel"]).map(|raw| PathBuf::from(raw.trim()))
}

fn is_clean(repo_root: &Path) -> Option<bool> {
    run_git(repo_root, &["status", "--porcelain"]).map(|raw| raw.trim().is_empty())
}

fn first_available_branch_name(repo_root: &Path, base: &str) -> Option<String> {
    for idx in 0..100 {
        let candidate = if idx == 0 {
            base.to_string()
        } else {
            format!("{base}-{idx}")
        };
        if !branch_exists(repo_root, &candidate) {
            return Some(candidate);
        }
    }
    None
}

fn branch_exists(repo_root: &Path, branch: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn first_available_path(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    let parent = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = base
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("worktree")
        .to_string();
    for idx in 2..100 {
        let candidate = parent.join(format!("{stem}-{idx}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-next"))
}

fn slug_for_ref(value: &str) -> Option<String> {
    let mut slug = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("//") {
        slug = slug.replace("//", "/");
    }
    let slug = slug.trim_matches(&['-', '/'][..]).to_string();
    if slug.is_empty() || slug.starts_with('-') || slug.contains("..") {
        None
    } else {
        Some(slug)
    }
}

fn slug_for_path_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.display().to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_git(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether the pane's cwd is the primary checkout of a git repo or a
/// linked worktree created via `git worktree add`. None when the cwd
/// is not a git working tree at all, or when git is unavailable.
///
/// This is a *derived* fact about local git state — distinct from the
/// `signals.worktree_path` metric, which carries the value the
/// provider's statusline printed (and is stamped `ProviderOfficial`).
/// Keep the two off each other's source-kind contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorktreeRole {
    Primary,
    Linked { parent_repo_root: PathBuf },
}

/// Resolve the worktree role of `current_path`. Runs `git -C <path>
/// rev-parse --git-common-dir --git-dir` once; spawning git is the
/// only side effect. Returns `None` on empty input, non-existent
/// cwd, non-git cwd, git failure, or any parse anomaly.
pub(crate) fn resolve_worktree_role(current_path: &str) -> Option<WorktreeRole> {
    let trimmed = current_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cwd = Path::new(trimmed);
    if !cwd.exists() {
        return None;
    }

    let common_dir_raw = run_git(cwd, &["rev-parse", "--git-common-dir"])?;
    let git_dir_raw = run_git(cwd, &["rev-parse", "--git-dir"])?;

    let common_dir = canonicalize_git_path(cwd, &common_dir_raw)?;
    let git_dir = canonicalize_git_path(cwd, &git_dir_raw)?;

    if common_dir == git_dir {
        return Some(WorktreeRole::Primary);
    }

    let parent_repo_root = common_dir.parent()?.to_path_buf();
    Some(WorktreeRole::Linked { parent_repo_root })
}

/// `git rev-parse --git-{common-,}dir` returns paths relative to the
/// cwd it was invoked from. Canonicalize so Primary detection (path
/// equality) is robust against `.git` vs absolute spellings.
fn canonicalize_git_path(cwd: &Path, raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = Path::new(trimmed);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        cwd.join(candidate)
    };
    std::fs::canonicalize(&absolute).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git command runs");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn clean_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), &["init", "-b", "main"]);
        git(
            dir.path(),
            &["config", "user.email", "qmonster@example.test"],
        );
        git(dir.path(), &["config", "user.name", "Qmonster Test"]);
        std::fs::write(dir.path().join("README.md"), "test\n").expect("write fixture");
        git(dir.path(), &["add", "README.md"]);
        git(dir.path(), &["commit", "-m", "init"]);
        dir
    }

    #[test]
    fn clean_repo_gets_executable_worktree_add_command() {
        let repo = clean_repo();

        let suggestion = suggest_worktree_split_command(repo.path().to_str().unwrap(), "main")
            .expect("clean repo should produce a command");

        assert_eq!(suggestion.branch_name, "main-split");
        assert!(
            suggestion.path.ends_with("main-split"),
            "path should be deterministic and sibling-safe: {suggestion:?}"
        );
        assert!(
            suggestion.command.starts_with("git -C "),
            "command must be executable shell, got: {}",
            suggestion.command
        );
        assert!(
            suggestion.command.contains(" worktree add -b "),
            "command must create a worktree, got: {}",
            suggestion.command
        );
        assert!(
            !suggestion.command.contains('<') && !suggestion.command.trim_start().starts_with('#'),
            "copyable command must not contain placeholders/comments: {}",
            suggestion.command
        );
    }

    #[test]
    fn dirty_repo_does_not_get_copyable_worktree_command() {
        let repo = clean_repo();
        std::fs::write(repo.path().join("dirty.txt"), "dirty\n").expect("dirty fixture");

        assert!(
            suggest_worktree_split_command(repo.path().to_str().unwrap(), "main").is_none(),
            "dirty repos need a next-step/checkpoint, not a copyable branch split command"
        );
    }

    #[test]
    fn empty_branch_falls_back_to_git_current_branch() {
        let repo = clean_repo();

        let suggestion = suggest_worktree_split_command(repo.path().to_str().unwrap(), "")
            .expect("git current branch should fill missing provider branch");

        assert_eq!(suggestion.branch_name, "main-split");
    }

    #[test]
    fn branch_slash_is_preserved_for_branch_but_flattened_for_path() {
        let repo = clean_repo();

        let suggestion =
            suggest_worktree_split_command(repo.path().to_str().unwrap(), "feat/worktree-copy")
                .expect("slash branch should still produce command");

        assert_eq!(suggestion.branch_name, "feat/worktree-copy-split");
        assert!(
            suggestion.path.ends_with("feat-worktree-copy-split"),
            "path must be a flat sibling directory, got: {suggestion:?}"
        );
    }

    #[test]
    fn resolve_worktree_role_returns_primary_for_main_checkout() {
        let repo = clean_repo();

        let role = resolve_worktree_role(repo.path().to_str().unwrap())
            .expect("clean repo should resolve as a git repo");

        assert!(
            matches!(role, WorktreeRole::Primary),
            "main checkout must resolve as Primary, got {role:?}"
        );
    }

    #[test]
    fn resolve_worktree_role_returns_linked_with_parent_root_for_added_worktree() {
        // Wrap both the primary checkout and the linked worktree inside a
        // single parent tempdir so both directories are cleaned on drop and
        // parallel `cargo test` runs do not race on a shared `/tmp/linked-wt`.
        let parent = tempfile::tempdir().expect("tempdir");
        let primary = parent.path().join("primary");
        std::fs::create_dir(&primary).expect("create primary dir");

        // Inline clean_repo() against the explicit `primary` path.
        git(&primary, &["init", "-b", "main"]);
        git(&primary, &["config", "user.email", "qmonster@example.test"]);
        git(&primary, &["config", "user.name", "Qmonster Test"]);
        std::fs::write(primary.join("README.md"), "test\n").expect("write fixture");
        git(&primary, &["add", "README.md"]);
        git(&primary, &["commit", "-m", "init"]);

        let worktree_dir = parent.path().join("linked");
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "feat-x",
                worktree_dir.to_str().unwrap(),
                "HEAD",
            ],
        );

        let role = resolve_worktree_role(worktree_dir.to_str().unwrap())
            .expect("linked worktree should resolve as a git repo");

        match role {
            WorktreeRole::Linked { parent_repo_root } => {
                let canonical_repo = std::fs::canonicalize(&primary).unwrap();
                let canonical_parent = std::fs::canonicalize(&parent_repo_root).unwrap();
                assert_eq!(
                    canonical_parent, canonical_repo,
                    "Linked.parent_repo_root must resolve to the primary checkout"
                );
            }
            other => panic!("linked worktree must resolve as Linked, got {other:?}"),
        }
    }

    #[test]
    fn resolve_worktree_role_returns_none_for_non_git_cwd() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            resolve_worktree_role(dir.path().to_str().unwrap()).is_none(),
            "non-git cwd must return None, not Primary"
        );
    }

    #[test]
    fn resolve_worktree_role_returns_none_for_empty_or_missing_cwd() {
        assert!(resolve_worktree_role("").is_none());
        assert!(resolve_worktree_role("   ").is_none());
        assert!(resolve_worktree_role("/this/path/does/not/exist/qmonster-plan").is_none());
    }
}
