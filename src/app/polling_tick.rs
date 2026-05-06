use std::collections::HashMap;
use std::time::Instant;

use crate::app::bootstrap::Context;
use crate::app::dashboard_state::update_pane_state_flashes;
use crate::app::event_loop::{PaneReport, run_once_with_target};
use crate::app::system_notice::{
    SystemNotice, route_tmux_source_failure, route_tmux_source_recovered,
};
use crate::domain::signal::IdleCause;
use crate::notify::desktop::NotifyBackend;
use crate::tmux::polling::PaneSource;
use crate::tmux::types::WindowTarget;
use crate::ui::panels::PaneStateFlash;

pub struct PollTickState<'a> {
    pub last_source_error: &'a mut Option<String>,
    pub last_pane_idle_states: &'a mut HashMap<String, Option<IdleCause>>,
    pub pane_state_flashes: &'a mut HashMap<String, PaneStateFlash>,
}

pub struct PollTickOutcome {
    pub reports: Option<Vec<PaneReport>>,
    pub notice: Option<SystemNotice>,
    /// Phase H (v1.42.0): zero or more `SystemNotice`s produced by
    /// `maybe_auto_snapshot` during this tick. Forwarded to the
    /// dashboard by the TUI loop alongside `notice`.
    pub auto_notices: Vec<SystemNotice>,
    pub resync_dashboard: bool,
}

pub fn handle_poll_tick<P, N>(
    ctx: &mut Context<P, N>,
    now: Instant,
    selected_target: Option<&WindowTarget>,
    state: PollTickState<'_>,
) -> PollTickOutcome
where
    P: PaneSource,
    N: NotifyBackend,
{
    match run_once_with_target(ctx, now, selected_target) {
        Ok((reports, auto_notices)) => {
            let notice = route_tmux_source_recovered(state.last_source_error);
            update_pane_state_flashes(
                &reports,
                state.last_pane_idle_states,
                state.pane_state_flashes,
                now,
            );
            PollTickOutcome {
                reports: Some(reports),
                notice,
                auto_notices,
                resync_dashboard: true,
            }
        }
        Err(e) => {
            let notice = route_tmux_source_failure(state.last_source_error, e.to_string());
            let resync_dashboard = notice.is_some();
            PollTickOutcome {
                reports: None,
                notice,
                auto_notices: vec![],
                resync_dashboard,
            }
        }
    }
}
