use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context as _;

const GIT_LABEL_WIDTH: usize = 10;
const RECENT_COMMIT_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPanel {
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitSnapshot {
    repo_root: PathBuf,
    branch: String,
    head: String,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
    staged: usize,
    unstaged: usize,
    untracked: usize,
    status_lines: Vec<String>,
    recent_commits: Vec<String>,
}

pub fn capture_repo_panel() -> GitPanel {
    let repo_hint = Path::new(env!("CARGO_MANIFEST_DIR"));
    match capture_snapshot(repo_hint) {
        Ok(snapshot) => panel_from_snapshot(snapshot),
        Err(err) => GitPanel {
            title: git_panel_title(),
            lines: vec![
                detail_line("repo", repo_hint.display().to_string()),
                detail_line("status", "unavailable"),
                detail_line("error", err.to_string()),
            ],
        },
    }
}

fn capture_snapshot(repo_hint: &Path) -> anyhow::Result<GitSnapshot> {
    let repo_root = PathBuf::from(
        run_git(repo_hint, &["rev-parse", "--show-toplevel"])?
            .trim()
            .to_string(),
    );
    let branch = run_git(&repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let head = run_git(&repo_root, &["log", "-1", "--pretty=format:%h %s"])?;
    let upstream = run_git_optional(
        &repo_root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    );
    let (ahead, behind) = match upstream {
        Some(_) => parse_tracking_counts(&run_git(
            &repo_root,
            &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        )?)?,
        None => (0, 0),
    };
    let status_lines = run_git_optional(&repo_root, &["status", "--short"])
        .unwrap_or_default()
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let (staged, unstaged, untracked) = summarize_status_lines(&status_lines);
    let recent_commits = run_git(&repo_root, &["log", "--oneline", "-n", "5"])?
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .take(RECENT_COMMIT_LIMIT)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    Ok(GitSnapshot {
        repo_root,
        branch,
        head,
        upstream,
        ahead,
        behind,
        staged,
        unstaged,
        untracked,
        status_lines,
        recent_commits,
    })
}

fn panel_from_snapshot(snapshot: GitSnapshot) -> GitPanel {
    let mut lines = vec![
        detail_line("repo", snapshot.repo_root.display().to_string()),
        detail_line("branch", snapshot.branch),
        detail_line("head", snapshot.head),
    ];
    let upstream = snapshot.upstream.as_deref().map_or_else(
        || "none".to_string(),
        |name| {
            format!(
                "{name} (ahead {} · behind {})",
                snapshot.ahead, snapshot.behind
            )
        },
    );
    lines.push(detail_line("upstream", upstream));

    let total = snapshot.staged + snapshot.unstaged + snapshot.untracked;
    let worktree = if total == 0 {
        "clean".to_string()
    } else {
        format!(
            "staged {} · unstaged {} · untracked {} · total {}",
            snapshot.staged, snapshot.unstaged, snapshot.untracked, total
        )
    };
    lines.push(detail_line("worktree", worktree));

    lines.push(String::new());
    lines.push("Changes".into());
    if snapshot.status_lines.is_empty() {
        lines.push("  clean".into());
    } else {
        for line in snapshot.status_lines {
            lines.push(format!("  {line}"));
        }
    }

    lines.push(String::new());
    lines.push("Recent Commits".into());
    if snapshot.recent_commits.is_empty() {
        lines.push("  none".into());
    } else {
        for line in snapshot.recent_commits {
            lines.push(format!("  {line}"));
        }
    }

    GitPanel {
        title: git_panel_title(),
        lines,
    }
}

fn git_panel_title() -> String {
    format!("Git · qmonster {}", env!("QMONSTER_GIT_VERSION"))
}

fn detail_line(label: &str, value: impl Into<String>) -> String {
    format!("{label:<GIT_LABEL_WIDTH$} : {}", value.into())
}

fn parse_tracking_counts(raw: &str) -> anyhow::Result<(usize, usize)> {
    let mut parts = raw.split_whitespace();
    let ahead = parts
        .next()
        .context("missing ahead count")?
        .parse::<usize>()
        .context("invalid ahead count")?;
    let behind = parts
        .next()
        .context("missing behind count")?
        .parse::<usize>()
        .context("invalid behind count")?;
    Ok((ahead, behind))
}

fn summarize_status_lines(lines: &[String]) -> (usize, usize, usize) {
    let mut staged = 0usize;
    let mut unstaged = 0usize;
    let mut untracked = 0usize;
    for line in lines {
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        if x == '?' && y == '?' {
            untracked += 1;
            continue;
        }
        if x != ' ' {
            staged += 1;
        }
        if y != ' ' {
            unstaged += 1;
        }
    }
    (staged, unstaged, untracked)
}

fn run_git(repo_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "git {} failed{}",
            args.join(" "),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git_optional(repo_root: &Path, args: &[&str]) -> Option<String> {
    run_git(repo_root, args)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tracking_counts_reads_ahead_and_behind() {
        assert_eq!(parse_tracking_counts("3\t2").unwrap(), (3, 2));
        assert_eq!(parse_tracking_counts("0 0").unwrap(), (0, 0));
    }

    #[test]
    fn summarize_status_lines_counts_each_bucket() {
        let lines = vec![
            "M  src/main.rs".to_string(),
            " M README.md".to_string(),
            "MM Cargo.toml".to_string(),
            "?? notes.txt".to_string(),
        ];
        assert_eq!(summarize_status_lines(&lines), (2, 2, 1));
    }

    #[test]
    fn git_panel_title_uses_footer_git_version() {
        assert_eq!(
            git_panel_title(),
            format!("Git · qmonster {}", env!("QMONSTER_GIT_VERSION"))
        );
    }
}
