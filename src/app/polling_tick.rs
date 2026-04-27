use std::collections::HashMap;
use std::time::Instant;

use crate::app::bootstrap::Context;
use crate::app::dashboard_state::update_pane_state_flashes;
use crate::app::event_loop::{PaneReport, run_once_with_target};
use crate::app::system_notice::{SystemNotice, route_polling_failure, route_polling_recovered};
use crate::domain::signal::IdleCause;
use crate::notify::desktop::NotifyBackend;
use crate::tmux::polling::PaneSource;
use crate::tmux::types::WindowTarget;
use crate::ui::panels::PaneStateFlash;

pub struct PollTickState<'a> {
    pub last_poll_error: &'a mut Option<String>,
    pub last_pane_idle_states: &'a mut HashMap<String, Option<IdleCause>>,
    pub pane_state_flashes: &'a mut HashMap<String, PaneStateFlash>,
}

pub struct PollTickOutcome {
    pub reports: Option<Vec<PaneReport>>,
    pub notice: Option<SystemNotice>,
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
        Ok(reports) => {
            let notice = route_polling_recovered(state.last_poll_error);
            update_pane_state_flashes(
                &reports,
                state.last_pane_idle_states,
                state.pane_state_flashes,
                now,
            );
            PollTickOutcome {
                reports: Some(reports),
                notice,
                resync_dashboard: true,
            }
        }
        Err(e) => {
            let notice = route_polling_failure(state.last_poll_error, e.to_string());
            let resync_dashboard = notice.is_some();
            PollTickOutcome {
                reports: None,
                notice,
                resync_dashboard,
            }
        }
    }
}
