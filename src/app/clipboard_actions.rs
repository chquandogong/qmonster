use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::widgets::ListState;

use crate::app::event_loop::PaneReport;
use crate::app::system_notice::SystemNotice;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::Severity;

pub struct AlertCommandCopyView<'a> {
    pub alert_state: &'a ListState,
    pub notices: &'a [SystemNotice],
    pub reports: &'a [PaneReport],
    pub fresh_alerts: &'a HashSet<String>,
    pub alert_times: &'a HashMap<String, String>,
    pub hidden_until: &'a HashMap<String, Instant>,
    pub now: Instant,
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| e.to_string())
}

pub fn copy_selected_alert_command_to_clipboard(view: AlertCommandCopyView<'_>) -> SystemNotice {
    copy_selected_alert_command(view, copy_text_to_clipboard)
}

/// v2.2.0 (P1-3): same as `copy_selected_alert_command_to_clipboard`
/// but also writes a `RecommendationOutcome::Copied` row to the
/// lifecycle ledger when the selected alert is a Recommendation-class
/// row AND the copy succeeded. Returns the same notice the legacy
/// helper does so the call site display path is unchanged.
///
/// Provider-facing rule: failures to write the ledger row do NOT alter
/// the user-visible notice. The clipboard copy is the actuation; the
/// ledger is the telemetry — they must not couple.
pub fn copy_selected_alert_command_to_clipboard_with_ledger(
    view: AlertCommandCopyView<'_>,
    sink: Option<&crate::store::SqliteRecommendationLifecycleSink>,
    now_unix_ms: i64,
) -> SystemNotice {
    copy_selected_alert_command_with_ledger(view, sink, now_unix_ms, copy_text_to_clipboard)
}

/// v2.2.0 (P1-3) testable inner: same contract as
/// `copy_selected_alert_command_to_clipboard_with_ledger` but takes the
/// clipboard write function as a closure. The public `..._to_clipboard_*`
/// wrapper passes `copy_text_to_clipboard`; tests inject a deterministic
/// closure so CI runners without a display server can still verify the
/// ledger write contract.
pub fn copy_selected_alert_command_with_ledger<F>(
    view: AlertCommandCopyView<'_>,
    sink: Option<&crate::store::SqliteRecommendationLifecycleSink>,
    now_unix_ms: i64,
    copy_text: F,
) -> SystemNotice
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let identity = crate::ui::alerts::selected_alert_recommendation_identity(
        view.alert_state,
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    let notice = copy_selected_alert_command(view, copy_text);
    if notice.title == "command copied"
        && let Some((pane_id, action)) = identity
        && let Some(sink) = sink
    {
        let record = crate::store::RecommendationOutcomeRecord {
            ts_unix_ms: now_unix_ms,
            recommendation_event_id: None,
            pane_id,
            action,
            outcome: crate::store::RecommendationOutcome::Copied,
            audit_event_id: None,
            summary: notice.body.clone(),
        };
        // P1-3 honesty rule: telemetry failure does not surface to the
        // operator. The clipboard copy already succeeded.
        let _ = sink.insert_recommendation_outcome(&record);
    }
    notice
}

/// v1.38 Phase D Task 20: lookup helper for the Action Explainer modal.
/// Mirrors what `copy_selected_alert_command` does internally but
/// returns `(alert_title, suggested_command, severity, source_kind)`
/// instead of writing to the clipboard. Used by tui_loop's `y` arm
/// to build a `build_copy_view` when `confirm_actions == Always`.
pub fn selected_alert_suggested_command(
    view: AlertCommandCopyView<'_>,
) -> Option<(String, String, Severity, SourceKind)> {
    crate::ui::alerts::selected_alert_suggested_command_meta(
        view.alert_state,
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    )
}

