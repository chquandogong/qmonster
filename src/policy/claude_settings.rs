use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ClaudeSettings {
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "permissionMode", alias = "permission_mode")]
    permission_mode: Option<String>,
    #[serde(default, rename = "allowedTools", alias = "allowed_tools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(default, rename = "disallowedTools", alias = "disallowed_tools")]
    disallowed_tools: Option<Vec<String>>,
    #[serde(
        default,
        rename = "additionalDirectories",
        alias = "additional_directories",
        alias = "addDirs"
    )]
    additional_directories: Option<Vec<String>>,
    // Other settings.json keys are ignored.
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeSettingsError {
    // Note: `Io(NotFound)` already covers the missing-file case via
    // `std::io::ErrorKind::NotFound`, so a separate `NotFound` variant
    // would be dead — never constructed by the code.
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

    pub fn permission_mode(&self) -> Option<&str> {
        self.permission_mode.as_deref()
    }

    pub fn allowed_tools(&self) -> &[String] {
        self.allowed_tools.as_deref().unwrap_or(&[])
    }

    pub fn disallowed_tools(&self) -> &[String] {
        self.disallowed_tools.as_deref().unwrap_or(&[])
    }

    pub fn additional_directories(&self) -> &[String] {
        self.additional_directories.as_deref().unwrap_or(&[])
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
    fn claude_settings_loads_runtime_fields_from_json() {
        let f = write_json(
            r#"{
                "permissionMode": "bypassPermissions",
                "allowedTools": ["Bash(git *)", "Read"],
                "disallowedTools": ["Bash(rm *)"],
                "additionalDirectories": ["/tmp/shared"]
            }"#,
        );
        let s = ClaudeSettings::load_from_path(f.path()).unwrap();
        assert_eq!(s.permission_mode(), Some("bypassPermissions"));
        assert_eq!(s.allowed_tools(), &["Bash(git *)", "Read"]);
        assert_eq!(s.disallowed_tools(), &["Bash(rm *)"]);
        assert_eq!(s.additional_directories(), &["/tmp/shared"]);
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
