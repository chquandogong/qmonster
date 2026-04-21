use crate::app::config::{ActionsMode, QmonsterConfig};
use crate::domain::recommendation::RequestedEffect;

/// Enforces the actuation allow-list at the `app/` boundary. Phase 1
/// allowed only `Notify` and `ArchiveLocal`. Phase 5 P5-1 (v1.9.0) adds
/// `PromptSendProposed` as an allow-list-passing *display* effect — the
/// runner lets the proposal surface to the UI in `recommend_only` but
/// `observe_only` still blocks it, mirroring the existing gate. Actual
/// `tmux send-keys` execution is NOT part of P5-1; it lands in P5-2+ on
/// a separate method that inspects `allow_auto_prompt_send` and requires
/// explicit operator confirmation. `SensitiveNotImplemented` is still
/// unconditionally denied — double-denying so any future code-path slip
/// is caught at this boundary.
#[derive(Debug, Clone)]
pub struct EffectRunner<'a> {
    cfg: &'a QmonsterConfig,
}

impl<'a> EffectRunner<'a> {
    pub fn new(cfg: &'a QmonsterConfig) -> Self {
        Self { cfg }
    }

    pub fn permit(&self, effect: &RequestedEffect) -> bool {
        if self.cfg.actions.mode == ActionsMode::ObserveOnly {
            return false;
        }
        match effect {
            RequestedEffect::Notify => self.cfg.actions.allow_auto_notifications,
            RequestedEffect::ArchiveLocal => self.cfg.actions.allow_auto_archive,
            // P5-1 contract: the proposal is a display-layer effect, not
            // an actuation. The runner lets it through so the UI can
            // render a pending send; `allow_auto_prompt_send` continues
            // to gate the real tmux send-keys call that a later slice
            // adds on top of this proposal.
            RequestedEffect::PromptSendProposed { .. } => true,
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

    fn proposal_sample() -> RequestedEffect {
        RequestedEffect::PromptSendProposed {
            target_pane_id: "%1".into(),
            slash_command: "/compact".into(),
            proposal_id: "%1:/compact".into(),
        }
    }

    #[test]
    fn observe_only_rejects_everything() {
        let c = cfg_with(ActionsMode::ObserveOnly, false, false);
        let runner = EffectRunner::new(&c);
        assert!(!runner.permit(&RequestedEffect::Notify));
        assert!(!runner.permit(&RequestedEffect::ArchiveLocal));
        assert!(
            !runner.permit(&proposal_sample()),
            "P5-1: observe_only blocks even display-layer proposals"
        );
        assert!(!runner.permit(&RequestedEffect::SensitiveNotImplemented));
    }

    #[test]
    fn recommend_only_permits_notify_archive_and_prompt_send_proposal() {
        let c = cfg_with(ActionsMode::RecommendOnly, false, false);
        let runner = EffectRunner::new(&c);
        assert!(runner.permit(&RequestedEffect::Notify));
        assert!(runner.permit(&RequestedEffect::ArchiveLocal));
        assert!(
            runner.permit(&proposal_sample()),
            "P5-1: prompt-send *proposals* pass the gate so UI can surface them (allow_auto_prompt_send still gates real execution, which lives in a later slice)"
        );
        assert!(!runner.permit(&RequestedEffect::SensitiveNotImplemented));
    }

    #[test]
    fn allow_flags_can_still_suppress_even_in_recommend_only() {
        let mut c = cfg_with(ActionsMode::RecommendOnly, false, false);
        c.actions.allow_auto_notifications = false;
        let runner = EffectRunner::new(&c);
        assert!(!runner.permit(&RequestedEffect::Notify));
    }

    #[test]
    fn prompt_send_proposal_passes_regardless_of_allow_auto_prompt_send() {
        // P5-1 contract: `allow_auto_prompt_send` gates the *execution*
        // surface (P5-2+). The *proposal* passes the allow-list so the
        // UI can surface it either way — the actuation check happens
        // downstream, not here. This test pins that separation.
        let c_off = cfg_with(ActionsMode::RecommendOnly, /*prompt*/ false, false);
        let c_on = cfg_with(ActionsMode::RecommendOnly, /*prompt*/ true, false);
        assert!(EffectRunner::new(&c_off).permit(&proposal_sample()));
        assert!(EffectRunner::new(&c_on).permit(&proposal_sample()));
    }

    #[test]
    fn sensitive_effects_never_permitted() {
        // Even if a future config tried to enable it, the runner treats
        // SensitiveNotImplemented as always-denied — guardrail against a
        // rule slipping through without an explicit allow-list path.
        let mut c = cfg_with(ActionsMode::RecommendOnly, true, true);
        c.actions.allow_auto_prompt_send = true;
        c.actions.allow_destructive_actions = true;
        let runner = EffectRunner::new(&c);
        assert!(!runner.permit(&RequestedEffect::SensitiveNotImplemented));
    }
}
