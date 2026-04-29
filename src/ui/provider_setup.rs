//! Phase G-1 (v1.29.0): Provider Setup overlay — read-only guidance
//! for wiring Claude / Codex / Gemini to expose token + cache data
//! to Qmonster. Static snippet content; state detection probes
//! `~/.claude/`, `~/.codex/`, `~/.gemini/` without modifying anything.
//!
//! The overlay never writes provider config files. Operator copies
//! the displayed snippet and applies it manually.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSetupTab {
    Claude,
    Codex,
    Gemini,
}

impl ProviderSetupTab {
    pub fn label(self) -> &'static str {
        match self {
            ProviderSetupTab::Claude => "Claude",
            ProviderSetupTab::Codex => "Codex",
            ProviderSetupTab::Gemini => "Gemini",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ProviderSetupTab::Claude => ProviderSetupTab::Codex,
            ProviderSetupTab::Codex => ProviderSetupTab::Gemini,
            ProviderSetupTab::Gemini => ProviderSetupTab::Claude,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderSetupOverlay {
    pub tab: ProviderSetupTab,
    pub claude_sidefile_enabled: bool,
    pub codex_app_server_enabled: bool,
    pub scroll_offset: usize,
}

impl Default for ProviderSetupOverlay {
    fn default() -> Self {
        Self {
            tab: ProviderSetupTab::Claude,
            claude_sidefile_enabled: false,
            codex_app_server_enabled: false,
            scroll_offset: 0,
        }
    }
}

impl ProviderSetupOverlay {
    /// Toggle the per-tab boolean. For Claude, toggles
    /// `claude_sidefile_enabled`; for Codex, toggles
    /// `codex_app_server_enabled`; Gemini has no toggle (no-op).
    pub fn toggle(&mut self) {
        match self.tab {
            ProviderSetupTab::Claude => {
                self.claude_sidefile_enabled = !self.claude_sidefile_enabled;
            }
            ProviderSetupTab::Codex => {
                self.codex_app_server_enabled = !self.codex_app_server_enabled;
            }
            ProviderSetupTab::Gemini => {}
        }
        self.scroll_offset = 0; // reset scroll when content shape changes
    }

    pub fn switch_tab(&mut self, tab: ProviderSetupTab) {
        self.tab = tab;
        self.scroll_offset = 0;
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeState {
    pub statusline_script_present: bool,
    pub statusline_size_bytes: u64,
    pub exports_cache_read: bool,
    pub exports_cache_creation: bool,
    pub exports_input_tokens: bool,
    pub sidefile_export_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexState {
    pub config_present: bool,
    pub app_server_running: bool, // best-effort; we don't actually probe a socket
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GeminiFooterState {
    pub settings_present: bool,
    pub hide_cwd: bool,
    pub hide_sandbox_status: bool,
    pub hide_model_info: bool,
    pub hide_context_percentage: bool,
    pub hide_footer: bool,
}

pub fn detect_claude_state(home: &Path) -> ClaudeState {
    let script_path = home.join(".claude").join("statusline.sh");
    let (present, size_bytes, body) = match fs::metadata(&script_path) {
        Ok(md) if md.is_file() => {
            let bytes = md.len();
            let body = fs::read_to_string(&script_path).unwrap_or_default();
            (true, bytes, body)
        }
        _ => (false, 0, String::new()),
    };
    ClaudeState {
        statusline_script_present: present,
        statusline_size_bytes: size_bytes,
        exports_cache_read: body.contains("cache_read_input_tokens"),
        exports_cache_creation: body.contains("cache_creation_input_tokens"),
        exports_input_tokens: body.contains("input_tokens")
            && !body.contains("cache_read_input_tokens"),
        sidefile_export_present: body.contains("ai-cli-status/claude")
            || body.contains("> \"$state_path\"")
            || body.contains("printf '%s' \"$input\" >"),
    }
}

pub fn detect_codex_state(home: &Path) -> CodexState {
    let config_path = home.join(".codex").join("config.toml");
    CodexState {
        config_present: fs::metadata(&config_path)
            .map(|m| m.is_file())
            .unwrap_or(false),
        app_server_running: false,
    }
}

pub fn detect_gemini_footer_state(home: &Path) -> GeminiFooterState {
    let settings_path = home.join(".gemini").join("settings.json");
    let body = match fs::read_to_string(&settings_path) {
        Ok(s) => s,
        Err(_) => return GeminiFooterState::default(),
    };
    let mut out = GeminiFooterState {
        settings_present: true,
        ..GeminiFooterState::default()
    };
    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return out,
    };
    if let Some(footer) = json.pointer("/ui/footer") {
        out.hide_cwd = footer
            .get("hideCWD")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out.hide_sandbox_status = footer
            .get("hideSandboxStatus")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out.hide_model_info = footer
            .get("hideModelInfo")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out.hide_context_percentage = footer
            .get("hideContextPercentage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out.hide_footer = footer
            .get("hideFooter")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    }
    out
}

pub fn detect_home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|bd| bd.home_dir().to_path_buf())
}

pub const CLAUDE_STATUSLINE_BASE: &str =
    include_str!("provider_setup_snippets/claude_statusline_base.sh");
pub const CLAUDE_SIDEFILE_ADDON: &str =
    include_str!("provider_setup_snippets/claude_sidefile_addon.sh");
pub const CODEX_STATUSLINE_GUIDE: &str =
    include_str!("provider_setup_snippets/codex_statusline_guide.txt");
pub const CODEX_APP_SERVER_GUIDE: &str =
    include_str!("provider_setup_snippets/codex_app_server_guide.sh");
pub const GEMINI_FOOTER_SETTINGS: &str =
    include_str!("provider_setup_snippets/gemini_footer_settings.json");
pub const GEMINI_AUTH_NOTE: &str = include_str!("provider_setup_snippets/gemini_auth_note.txt");

/// Compose the full text shown for the active tab, accounting for the
/// per-tab toggle. Returns lines (Vec<String>) for ratatui Paragraph
/// rendering.
pub fn render_tab_content(
    overlay: &ProviderSetupOverlay,
    claude: &ClaudeState,
    codex: &CodexState,
    gemini: &GeminiFooterState,
) -> Vec<String> {
    let mut out = Vec::new();
    match overlay.tab {
        ProviderSetupTab::Claude => {
            out.push("=== Current state ===".into());
            out.push(format!(
                "  ~/.claude/statusline.sh: {}",
                if claude.statusline_script_present {
                    format!("present ({} bytes)", claude.statusline_size_bytes)
                } else {
                    "MISSING".into()
                }
            ));
            out.push(format!(
                "  exports cache_read_input_tokens: {}",
                if claude.exports_cache_read {
                    "YES"
                } else {
                    "NO"
                }
            ));
            out.push(format!(
                "  exports cache_creation_input_tokens: {}",
                if claude.exports_cache_creation {
                    "YES"
                } else {
                    "NO"
                }
            ));
            out.push(format!(
                "  sidefile JSON export: {}",
                if claude.sidefile_export_present {
                    "YES"
                } else {
                    "NO"
                }
            ));
            out.push(String::new());
            out.push(format!(
                "[s] toggle Sidefile JSON export section: {}",
                if overlay.claude_sidefile_enabled {
                    "[x]"
                } else {
                    "[ ]"
                }
            ));
            out.push(String::new());
            out.push("=== Recommended ~/.claude/statusline.sh ===".into());
            out.push("# Copy the block below into ~/.claude/statusline.sh".into());
            out.push("# then: chmod +x ~/.claude/statusline.sh".into());
            out.push(String::new());
            for line in CLAUDE_STATUSLINE_BASE.lines() {
                out.push(line.to_string());
            }
            if overlay.claude_sidefile_enabled {
                out.push(String::new());
                out.push("=== Sidefile JSON export (paste into the script's top) ===".into());
                out.push("# Saves the full statusLine JSON to a per-session file.".into());
                out.push("# Future Qmonster slices (F-5) can read this file to surface".into());
                out.push("# cache_read_input_tokens, cost, resets_at directly.".into());
                out.push(String::new());
                for line in CLAUDE_SIDEFILE_ADDON.lines() {
                    out.push(line.to_string());
                }
                out.push(String::new());
                out.push(
                    "# Sidefile path: ~/.local/share/ai-cli-status/claude/<session_id>.json".into(),
                );
            }
            out.push(String::new());
            out.push("=== Wiring (one-time) ===".into());
            out.push("Add to ~/.claude/settings.json:".into());
            out.push(r#"  "statusLine": {"#.into());
            out.push(r#"    "type": "command","#.into());
            out.push(r#"    "command": "$HOME/.claude/statusline.sh""#.into());
            out.push(r#"  }"#.into());
        }
        ProviderSetupTab::Codex => {
            out.push("=== Current state ===".into());
            out.push(format!(
                "  ~/.codex/config.toml: {}",
                if codex.config_present {
                    "present"
                } else {
                    "MISSING"
                }
            ));
            out.push("  /statusline live toggles: not detectable (run /statusline in pane)".into());
            out.push(String::new());
            out.push(format!(
                "[s] toggle Codex App Server section: {}",
                if overlay.codex_app_server_enabled {
                    "[x]"
                } else {
                    "[ ]"
                }
            ));
            out.push(String::new());
            out.push("=== /statusline ON/OFF guidance ===".into());
            for line in CODEX_STATUSLINE_GUIDE.lines() {
                out.push(line.to_string());
            }
            if overlay.codex_app_server_enabled {
                out.push(String::new());
                out.push("=== Codex App Server polling (advanced) ===".into());
                for line in CODEX_APP_SERVER_GUIDE.lines() {
                    out.push(line.to_string());
                }
            }
        }
        ProviderSetupTab::Gemini => {
            out.push("=== Current state ===".into());
            out.push(format!(
                "  ~/.gemini/settings.json: {}",
                if gemini.settings_present {
                    "present"
                } else {
                    "MISSING"
                }
            ));
            out.push(format!("    hideCWD: {}", gemini.hide_cwd));
            out.push(format!(
                "    hideSandboxStatus: {}",
                gemini.hide_sandbox_status
            ));
            out.push(format!("    hideModelInfo: {}", gemini.hide_model_info));
            out.push(format!(
                "    hideContextPercentage: {}",
                gemini.hide_context_percentage
            ));
            out.push(format!("    hideFooter: {}", gemini.hide_footer));
            out.push(String::new());
            out.push("=== Recommended ~/.gemini/settings.json (merge with existing) ===".into());
            for line in GEMINI_FOOTER_SETTINGS.lines() {
                out.push(line.to_string());
            }
            out.push(String::new());
            out.push("=== /stats periodic dispatch ===".into());
            out.push("Qmonster's `u` key already cycles `/stats session` →".into());
            out.push("`/stats model` → `/stats tools` on the selected Gemini pane.".into());
            out.push("No additional setup needed — just keep using `u` periodically.".into());
            out.push(String::new());
            out.push("=== Auth note (informational, no action) ===".into());
            for line in GEMINI_AUTH_NOTE.lines() {
                out.push(line.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detect_claude_state_reports_missing_when_no_file() {
        let tmp = tempdir().unwrap();
        let s = detect_claude_state(tmp.path());
        assert!(!s.statusline_script_present);
        assert_eq!(s.statusline_size_bytes, 0);
        assert!(!s.exports_cache_read);
    }

    #[test]
    fn detect_claude_state_reports_basic_script_with_no_cache() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("statusline.sh"),
            "#!/bin/sh\nmodel=$(jq -r '.model.display_name')\n",
        )
        .unwrap();
        let s = detect_claude_state(tmp.path());
        assert!(s.statusline_script_present);
        assert!(s.statusline_size_bytes > 0);
        assert!(!s.exports_cache_read);
        assert!(!s.sidefile_export_present);
    }

    #[test]
    fn detect_claude_state_reports_cache_when_present() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("statusline.sh"),
            "#!/bin/sh\ncache_read=$(jq -r '.context_window.current_usage.cache_read_input_tokens')\n",
        )
        .unwrap();
        let s = detect_claude_state(tmp.path());
        assert!(s.exports_cache_read);
    }

    #[test]
    fn detect_gemini_footer_state_parses_settings_json() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(".gemini");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{"ui":{"footer":{"hideCWD":true,"hideFooter":false}}}"#,
        )
        .unwrap();
        let s = detect_gemini_footer_state(tmp.path());
        assert!(s.settings_present);
        assert!(s.hide_cwd);
        assert!(!s.hide_footer);
    }

    #[test]
    fn detect_gemini_footer_state_handles_missing_file() {
        let tmp = tempdir().unwrap();
        let s = detect_gemini_footer_state(tmp.path());
        assert!(!s.settings_present);
    }

    #[test]
    fn overlay_toggle_flips_per_tab_boolean() {
        let mut o = ProviderSetupOverlay::default();
        assert!(!o.claude_sidefile_enabled);
        o.toggle(); // tab is Claude
        assert!(o.claude_sidefile_enabled);
        o.tab = ProviderSetupTab::Codex;
        o.toggle();
        assert!(o.codex_app_server_enabled);
        o.tab = ProviderSetupTab::Gemini;
        let before = o.claude_sidefile_enabled;
        o.toggle(); // no-op for Gemini
        assert_eq!(o.claude_sidefile_enabled, before);
    }

    #[test]
    fn render_tab_content_claude_includes_state_and_snippet() {
        let overlay = ProviderSetupOverlay::default();
        let claude = ClaudeState {
            statusline_script_present: true,
            statusline_size_bytes: 1718,
            exports_cache_read: false,
            exports_cache_creation: false,
            exports_input_tokens: false,
            sidefile_export_present: false,
        };
        let codex = CodexState {
            config_present: false,
            app_server_running: false,
        };
        let gemini = GeminiFooterState::default();
        let lines = render_tab_content(&overlay, &claude, &codex, &gemini);
        let text = lines.join("\n");
        assert!(text.contains("Current state"));
        assert!(text.contains("1718 bytes"));
        assert!(text.contains("Recommended ~/.claude/statusline.sh"));
        assert!(
            !text.contains("Sidefile JSON export (paste"),
            "sidefile section should be hidden by default"
        );
    }

    #[test]
    fn render_tab_content_claude_shows_sidefile_when_toggled() {
        let overlay = ProviderSetupOverlay {
            claude_sidefile_enabled: true,
            ..Default::default()
        };
        let claude = ClaudeState {
            statusline_script_present: true,
            statusline_size_bytes: 1718,
            exports_cache_read: false,
            exports_cache_creation: false,
            exports_input_tokens: false,
            sidefile_export_present: false,
        };
        let codex = CodexState {
            config_present: false,
            app_server_running: false,
        };
        let gemini = GeminiFooterState::default();
        let lines = render_tab_content(&overlay, &claude, &codex, &gemini);
        let text = lines.join("\n");
        assert!(text.contains("Sidefile JSON export"));
        assert!(text.contains("ai-cli-status/claude"));
    }
}
