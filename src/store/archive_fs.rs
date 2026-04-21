use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use thiserror::Error;

use crate::store::paths::QmonsterPaths;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub enum ArchiveOutcome {
    /// Tail was short; nothing written. `preview` is the tail verbatim.
    Skipped { preview: String },
    /// Tail exceeded the threshold and was written to disk.
    Archived {
        preview: String,
        path: PathBuf,
        bytes: usize,
    },
}

/// Writes raw pane tails to `~/.qmonster/archive/YYYY-MM-DD/<pane>/…`
/// only when they exceed `threshold_chars` (`big_output_chars` in
/// config). Short tails stay in memory as previews.
///
/// This is the **only** place in the crate where raw pane bytes are
/// written to disk. By keeping it separate from `store::audit`, we
/// preserve the r2 rule that the audit log cannot accept raw tails.
#[derive(Debug, Clone)]
pub struct ArchiveWriter {
    paths: QmonsterPaths,
    threshold_chars: usize,
}

impl ArchiveWriter {
    pub fn new(paths: QmonsterPaths, threshold_chars: usize) -> Self {
        Self {
            paths,
            threshold_chars,
        }
    }

    /// Archive `tail` if its char count exceeds the threshold; return
    /// a preview either way.
    pub fn archive_if_long(
        &self,
        pane_id: &str,
        tail: &str,
    ) -> Result<ArchiveOutcome, ArchiveError> {
        let preview = preview_of(tail, self.threshold_chars);
        if tail.chars().count() <= self.threshold_chars {
            return Ok(ArchiveOutcome::Skipped { preview });
        }

        let now = Utc::now();
        let day = now.format("%Y-%m-%d").to_string();
        let stamp = now.format("%Y%m%dT%H%M%S%fZ").to_string();
        let safe_pane = sanitize(pane_id);

        let dir = self.paths.archive_dir().join(day).join(safe_pane);
        std::fs::create_dir_all(&dir)?;
        let file_path = dir.join(format!("{stamp}.log"));
        {
            let mut f = std::fs::File::create(&file_path)?;
            f.write_all(tail.as_bytes())?;
            f.sync_all()?;
        }
        Ok(ArchiveOutcome::Archived {
            preview,
            path: file_path,
            bytes: tail.len(),
        })
    }
}

fn preview_of(tail: &str, limit: usize) -> String {
    tail.chars().take(limit).collect()
}

fn sanitize(pane_id: &str) -> String {
    // Only keep ASCII alphanumerics, `-`, and `_`. Dots are replaced so
    // traversal fragments (`..`) cannot survive sanitization even as
    // literal strings inside a directory name.
    pane_id
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() => c,
            '-' | '_' => c,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::paths::QmonsterPaths;
    use tempfile::TempDir;

    #[test]
    fn short_tail_returns_preview_and_no_file_written() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let writer = ArchiveWriter::new(paths, /*threshold=*/ 100);

        let result = writer
            .archive_if_long("%1", "small tail")
            .expect("archive call");

        match result {
            ArchiveOutcome::Skipped { preview } => {
                assert_eq!(preview, "small tail");
            }
            _ => panic!("expected Skipped"),
        }
        let entries: Vec<_> = walk_files(td.path()).collect();
        assert!(entries.is_empty(), "no files expected; got {entries:?}");
    }

    #[test]
    fn long_tail_writes_full_and_returns_preview_plus_path() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let threshold = 20usize;
        let writer = ArchiveWriter::new(paths, threshold);

        let tail = "A".repeat(500);
        let result = writer.archive_if_long("%7", &tail).unwrap();

        match result {
            ArchiveOutcome::Archived {
                preview,
                path,
                bytes,
            } => {
                assert_eq!(preview.chars().count(), threshold);
                assert_eq!(bytes, 500);
                assert!(path.exists(), "archive file must exist at {path:?}");
                let written = std::fs::read_to_string(&path).unwrap();
                assert_eq!(written, tail);
            }
            _ => panic!("expected Archived"),
        }
    }

    #[test]
    fn archive_files_live_under_archive_root_only() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let writer = ArchiveWriter::new(paths.clone(), 10);
        let tail = "X".repeat(200);
        let result = writer.archive_if_long("%1", &tail).unwrap();
        let ArchiveOutcome::Archived { path, .. } = result else {
            panic!("expected Archived");
        };
        assert!(
            path.starts_with(paths.archive_dir()),
            "written path {path:?} must live under archive root"
        );
    }

    #[test]
    fn pane_id_is_sanitized_for_filesystem() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let writer = ArchiveWriter::new(paths.clone(), 1);
        let tail = "hello world".to_string();
        let result = writer.archive_if_long("%42/../evil", &tail).unwrap();
        let ArchiveOutcome::Archived { path, .. } = result else {
            panic!("expected Archived");
        };
        assert!(path.starts_with(paths.archive_dir()));
        let rel = path.strip_prefix(paths.archive_dir()).unwrap();
        let s = rel.to_string_lossy();
        assert!(!s.contains(".."), "sanitized path leaked traversal: {s}");
    }

    fn walk_files(dir: &std::path::Path) -> impl Iterator<Item = std::path::PathBuf> {
        fn inner(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(d) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        inner(&p, out);
                    } else {
                        out.push(p);
                    }
                }
            }
        }
        let mut out = Vec::new();
        inner(dir, &mut out);
        out.into_iter()
    }
}
