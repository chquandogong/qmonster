use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ClaudeSettings {
    model: Option<String>,
    // Other settings.json keys are ignored — Slice 2 only surfaces `model`.
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeSettingsError {
    #[error("claude settings not found at {0}")]
    NotFound(PathBuf),
    #[error("failed to read claude settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse claude settings: {0}")]
    Parse(#[from] serde_json::Error),
}

impl ClaudeSettings {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Standard location: `$HOME/.claude/settings.json`. Returns None
    /// when `HOME` is unset (uncommon but possible in sandboxes).
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/settings.json"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ClaudeSettingsError> {
        let text = fs::read_to_string(path)?;
        let parsed: Self = serde_json::from_str(&text)?;
        Ok(parsed)
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_json(body: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", body).unwrap();
        f
    }

    #[test]
    fn claude_settings_empty_has_no_model() {
        assert!(ClaudeSettings::empty().model().is_none());
    }

    #[test]
    fn claude_settings_loads_model_from_json() {
        let f = write_json(r#"{"model": "claude-sonnet-4-6"}"#);
        let s = ClaudeSettings::load_from_path(f.path()).unwrap();
        assert_eq!(s.model(), Some("claude-sonnet-4-6"));
    }

    #[test]
    fn claude_settings_missing_model_key_returns_none() {
        let f = write_json(r#"{"other_key": "value"}"#);
        let s = ClaudeSettings::load_from_path(f.path()).unwrap();
        assert!(s.model().is_none());
    }

    #[test]
    fn claude_settings_missing_file_returns_io_not_found() {
        let result = ClaudeSettings::load_from_path(Path::new("/nonexistent/settings.json"));
        match result {
            Err(ClaudeSettingsError::Io(io)) => {
                assert_eq!(io.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected Io(NotFound), got {other:?}"),
        }
    }

    #[test]
    fn claude_settings_parse_error_surfaces_via_result() {
        let f = write_json(r#"not valid json at all"#);
        let result = ClaudeSettings::load_from_path(f.path());
        assert!(
            matches!(result, Err(ClaudeSettingsError::Parse(_))),
            "expected Parse error, got {result:?}"
        );
    }
}
