use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context as _;

const GIT_LABEL_WIDTH: usize = 10;
const RECENT_COMMIT_LIMIT: usize = 5;
const CONTRIBUTOR_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPanel {
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContribLine {
    pub commits: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitSnapshot {
    repo_root: PathBuf,
    branch: String,
    head: String,
    upstream: Option<String>,
    origin_url: Option<String>,
    ahead: usize,
    behind: usize,
    staged: usize,
    unstaged: usize,
    untracked: usize,
    status_lines: Vec<String>,
    recent_commits: Vec<String>,
    top_contributors: Vec<ContribLine>,
    extra_contributors: usize,
    extra_contributor_commits: usize,
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

    let origin_url = run_git_optional(&repo_root, &["config", "--get", "remote.origin.url"])
        .map(|raw| normalize_origin_url(&raw));

    let shortlog_raw = run_git_optional(&repo_root, &["shortlog", "-sne", "HEAD"])
        .or_else(|| run_git_optional(&repo_root, &["shortlog", "-sn", "HEAD"]))
        .unwrap_or_default();
    let all_contributors = parse_shortlog(&shortlog_raw);
    let (top_contributors, extra_contributors, extra_contributor_commits) =
        split_contributors(all_contributors, CONTRIBUTOR_LIMIT);

    Ok(GitSnapshot {
        repo_root,
        branch,
        head,
        upstream,
        origin_url,
        ahead,
        behind,
        staged,
        unstaged,
        untracked,
        status_lines,
        recent_commits,
        top_contributors,
        extra_contributors,
        extra_contributor_commits,
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
    lines.push(detail_line(
        "origin",
        snapshot
            .origin_url
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    ));

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

    lines.push(String::new());
    lines.push("Contributors".into());
    if snapshot.top_contributors.is_empty() {
        lines.push("  none".into());
    } else {
        for c in &snapshot.top_contributors {
            lines.push(format!("  {:>4}  {}", c.commits, c.name));
        }
        if snapshot.extra_contributors > 0 {
            lines.push(format!(
                "  +{} more ({} commits)",
                snapshot.extra_contributors, snapshot.extra_contributor_commits
            ));
        }
    }

    GitPanel {
        title: git_panel_title(),
        lines,
    }
}

/// Strip a trailing `.git` and rewrite SSH-style remotes to HTTPS so the
/// panel renders a URL operators can paste into a browser.
pub(crate) fn normalize_origin_url(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if let Some(rest) = stripped.strip_prefix("git@")
        && let Some((host, path)) = rest.split_once(':')
    {
        return format!("https://{host}/{path}");
    }
    if let Some(rest) = stripped.strip_prefix("ssh://git@")
        && let Some((host_port, path)) = rest.split_once('/')
    {
        let host = host_port.split(':').next().unwrap_or(host_port);
        return format!("https://{host}/{path}");
    }
    stripped.to_string()
}

/// Parse `git shortlog -sne` (or `-sn`) output.
///
/// Each line looks like `   150\tAlice <alice@example.com>` (tab) or with
/// run-on whitespace. The leading number is commit count; the email-bracket
/// suffix is dropped so the rendered list stays narrow.
pub(crate) fn parse_shortlog(raw: &str) -> Vec<ContribLine> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                return None;
            }
            let mut chars = trimmed.char_indices();
            let split_at = chars.find(|(_, c)| c.is_whitespace()).map(|(i, _)| i)?;
            let (count_str, rest) = trimmed.split_at(split_at);
            let commits = count_str.parse::<usize>().ok()?;
            let rest = rest.trim_start();
            let name_part = rest.split('<').next().unwrap_or(rest).trim();
            if name_part.is_empty() {
                return None;
            }
            Some(ContribLine {
                commits,
                name: name_part.to_string(),
            })
        })
        .collect()
}

