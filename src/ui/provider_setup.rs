//! Phase G-1 (v1.29.0): Provider Setup overlay — read-only guidance
//! for wiring Claude / Codex / Gemini to expose token + cache data
//! to Qmonster. Static snippet content; state detection probes
//! `~/.claude/`, `~/.codex/`, `~/.gemini/` without modifying anything.
//! The Tmux tab provides a copyable installer for the recommended
//! four-pane launcher/config bundle.
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
    Tmux,
}

impl ProviderSetupTab {
    pub fn label(self) -> &'static str {
        match self {
            ProviderSetupTab::Claude => "Claude",
            ProviderSetupTab::Codex => "Codex",
            ProviderSetupTab::Gemini => "Gemini",
            ProviderSetupTab::Tmux => "Tmux",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ProviderSetupTab::Claude => ProviderSetupTab::Codex,
            ProviderSetupTab::Codex => ProviderSetupTab::Gemini,
            ProviderSetupTab::Gemini => ProviderSetupTab::Tmux,
            ProviderSetupTab::Tmux => ProviderSetupTab::Claude,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            ProviderSetupTab::Claude => ProviderSetupTab::Tmux,
            ProviderSetupTab::Codex => ProviderSetupTab::Claude,
            ProviderSetupTab::Gemini => ProviderSetupTab::Codex,
            ProviderSetupTab::Tmux => ProviderSetupTab::Gemini,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderSetupOverlay {
    pub tab: ProviderSetupTab,
    pub claude_sidefile_enabled: bool,
    pub codex_app_server_enabled: bool,
    pub scroll_offset: usize,
    /// Whether the overlay is currently visible. Mirrors the
    /// `SettingsOverlay::open` field so the dashboard frame can
    /// short-circuit pane focus + key/mouse dispatch while the
    /// modal is on screen.
    open: bool,
}

impl Default for ProviderSetupOverlay {
    fn default() -> Self {
        Self {
            tab: ProviderSetupTab::Claude,
            claude_sidefile_enabled: false,
            codex_app_server_enabled: false,
            scroll_offset: 0,
            open: false,
        }
    }
}

impl ProviderSetupOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build an overlay whose read-only integration flags match the
    /// operator's persisted `[provider_setup]` config defaults — used
    /// at TUI startup so Provider Setup previews mirror Settings.
    pub fn from_config(config: &crate::app::config::QmonsterConfig) -> Self {
        Self {
            tab: ProviderSetupTab::Claude,
            claude_sidefile_enabled: config.provider_setup.claude_sidefile,
            codex_app_server_enabled: config.provider_setup.codex_app_server,
            scroll_offset: 0,
            open: false,
        }
    }

    pub fn sync_from_config(&mut self, config: &crate::app::config::QmonsterConfig) {
        self.claude_sidefile_enabled = config.provider_setup.claude_sidefile;
        self.codex_app_server_enabled = config.provider_setup.codex_app_server;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the overlay. Resets the active tab to `Claude` and the
    /// scroll offset to `0` so the first-use experience is the same
    /// as a freshly-constructed overlay. Integration flags are synced
    /// from Settings by the caller before opening.
    pub fn open(&mut self) {
        self.open = true;
        self.tab = ProviderSetupTab::Claude;
        self.scroll_offset = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn switch_tab(&mut self, tab: ProviderSetupTab) {
        self.tab = tab;
        self.scroll_offset = 0;
    }

    pub fn next_tab(&mut self) {
        self.switch_tab(self.tab.next());
    }

    pub fn previous_tab(&mut self) {
        self.switch_tab(self.tab.previous());
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
pub const TMUX_QMONSTER_BUNDLE: &str =
    include_str!("provider_setup_snippets/tmux_qmonster_bundle.sh");

/// Compose the raw, copy-pasteable snippet text for the active tab —
/// used by the clipboard-copy action so operators don't have to mouse-
/// select rendered text out of the terminal (which would pick up
/// border characters, ANSI styling, and other artifacts).
///
/// Returns `(label, text)` where `label` is a short human-readable
/// surface name for the resulting toast notice. Honors the
/// Settings-synced integration flags — when
/// `claude_sidefile_enabled` is set, the addon block is appended to
/// the Claude statusline; when `codex_app_server_enabled` is set, the
/// app-server guide is appended to the Codex guidance. The Gemini auth
/// note is informational and is intentionally excluded from the
/// copied output (operator does not paste it anywhere).
pub fn snippet_for_tab(overlay: &ProviderSetupOverlay) -> (&'static str, String) {
    match overlay.tab {
        ProviderSetupTab::Claude => {
            let mut text = String::from(CLAUDE_STATUSLINE_BASE);
            if overlay.claude_sidefile_enabled {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(
                    "\n# --- Sidefile JSON export — paste IMMEDIATELY AFTER `input=$(cat)` ---\n",
                );
                text.push_str(CLAUDE_SIDEFILE_ADDON);
            }
            ("Claude statusline.sh", text)
        }
        ProviderSetupTab::Codex => {
            let mut text = String::from(CODEX_STATUSLINE_GUIDE);
            if overlay.codex_app_server_enabled {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str("\n# --- Codex App Server polling guide ---\n");
                text.push_str(CODEX_APP_SERVER_GUIDE);
            }
            ("Codex /statusline guide", text)
        }
        ProviderSetupTab::Gemini => (
            "Gemini ~/.gemini/settings.json",
            String::from(GEMINI_FOOTER_SETTINGS),
        ),
        ProviderSetupTab::Tmux => (
            "Qmonster tmux bundle installer",
            String::from(TMUX_QMONSTER_BUNDLE),
        ),
    }
}

fn section(out: &mut Vec<String>, title: &str) {
    if !out.is_empty() {
        out.push(String::new());
    }
    out.push(format!("--- {title} ---"));
}

fn detail_row(label: &str, value: impl AsRef<str>) -> String {
    format!("  {label:<34} {}", value.as_ref())
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "ON" } else { "OFF" }
}

fn yes_no(enabled: bool) -> &'static str {
    if enabled { "YES" } else { "NO" }
}

fn append_copy_contract(out: &mut Vec<String>, overlay: &ProviderSetupOverlay) {
    section(out, "Copy With y");
    match overlay.tab {
        ProviderSetupTab::Claude => {
            out.push(detail_row("Target", "~/.claude/statusline.sh"));
            out.push(detail_row("Content", "base statusline script"));
            out.push(detail_row(
                "Optional included",
                format!(
                    "Sidefile JSON export {} (from Settings)",
                    on_off(overlay.claude_sidefile_enabled)
                ),
            ));
            out.push(detail_row("Not copied", "Wiring section below"));
        }
        ProviderSetupTab::Codex => {
            out.push(detail_row("Target", "Codex setup guide"));
            out.push(detail_row("Content", "/statusline ON/OFF guidance"));
            out.push(detail_row(
                "Optional included",
                format!(
                    "Codex App Server polling guide {} (from Settings)",
                    on_off(overlay.codex_app_server_enabled)
                ),
            ));
        }
        ProviderSetupTab::Gemini => {
            out.push(detail_row("Target", "~/.gemini/settings.json"));
            out.push(detail_row("Content", "footer visibility JSON snippet"));
            out.push(detail_row(
                "Not copied",
                "/stats instructions and auth note below",
            ));
        }
        ProviderSetupTab::Tmux => {
            out.push(detail_row("Target", "~/ts.sh + ~/.tmux/qmonster.tmux.conf"));
            out.push(detail_row(
                "Content",
                "installer script that writes both recommended files",
            ));
            out.push(detail_row(
                "Not copied",
                "provider statusline/footer snippets",
            ));
        }
    }
}

fn append_copied_preview(out: &mut Vec<String>, overlay: &ProviderSetupOverlay) {
    let (label, text) = snippet_for_tab(overlay);
    section(out, "Preview: y copies this content");
    out.push(format!("# {label}"));
    for line in text.lines() {
        out.push(line.to_string());
    }
}

/// Compose the full text shown for the active tab, accounting for the
/// Settings-synced integration flags. Returns lines (Vec<String>) for
/// ratatui Paragraph rendering.
pub fn render_tab_content(
    overlay: &ProviderSetupOverlay,
    claude: &ClaudeState,
    codex: &CodexState,
    gemini: &GeminiFooterState,
) -> Vec<String> {
    let mut out = Vec::new();
    match overlay.tab {
        ProviderSetupTab::Claude => {
            section(&mut out, "Current Status");
            let statusline_state = if claude.statusline_script_present {
                format!("present ({} bytes)", claude.statusline_size_bytes)
            } else {
                "MISSING".into()
            };
            out.push(detail_row("~/.claude/statusline.sh", statusline_state));
            out.push(detail_row(
                "cache_read_input_tokens",
                yes_no(claude.exports_cache_read),
            ));
            out.push(detail_row(
                "cache_creation_input_tokens",
                yes_no(claude.exports_cache_creation),
            ));
            out.push(detail_row(
                "sidefile JSON export",
                yes_no(claude.sidefile_export_present),
            ));

            section(&mut out, "Settings");
            out.push(detail_row(
                "provider_setup.claude_sidefile",
                on_off(overlay.claude_sidefile_enabled),
            ));
            out.push("      Read-only here. Change in S Settings -> Integrations.".into());
            out.push("      y copies the Claude snippet using this Settings value.".into());

            append_copy_contract(&mut out, overlay);
            append_copied_preview(&mut out, overlay);

            section(&mut out, "Wiring (one-time, not copied by y)");
            out.push("Add to ~/.claude/settings.json:".into());
            out.push(r#"  "statusLine": {"#.into());
            out.push(r#"    "type": "command","#.into());
            out.push(r#"    "command": "$HOME/.claude/statusline.sh""#.into());
            out.push(r#"  }"#.into());
        }
        ProviderSetupTab::Codex => {
            section(&mut out, "Current Status");
            out.push(detail_row(
                "~/.codex/config.toml",
                if codex.config_present {
                    "present"
                } else {
                    "MISSING"
                },
            ));
            out.push(detail_row(
                "/statusline live toggles",
                "not detectable; run /statusline in pane",
            ));

            section(&mut out, "Settings");
            out.push(detail_row(
                "provider_setup.codex_app_server",
                on_off(overlay.codex_app_server_enabled),
            ));
            out.push("      Read-only here. Change in S Settings -> Integrations.".into());
            out.push(
                "      Restart Qmonster after saving ON; Qmonster auto-spawns app-server.".into(),
            );

            append_copy_contract(&mut out, overlay);
            append_copied_preview(&mut out, overlay);
        }
        ProviderSetupTab::Gemini => {
            section(&mut out, "Current Status");
            out.push(detail_row(
                "~/.gemini/settings.json",
                if gemini.settings_present {
                    "present"
                } else {
                    "MISSING"
                },
            ));
            out.push(detail_row("hideCWD", gemini.hide_cwd.to_string()));
            out.push(detail_row(
                "hideSandboxStatus",
                gemini.hide_sandbox_status.to_string(),
            ));
            out.push(detail_row(
                "hideModelInfo",
                gemini.hide_model_info.to_string(),
            ));
            out.push(detail_row(
                "hideContextPercentage",
                gemini.hide_context_percentage.to_string(),
            ));
            out.push(detail_row("hideFooter", gemini.hide_footer.to_string()));

            section(&mut out, "Settings");
            out.push("  No optional sections on this tab.".into());
            out.push("  Provider integration toggles live in S Settings -> Integrations.".into());

            append_copy_contract(&mut out, overlay);
            append_copied_preview(&mut out, overlay);

            section(
                &mut out,
                "/stats + /model periodic dispatch (not copied by y)",
            );
            out.push("When the selected Gemini pane is idle, `u` cycles `/model` →".into());
            out.push("`/stats session` → `/stats model`.".into());
            out.push(
                "While Gemini is active, `u` skips `/model` and cycles only the stats panels."
                    .into(),
            );
            out.push("`/stats tools` is not sent because Qmonster does not parse it today.".into());
            out.push("No additional setup needed — just keep using `u` periodically.".into());

            section(&mut out, "Auth note (informational, not copied by y)");
            for line in GEMINI_AUTH_NOTE.lines() {
                out.push(line.to_string());
            }
        }
        ProviderSetupTab::Tmux => {
            section(&mut out, "Purpose");
            out.push("  Recommended tmux launcher for the four-pane Qmonster workflow.".into());
            out.push(
                "  It creates/reattaches a tiled session and sets canonical pane titles.".into(),
            );

            section(&mut out, "Files Created By Copied Installer");
            out.push(detail_row(
                "~/ts.sh",
                "executable launcher; run ~/ts.sh <session-name> <directory>",
            ));
            out.push(detail_row(
                "~/.tmux/qmonster.tmux.conf",
                "tmux mouse/history/navigation/title helpers",
            ));

            section(&mut out, "After Running The Copied Installer");
            out.push("  tmux source-file ~/.tmux/qmonster.tmux.conf".into());
            out.push("  ~/ts.sh qmonster ~/Qmonster".into());

            section(&mut out, "Pane Titles");
            out.push(detail_row("0.0", "claude:1:main"));
            out.push(detail_row("0.1", "codex:1:review"));
            out.push(detail_row("0.2", "gemini:1:research"));
            out.push(detail_row("0.3", "qmonster:1:monitor"));

            append_copy_contract(&mut out, overlay);
            append_copied_preview(&mut out, overlay);
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
    fn sync_from_config_refreshes_read_only_integration_flags() {
        use crate::app::config::{ProviderSetupConfig, QmonsterConfig};
        let mut cfg = QmonsterConfig::defaults();
        cfg.provider_setup = ProviderSetupConfig {
            claude_sidefile: true,
            codex_app_server: false,
        };
        let mut overlay = ProviderSetupOverlay::default();

        overlay.sync_from_config(&cfg);
        assert!(overlay.claude_sidefile_enabled);
        assert!(!overlay.codex_app_server_enabled);

        cfg.provider_setup = ProviderSetupConfig {
            claude_sidefile: false,
            codex_app_server: true,
        };
        overlay.sync_from_config(&cfg);
        assert!(!overlay.claude_sidefile_enabled);
        assert!(overlay.codex_app_server_enabled);
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
        assert!(text.contains("Current Status"));
        assert!(text.contains("1718 bytes"));
        assert!(text.contains("Settings"));
        assert!(text.contains("provider_setup.claude_sidefile"));
        assert!(text.contains("Change in S Settings -> Integrations"));
        assert!(text.contains("Copy With y"));
        assert!(text.contains("Target"));
        assert!(text.contains("~/.claude/statusline.sh"));
        assert!(text.contains("Preview: y copies this content"));
        assert!(
            text.contains("Sidefile JSON export OFF (from Settings)"),
            "sidefile section should be hidden by default"
        );
        assert!(
            !text.contains("ai-cli-status/claude"),
            "sidefile addon must not be rendered by default"
        );
    }

    #[test]
    fn overlay_open_close_round_trip() {
        // Phase G-1 Task 2 invariant: `is_open()` must mirror the
        // private `open` field so the dashboard frame can decide
        // whether to render the modal and route keys to the overlay
        // handler. `open()` resets navigation state (tab + scroll)
        // so the first frame after a re-open is consistent with the
        // first frame after a fresh `new()`. Settings-synced flags are
        // not mutated by open/close.
        let mut overlay = ProviderSetupOverlay::new();
        assert!(!overlay.is_open(), "fresh overlay must start closed");

        // Pre-state mutation: switch tabs, scroll, mirror Settings.
        overlay.tab = ProviderSetupTab::Gemini;
        overlay.scroll_offset = 7;
        overlay.claude_sidefile_enabled = true;

        overlay.open();
        assert!(overlay.is_open(), "open() must set is_open()");
        assert_eq!(
            overlay.tab,
            ProviderSetupTab::Claude,
            "open() resets the active tab to Claude"
        );
        assert_eq!(overlay.scroll_offset, 0, "open() resets scroll_offset");
        assert!(
            overlay.claude_sidefile_enabled,
            "open() must NOT clobber Settings-synced flags"
        );

        overlay.close();
        assert!(!overlay.is_open(), "close() must clear is_open()");
        assert!(
            overlay.claude_sidefile_enabled,
            "close() must NOT clobber Settings-synced flags"
        );
    }

    #[test]
    fn from_config_seeds_read_only_flags_from_provider_setup_section() {
        // Phase G G-2 (v1.30.0): the overlay's read-only flags mirror
        // whatever the operator persisted in the [provider_setup]
        // config section, so a fresh `P` press shows the current
        // sidefile / app-server posture.
        use crate::app::config::{ProviderSetupConfig, QmonsterConfig};
        let mut cfg = QmonsterConfig::defaults();
        cfg.provider_setup = ProviderSetupConfig {
            claude_sidefile: true,
            codex_app_server: false,
        };
        let overlay = ProviderSetupOverlay::from_config(&cfg);
        assert!(overlay.claude_sidefile_enabled);
        assert!(!overlay.codex_app_server_enabled);
        assert!(!overlay.is_open(), "from_config must not auto-open");

        cfg.provider_setup = ProviderSetupConfig {
            claude_sidefile: false,
            codex_app_server: true,
        };
        let overlay = ProviderSetupOverlay::from_config(&cfg);
        assert!(!overlay.claude_sidefile_enabled);
        assert!(overlay.codex_app_server_enabled);
    }

    #[test]
    fn render_tab_content_claude_shows_sidefile_when_enabled_by_settings() {
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
        assert!(text.contains("Sidefile JSON export ON"));
        assert!(text.contains("ai-cli-status/claude"));
    }

    #[test]
    fn render_tab_content_codex_surfaces_settings_value_and_y_copy_target() {
        let overlay = ProviderSetupOverlay {
            tab: ProviderSetupTab::Codex,
            codex_app_server_enabled: true,
            ..Default::default()
        };
        let claude = ClaudeState {
            statusline_script_present: false,
            statusline_size_bytes: 0,
            exports_cache_read: false,
            exports_cache_creation: false,
            exports_input_tokens: false,
            sidefile_export_present: false,
        };
        let codex = CodexState {
            config_present: true,
            app_server_running: false,
        };
        let gemini = GeminiFooterState::default();
        let text = render_tab_content(&overlay, &claude, &codex, &gemini).join("\n");
        assert!(text.contains("Settings"));
        assert!(text.contains("provider_setup.codex_app_server"));
        assert!(text.contains("Change in S Settings -> Integrations"));
        assert!(text.contains("Restart Qmonster after saving ON; Qmonster auto-spawns app-server"));
        assert!(text.contains("Codex App Server polling guide ON (from Settings)"));
        assert!(text.contains("Copy With y"));
        assert!(text.contains("Target"));
        assert!(text.contains("Codex setup guide"));
        assert!(text.contains("Preview: y copies this content"));
        assert!(text.contains("codex app-server"));
    }

    #[test]
    fn render_tab_content_gemini_marks_non_copied_notes() {
        let overlay = ProviderSetupOverlay {
            tab: ProviderSetupTab::Gemini,
            ..Default::default()
        };
        let claude = ClaudeState {
            statusline_script_present: false,
            statusline_size_bytes: 0,
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
        let text = render_tab_content(&overlay, &claude, &codex, &gemini).join("\n");
        assert!(text.contains("No optional sections"));
        assert!(text.contains("Copy With y"));
        assert!(text.contains("Target"));
        assert!(text.contains("~/.gemini/settings.json"));
        assert!(text.contains("Not copied"));
        assert!(text.contains("/stats instructions and auth note below"));
        assert!(text.contains("/stats + /model periodic dispatch (not copied by y)"));
        assert!(text.contains("Auth note (informational, not copied by y)"));
    }

    #[test]
    fn render_tab_content_tmux_documents_targets_and_next_steps() {
        let overlay = ProviderSetupOverlay {
            tab: ProviderSetupTab::Tmux,
            ..Default::default()
        };
        let claude = ClaudeState {
            statusline_script_present: false,
            statusline_size_bytes: 0,
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
        let text = render_tab_content(&overlay, &claude, &codex, &gemini).join("\n");
        assert!(text.contains("Recommended tmux launcher"));
        assert!(text.contains("~/ts.sh"));
        assert!(text.contains("~/.tmux/qmonster.tmux.conf"));
        assert!(text.contains("tmux source-file ~/.tmux/qmonster.tmux.conf"));
        assert!(text.contains("~/ts.sh qmonster ~/Qmonster"));
        assert!(text.contains("claude:1:main"));
        assert!(text.contains("Preview: y copies this content"));
    }

    #[test]
    fn snippet_for_tab_claude_default_is_just_the_base_script() {
        let overlay = ProviderSetupOverlay::default();
        let (label, text) = snippet_for_tab(&overlay);
        assert_eq!(label, "Claude statusline.sh");
        assert!(text.starts_with("#!/usr/bin/env bash"));
        assert!(text.contains("cache_read"));
        assert!(
            !text.contains("ai-cli-status/claude"),
            "sidefile addon must not be included unless enabled in Settings"
        );
    }

    #[test]
    fn snippet_for_tab_claude_with_sidefile_appends_addon() {
        let overlay = ProviderSetupOverlay {
            claude_sidefile_enabled: true,
            ..Default::default()
        };
        let (_, text) = snippet_for_tab(&overlay);
        assert!(text.contains("#!/usr/bin/env bash"));
        assert!(text.contains("ai-cli-status/claude"));
        assert!(
            text.contains("paste IMMEDIATELY AFTER"),
            "addon must carry the placement-instruction comment so the copied output is self-explanatory"
        );
    }

    #[test]
    fn snippet_for_tab_codex_default_is_just_the_statusline_guide() {
        let overlay = ProviderSetupOverlay {
            tab: ProviderSetupTab::Codex,
            ..Default::default()
        };
        let (label, text) = snippet_for_tab(&overlay);
        assert_eq!(label, "Codex /statusline guide");
        assert!(text.contains("/statusline"));
        assert!(
            !text.contains("codex app-server"),
            "app-server guide must not be included unless enabled in Settings"
        );
    }

    #[test]
    fn snippet_for_tab_codex_with_app_server_appends_guide() {
        let overlay = ProviderSetupOverlay {
            tab: ProviderSetupTab::Codex,
            codex_app_server_enabled: true,
            ..Default::default()
        };
        let (_, text) = snippet_for_tab(&overlay);
        assert!(text.contains("/statusline"));
        assert!(text.contains("codex app-server"));
    }

    #[test]
    fn snippet_for_tab_gemini_returns_settings_json_only() {
        let overlay = ProviderSetupOverlay {
            tab: ProviderSetupTab::Gemini,
            ..Default::default()
        };
        let (label, text) = snippet_for_tab(&overlay);
        assert_eq!(label, "Gemini ~/.gemini/settings.json");
        assert!(text.contains("\"ui\""));
        assert!(text.contains("\"footer\""));
        assert!(
            !text.contains("OAuth"),
            "auth note is informational and must not be copied"
        );
    }

    #[test]
    fn snippet_for_tab_tmux_returns_installer_for_launcher_and_conf() {
        let overlay = ProviderSetupOverlay {
            tab: ProviderSetupTab::Tmux,
            ..Default::default()
        };
        let (label, text) = snippet_for_tab(&overlay);
        assert_eq!(label, "Qmonster tmux bundle installer");
        assert!(text.contains("cat > \"$HOME/ts.sh\""));
        assert!(text.contains("cat > \"$HOME/.tmux/qmonster.tmux.conf\""));
        assert!(text.contains("chmod 0755 \"$HOME/ts.sh\""));
        assert!(text.contains("claude:1:main"));
        assert!(text.contains("set -g mouse on"));
    }
}
