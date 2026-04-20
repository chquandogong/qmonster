use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::store::paths::QmonsterPaths;

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialize: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// One pane's slice of a snapshot. Deliberately shallow: snapshots are
/// a checkpoint of what the operator was looking at, not a mirror of
/// every field in `PaneReport`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub pane_id: String,
    pub provider: String,
    pub role: String,
    pub alerts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInput {
    pub reason: String,
    pub pane_summaries: Vec<PaneSnapshot>,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SnapshotFile<'a> {
    timestamp_utc: String,
    #[serde(flatten)]
    input: &'a SnapshotInput,
}

/// Writes runtime checkpoints to `~/.qmonster/snapshots/<ts>.json`.
/// **Never** touches `.mission/CURRENT_STATE.md` — that is a day-end
/// human document (r2 §3, §4).
#[derive(Debug, Clone)]
pub struct SnapshotWriter {
    paths: QmonsterPaths,
}

impl SnapshotWriter {
    pub fn new(paths: QmonsterPaths) -> Self {
        Self { paths }
    }

    pub fn write(&self, input: &SnapshotInput) -> Result<PathBuf, SnapshotError> {
        let now = Utc::now();
        let stamp = now.format("%Y%m%dT%H%M%S%fZ").to_string();
        std::fs::create_dir_all(self.paths.snapshot_dir())?;
        let path = self.paths.snapshot_dir().join(format!("{stamp}.json"));
        let file = SnapshotFile {
            timestamp_utc: now.to_rfc3339(),
            input,
        };
        let data = serde_json::to_vec_pretty(&file)?;
        std::fs::write(&path, data)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::paths::QmonsterPaths;
    use tempfile::TempDir;

    #[test]
    fn write_produces_json_under_snapshot_dir() {
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let writer = SnapshotWriter::new(paths.clone());

        let snap = SnapshotInput {
            reason: "operator-requested".into(),
            pane_summaries: vec![PaneSnapshot {
                pane_id: "%1".into(),
                provider: "Claude".into(),
                role: "Main".into(),
                alerts: vec!["notify-input-wait".into()],
            }],
            notices: vec!["version drift: tmux 3.4 -> 3.5".into()],
        };

        let path = writer.write(&snap).unwrap();
        assert!(path.starts_with(paths.snapshot_dir()));
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("operator-requested"));
        assert!(text.contains("notify-input-wait"));
    }

    #[test]
    fn snapshot_writer_never_writes_under_mission_dir() {
        // Paranoia guard: snapshots must land under ~/.qmonster/snapshots/,
        // never under .mission/ (r2 §3).
        let td = TempDir::new().unwrap();
        let paths = QmonsterPaths::at(td.path());
        paths.ensure().unwrap();
        let writer = SnapshotWriter::new(paths.clone());
        let snap = SnapshotInput {
            reason: "x".into(),
            pane_summaries: vec![],
            notices: vec![],
        };
        let path = writer.write(&snap).unwrap();
        assert!(!path.to_string_lossy().contains(".mission"));
    }
}