/// Take the first `limit` contributors as the "top" list and roll the
/// remainder into a single `+N more (M commits)` aggregate so the
/// modal stays bounded on busy repos.
pub(crate) fn split_contributors(
    mut all: Vec<ContribLine>,
    limit: usize,
) -> (Vec<ContribLine>, usize, usize) {
    if all.len() <= limit {
        return (all, 0, 0);
    }
    let extras = all.split_off(limit);
    let extra_count = extras.len();
    let extra_commits = extras.iter().map(|c| c.commits).sum();
    (all, extra_count, extra_commits)
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

    #[test]
    fn normalize_origin_url_rewrites_ssh_to_https() {
        assert_eq!(
            normalize_origin_url("git@github.com:chquandogong/qmonster.git"),
            "https://github.com/chquandogong/qmonster"
        );
        assert_eq!(
            normalize_origin_url("git@gitlab.com:group/sub/proj.git"),
            "https://gitlab.com/group/sub/proj"
        );
    }

    #[test]
    fn normalize_origin_url_strips_dot_git_on_https() {
        assert_eq!(
            normalize_origin_url("https://github.com/chquandogong/qmonster.git"),
            "https://github.com/chquandogong/qmonster"
        );
        assert_eq!(
            normalize_origin_url("https://github.com/foo/bar"),
            "https://github.com/foo/bar"
        );
    }

    #[test]
    fn normalize_origin_url_handles_ssh_url_form() {
        assert_eq!(
            normalize_origin_url("ssh://git@github.com:22/chquandogong/qmonster.git"),
            "https://github.com/chquandogong/qmonster"
        );
    }

    #[test]
    fn parse_shortlog_strips_email_and_returns_count() {
        let raw = "   150\tAlice <alice@example.com>\n    47\tBob <bob@example.com>\n";
        let parsed = parse_shortlog(raw);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].commits, 150);
        assert_eq!(parsed[0].name, "Alice");
        assert_eq!(parsed[1].commits, 47);
        assert_eq!(parsed[1].name, "Bob");
    }

    #[test]
    fn parse_shortlog_skips_blank_and_malformed_lines() {
        let raw = "\n   not_a_number\tFoo\n   12\tValid Name\n";
        let parsed = parse_shortlog(raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].commits, 12);
        assert_eq!(parsed[0].name, "Valid Name");
    }

    #[test]
    fn split_contributors_caps_at_limit_and_reports_extras() {
        let lines = (0..7)
            .map(|i| ContribLine {
                commits: 10 + i,
                name: format!("U{i}"),
            })
            .collect::<Vec<_>>();
        let (top, extra_n, extra_commits) = split_contributors(lines, 5);
        assert_eq!(top.len(), 5);
        assert_eq!(extra_n, 2);
        // U5 (commits=15) + U6 (commits=16) = 31
        assert_eq!(extra_commits, 31);
    }

    #[test]
    fn split_contributors_passes_through_when_below_limit() {
        let lines = vec![ContribLine {
            commits: 5,
            name: "Solo".to_string(),
        }];
        let (top, extra_n, extra_commits) = split_contributors(lines.clone(), 5);
        assert_eq!(top, lines);
        assert_eq!(extra_n, 0);
        assert_eq!(extra_commits, 0);
    }

    #[test]
    fn panel_from_snapshot_renders_origin_and_contributors() {
        let snapshot = GitSnapshot {
            repo_root: PathBuf::from("/tmp/repo"),
            branch: "main".into(),
            head: "abc1234 init".into(),
            upstream: Some("origin/main".into()),
            origin_url: Some("https://github.com/chquandogong/qmonster".into()),
            ahead: 0,
            behind: 0,
            staged: 0,
            unstaged: 0,
            untracked: 0,
            status_lines: vec![],
            recent_commits: vec!["abc1234 init".into()],
            top_contributors: vec![
                ContribLine {
                    commits: 100,
                    name: "Alice".into(),
                },
                ContribLine {
                    commits: 40,
                    name: "Bob".into(),
                },
            ],
            extra_contributors: 3,
            extra_contributor_commits: 25,
        };
        let panel = panel_from_snapshot(snapshot);
        let joined = panel.lines.join("\n");
        assert!(
            joined.contains("origin     : https://github.com/chquandogong/qmonster"),
            "origin line missing: {joined}"
        );
        assert!(
            joined.contains("Contributors"),
            "Contributors header missing: {joined}"
        );
        assert!(
            joined.contains("100  Alice"),
            "top contributor row missing: {joined}"
        );
        assert!(
            joined.contains("+3 more (25 commits)"),
            "extras roll-up missing: {joined}"
        );
    }

    #[test]
    fn panel_from_snapshot_shows_none_when_no_origin_or_contributors() {
        let snapshot = GitSnapshot {
            repo_root: PathBuf::from("/tmp/repo"),
            branch: "main".into(),
            head: "abc1234 init".into(),
            upstream: None,
            origin_url: None,
            ahead: 0,
            behind: 0,
            staged: 0,
            unstaged: 0,
            untracked: 0,
            status_lines: vec![],
            recent_commits: vec![],
            top_contributors: vec![],
            extra_contributors: 0,
            extra_contributor_commits: 0,
        };
        let panel = panel_from_snapshot(snapshot);
        let joined = panel.lines.join("\n");
        assert!(joined.contains("origin     : none"));
        assert!(joined.contains("Contributors\n  none"));
    }
}
