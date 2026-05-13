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
    let current_branch = current_branch.trim();
    if current_path.is_empty() || current_branch.is_empty() {
        return None;
    }

    let repo_root = repo_root(Path::new(current_path))?;
    if !is_clean(&repo_root)? {
        return None;
    }

    let branch_slug = slug_for_ref(current_branch)?;
    let branch_name = first_available_branch_name(&repo_root, &format!("{branch_slug}-split"))?;
    let repo_name = repo_root
        .file_name()
        .and_then(|s| s.to_str())
        .map(slug_for_path_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "worktree".into());
    let parent = repo_root.parent()?;
    let path = first_available_path(parent.join(format!("{repo_name}-{branch_name}")));

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
    run_git(current_path, &["rev-parse", "--show-toplevel"])
        .map(|raw| PathBuf::from(raw.trim()))
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
        git(dir.path(), &["config", "user.email", "qmonster@example.test"]);
        git(dir.path(), &["config", "user.name", "Qmonster Test"]);
        std::fs::write(dir.path().join("README.md"), "test\n").expect("write fixture");
        git(dir.path(), &["add", "README.md"]);
        git(dir.path(), &["commit", "-m", "init"]);
        dir
    }

    #[test]
    fn clean_repo_gets_executable_worktree_add_command() {
        let repo = clean_repo();

        let suggestion =
            suggest_worktree_split_command(repo.path().to_str().unwrap(), "main")
                .expect("clean repo should produce a command");

        assert_eq!(suggestion.branch_name, "main-split");
        assert!(
            suggestion.path.ends_with("qmonster-worktree-main-split"),
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
}
