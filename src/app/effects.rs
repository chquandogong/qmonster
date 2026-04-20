use crate::app::config::{ActionsMode, QmonsterConfig};
use crate::domain::recommendation::RequestedEffect;

/// Enforces the actuation allow-list at the `app/` boundary. Phase 1
/// allows only `Notify` and `ArchiveLocal`. `SensitiveNotImplemented`
/// is never permitted — there is no code path that produces it, and
/// this runner double-denies so any future mistake is caught here.
#[derive(Debug, Clone)]
pub struct EffectRunner<'a> {
    cfg: &'a QmonsterConfig,
}

impl<'a> EffectRunner<'a> {
    pub fn new(cfg: &'a QmonsterConfig) -> Self {
        Self { cfg }
    }

    pub fn permit(&self, effect: RequestedEffect) -> bool {
        if self.cfg.actions.mode == ActionsMode::ObserveOnly {
            return false;
        }
        match effect {
            RequestedEffect::Notify => self.cfg.actions.allow_auto_notifications,
            RequestedEffect::ArchiveLocal => self.cfg.actions.allow_auto_archive,
            RequestedEffect::SensitiveNotImplemented => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::{ActionsConfig, ActionsMode, QmonsterConfig};
    use crate::domain::recommendation::RequestedEffect;

    fn cfg_with(mode: ActionsMode, prompt: bool, destructive: bool) -> QmonsterConfig {
        let mut c = QmonsterConfig::defaults();
        c.actions = ActionsConfig {
            mode,
            allow_auto_notifications: true,
            allow_auto_archive: true,
            allow_auto_prompt_send: prompt,
            allow_destructive_actions: destructive,
        };
        c
    }

    #[test]
    fn observe_only_rejects_everything() {
        let c = cfg_with(ActionsMode::ObserveOnly, false, false);
        let runner = EffectRunner::new(&c);
        assert!(!runner.permit(RequestedEffect::Notify));
        assert!(!runner.permit(RequestedEffect::ArchiveLocal));
        assert!(!runner.permit(RequestedEffect::SensitiveNotImplemented));
    }

    #[test]
    fn recommend_only_permits_notify_and_archive_only() {
        let c = cfg_with(ActionsMode::RecommendOnly, false, false);
        let runner = EffectRunner::new(&c);
        assert!(runner.permit(RequestedEffect::Notify));
        assert!(runner.permit(RequestedEffect::ArchiveLocal));
        assert!(!runner.permit(RequestedEffect::SensitiveNotImplemented));
    }

    #[test]
    fn allow_flags_can_still_suppress_even_in_recommend_only() {
        let mut c = cfg_with(ActionsMode::RecommendOnly, false, false);
        c.actions.allow_auto_notifications = false;
        let runner = EffectRunner::new(&c);
        assert!(!runner.permit(RequestedEffect::Notify));
    }

    #[test]
    fn sensitive_effects_never_permitted_in_phase_1() {
        // Even if a future config tried to enable it, the Phase-1 runner
        // treats SensitiveNotImplemented as always-denied.
        let mut c = cfg_with(ActionsMode::RecommendOnly, true, true);
        c.actions.allow_auto_prompt_send = true;
        c.actions.allow_destructive_actions = true;
        let runner = EffectRunner::new(&c);
        assert!(!runner.permit(RequestedEffect::SensitiveNotImplemented));
    }
}