pub fn copy_selected_alert_command<F>(view: AlertCommandCopyView<'_>, copy_text: F) -> SystemNotice
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let command = crate::ui::alerts::selected_alert_suggested_command(
        view.alert_state,
        view.notices,
        view.reports,
        view.fresh_alerts,
        view.alert_times,
        view.hidden_until,
        view.now,
    );
    match command {
        Some(cmd) => match copy_text(&cmd) {
            Ok(()) => SystemNotice {
                title: "command copied".into(),
                body: format!("`{cmd}`"),
                severity: Severity::Good,
                source_kind: SourceKind::ProjectCanonical,
            },
            Err(e) => SystemNotice {
                title: "clipboard unavailable".into(),
                body: format!("could not copy command: {e}"),
                severity: Severity::Warning,
                source_kind: SourceKind::ProjectCanonical,
            },
        },
        None => SystemNotice {
            title: "no command selected".into(),
            body: "select an alert with a run command before pressing y".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::ProjectCanonical,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::Recommendation;
    use crate::domain::signal::SignalSet;
    use std::cell::RefCell;

    fn base_report(recs: Vec<Recommendation>) -> PaneReport {
        PaneReport {
            pane_id: "%1".into(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: recs,
            effects: vec![],
            dead: false,
            current_path: "/repo".into(),
            worktree_role: None,
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
            recent_token_samples: Vec::new(), // F-3: test fixture; production fetches via event_loop
            anomalies: vec![],
        }
    }

    fn recommendation_with_command(command: Option<&str>) -> Recommendation {
        Recommendation {
            action: "run command",
            reason: "operator can copy".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: command.map(str::to_string),
            side_effects: vec![],
            is_strong: false,
            next_step: None,
        }
    }

    fn view<'a>(
        state: &'a ListState,
        notices: &'a [SystemNotice],
        reports: &'a [PaneReport],
        fresh: &'a HashSet<String>,
        times: &'a HashMap<String, String>,
        hidden: &'a HashMap<String, Instant>,
    ) -> AlertCommandCopyView<'a> {
        AlertCommandCopyView {
            alert_state: state,
            notices,
            reports,
            fresh_alerts: fresh,
            alert_times: times,
            hidden_until: hidden,
            now: Instant::now(),
        }
    }

    #[test]
    fn copy_selected_alert_command_copies_run_command() {
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(Some(
            "cargo test",
        ))])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let copied = RefCell::new(String::new());

        let notice = copy_selected_alert_command(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            |text| {
                copied.replace(text.to_string());
                Ok(())
            },
        );

        assert_eq!(notice.title, "command copied");
        assert_eq!(copied.into_inner(), "cargo test");
    }

    #[test]
    fn copy_selected_alert_command_reports_clipboard_failure() {
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(Some(
            "cargo test",
        ))])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();

        let notice = copy_selected_alert_command(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            |_| Err("not available".into()),
        );

        assert_eq!(notice.title, "clipboard unavailable");
        assert_eq!(notice.severity, Severity::Warning);
    }

    #[test]
    fn copy_selected_alert_command_reports_missing_command() {
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(None)])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();

        let notice = copy_selected_alert_command(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            |_| Ok(()),
        );

        assert_eq!(notice.title, "no command selected");
        assert_eq!(notice.severity, Severity::Concern);
    }

    #[test]
    fn ledger_helper_writes_copied_outcome_on_successful_copy() {
        // v2.2.0 (P1-3): when the operator hits `y` on a recommendation-class
        // alert AND the clipboard write succeeds, the lifecycle ledger gains
        // a `RecommendationOutcome::Copied` row.
        //
        // v2.2.0 hotfix: use the generic `_with_ledger` variant with an
        // injected closure so CI runners without a display server (no
        // arboard backend) can still verify the contract.
        use crate::store::{RecommendationOutcome, SqliteRecommendationLifecycleSink};
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(Some(
            "cargo test",
        ))])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let sink = SqliteRecommendationLifecycleSink::open(&tmp.path().join("audit.db")).unwrap();

        let notice = copy_selected_alert_command_with_ledger(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            Some(&sink),
            1_700_000_000_000,
            |_text| Ok(()),
        );
        assert_eq!(notice.title, "command copied");

        // Read the outcome back via raw SQL — the sink doesn't expose a
        // public reader, but the test only needs to confirm presence.
        let count: i64 = sink
            .db_for_test()
            .connection()
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM recommendation_outcomes WHERE outcome = 'copied'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "exactly one Copied outcome must be persisted");
        let _ = RecommendationOutcome::Copied; // pin enum existence
    }

    #[test]
    fn ledger_helper_skips_outcome_when_copy_fails_or_no_sink() {
        // Two contracts pinned here:
        //   (a) clipboard write failure → no row, despite sink being present.
        //   (b) None sink + success → no panic, no row.
        use crate::store::SqliteRecommendationLifecycleSink;
        let mut state = ListState::default();
        state.select(Some(0));
        let reports = vec![base_report(vec![recommendation_with_command(Some(
            "cargo test",
        ))])];
        let fresh = HashSet::new();
        let times = HashMap::new();
        let hidden = HashMap::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let sink = SqliteRecommendationLifecycleSink::open(&tmp.path().join("audit.db")).unwrap();

        // (a) Failure path: clipboard refuses → no row written.
        let notice_fail = copy_selected_alert_command_with_ledger(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            Some(&sink),
            1_700_000_000_000,
            |_text| Err("no display server".into()),
        );
        assert_eq!(notice_fail.title, "clipboard unavailable");

        // (b) No-sink path with successful copy → no panic, no row.
        let notice_ok = copy_selected_alert_command_with_ledger(
            view(&state, &[], &reports, &fresh, &times, &hidden),
            None,
            1_700_000_000_000,
            |_text| Ok(()),
        );
        assert_eq!(notice_ok.title, "command copied");

        // Neither call wrote to the ledger.
        let count: i64 = sink
            .db_for_test()
            .connection()
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM recommendation_outcomes WHERE outcome = 'copied'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "no rows: copy failure suppresses ledger; None sink has nowhere to write"
        );
    }
}
