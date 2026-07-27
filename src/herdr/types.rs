//! Serde types for herdr CLI JSON output (herdr 0.7.5, protocol 17).
//!
//! Posture toward a 0.x CLI: structs carry ONLY the fields Qmonster
//! consumes, everything is `#[serde(default)]`-tolerant, and unknown
//! fields are ignored (never `deny_unknown_fields`) so herdr adding
//! fields cannot break acquisition. The live envelope shape is
//! `{"id":"cli:<area>:<verb>","result":{<key>: …, "type": "…"}}`.

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// One pane row from `herdr pane list` / `pane get`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HerdrPane {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
}

/// One tab row from `herdr tab list`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HerdrTab {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: Option<u32>,
}

/// One workspace row from `herdr workspace list`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HerdrWorkspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: Option<u32>,
}

/// `herdr pane process-info --pane <id>` result body.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct HerdrProcessInfo {
    #[serde(default)]
    pub foreground_processes: Vec<HerdrProcess>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HerdrProcess {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Unwrap the herdr CLI envelope `{"id": "…", "result": {<key>: T, …}}`.
///
/// Errors are strings (not a new error enum) because every caller maps
/// them into the existing `PollingError` at the source boundary.
pub fn parse_envelope<T: DeserializeOwned>(raw: &str, key: &str) -> Result<T, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("herdr json parse: {e}"))?;
    let inner = value
        .get("result")
        .and_then(|r| r.get(key))
        .ok_or_else(|| format!("herdr envelope missing result.{key}"))?;
    serde_json::from_value(inner.clone()).map_err(|e| format!("herdr result.{key} shape: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Live-captured 2026-07-27, herdr 0.7.5 protocol 17. Trimmed to two
    // panes: one detected agent pane + one plain shell pane (agent
    // fields absent) so both serde paths are locked.
    const PANE_LIST_FIXTURE: &str = r#"{"id":"cli:pane:list","result":{"panes":[{"agent":"claude","agent_status":"idle","cwd":"/home/u/proj","focused":false,"foreground_cwd":"/home/u/proj","pane_id":"w1:p1","revision":5,"scroll":{"max_offset_from_bottom":0,"offset_from_bottom":0,"viewport_rows":32},"tab_id":"w1:t1","terminal_id":"term_a","terminal_title":"current session","terminal_title_stripped":"current session","workspace_id":"w1"},{"agent_status":"unknown","cwd":"/home/u/proj","focused":true,"foreground_cwd":"/home/u/proj","pane_id":"w1:p6","revision":1,"scroll":{"max_offset_from_bottom":0,"offset_from_bottom":0,"viewport_rows":32},"tab_id":"w1:t1","terminal_id":"term_b","terminal_title":"u@host: ~/proj","terminal_title_stripped":"u@host: ~/proj","workspace_id":"w1"}],"type":"pane_list"}}"#;

    const TAB_LIST_FIXTURE: &str = r#"{"id":"cli:tab:list","result":{"tabs":[{"agent_status":"idle","focused":false,"label":"1-claude","number":1,"pane_count":2,"tab_id":"w1:t1","workspace_id":"w1"}],"type":"tab_list"}}"#;

    const WORKSPACE_LIST_FIXTURE: &str = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"w1:t3","agent_status":"idle","focused":false,"label":"dogu-3d-studio","number":1,"pane_count":6,"tab_count":3,"workspace_id":"w1"}]}}"#;

    const PROCESS_INFO_FIXTURE: &str = r#"{"id":"cli:pane:process_info","result":{"process_info":{"foreground_process_group_id":905722,"foreground_processes":[{"argv":["claude","--flag"],"cmdline":"claude --flag","cwd":"/home/u/proj","name":"claude","pid":905722}]},"type":"pane_process_info"}}"#;

    #[test]
    fn parses_pane_list_with_and_without_agent_fields() {
        let panes: Vec<HerdrPane> = parse_envelope(PANE_LIST_FIXTURE, "panes").unwrap();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].pane_id, "w1:p1");
        assert_eq!(panes[0].agent.as_deref(), Some("claude"));
        assert_eq!(panes[0].workspace_id, "w1");
        assert_eq!(panes[0].tab_id, "w1:t1");
        assert_eq!(panes[0].foreground_cwd.as_deref(), Some("/home/u/proj"));
        assert!(!panes[0].focused);
        assert!(panes[1].agent.is_none());
        assert!(panes[1].focused);
        assert_eq!(
            panes[1].terminal_title_stripped.as_deref(),
            Some("u@host: ~/proj")
        );
    }

    #[test]
    fn parses_tab_and_workspace_lists_with_labels_and_numbers() {
        let tabs: Vec<HerdrTab> = parse_envelope(TAB_LIST_FIXTURE, "tabs").unwrap();
        assert_eq!(tabs[0].tab_id, "w1:t1");
        assert_eq!(tabs[0].label.as_deref(), Some("1-claude"));
        assert_eq!(tabs[0].number, Some(1));
        let ws: Vec<HerdrWorkspace> = parse_envelope(WORKSPACE_LIST_FIXTURE, "workspaces").unwrap();
        assert_eq!(ws[0].workspace_id, "w1");
        assert_eq!(ws[0].label.as_deref(), Some("dogu-3d-studio"));
        assert_eq!(ws[0].number, Some(1));
    }

    #[test]
    fn parses_process_info_first_foreground_process() {
        let info: HerdrProcessInfo = parse_envelope(PROCESS_INFO_FIXTURE, "process_info").unwrap();
        let first = info.foreground_processes.first().unwrap();
        assert_eq!(first.name.as_deref(), Some("claude"));
        assert_eq!(first.pid, Some(905722));
    }

    #[test]
    fn parse_envelope_reports_malformed_json_and_missing_key() {
        assert!(parse_envelope::<Vec<HerdrPane>>("not json", "panes").is_err());
        assert!(parse_envelope::<Vec<HerdrPane>>(PANE_LIST_FIXTURE, "nope").is_err());
    }

    #[test]
    fn pane_label_field_surfaces_when_renamed() {
        // `herdr pane rename <id> <label>` sets `label`; verified live
        // 2026-07-27 (a no-arg rename does NOT clear it; `rename <id> ""`
        // does). hs.sh relies on this to feed canonical titles.
        let raw = r#"{"id":"cli:pane:get","result":{"pane":{"agent_status":"unknown","cwd":"/x","focused":false,"label":"claude:1:main","pane_id":"w4:p4","revision":1,"tab_id":"w4:t1","terminal_id":"t","workspace_id":"w4"},"type":"pane_info"}}"#;
        let pane: HerdrPane = parse_envelope(raw, "pane").unwrap();
        assert_eq!(pane.label.as_deref(), Some("claude:1:main"));
    }
}
