use crate::domain::origin::SourceKind;
use crate::domain::profile::ProviderProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Safe,
    Good,
    Concern,
    Warning,
    Risk,
}

impl Severity {
    pub fn letter(self) -> &'static str {
        match self {
            Severity::Safe => "S",
            Severity::Good => "G",
            Severity::Concern => "C",
            Severity::Warning => "W",
            Severity::Risk => "R",
        }
    }
}

/// Advisory recommendation surfaced to the UI and (optionally) as an
/// audit event. Pure data — produced by `policy/` and consumed by
/// `app/` and `ui/`.
#[derive(Debug, Clone)]
pub struct Recommendation {
    pub action: &'static str,
    pub reason: String,
    pub severity: Severity,
    pub source_kind: SourceKind,
    /// Executable command intended for copy-paste: shell invocation,
    /// in-pane slash-command, or `# config-edit …` comment pointer. Must
    /// be runnable or copy-pastable on a single surface; mixed-surface
    /// prose belongs in `next_step` instead. Renderers prefix this with
    /// `run:` in the UI and `--once` output.
    pub suggested_command: Option<String>,
    pub side_effects: Vec<String>,
    /// G-7: if true, this recommendation is rendered in a dedicated
    /// "CHECKPOINT" alert kind in the alert queue and `--once` output.
    pub is_strong: bool,
    /// Codex v1.7.3 (phase3b-strong-rec cleanup): prose operator-facing
    /// precondition/hint that precedes the runnable
    /// `suggested_command`. Used for strong recs whose safe execution
    /// requires a step (e.g. TUI key `s` to snapshot first) that
    /// cannot be represented as a command on the same surface.
    /// Renderers print this as `next: …` before `run: …`.
    pub next_step: Option<String>,
    /// Codex v1.8.1 (Phase 4 P4-1 remediation): structured provider-
    /// profile payload. `Some(_)` only when the rec recommends a
    /// `ProviderProfile` (from `src/policy/rules/profiles.rs`); in
    /// that case the renderer surfaces each lever's
    /// key/value/citation/SourceKind so the `ProjectCanonical` bundle
    /// vs `ProviderOfficial` lever authority split is visible end-
    /// to-end rather than collapsed into `reason` prose.
    pub profile: Option<ProviderProfile>,
}

/// Effects the policy engine wants `app::EffectRunner` to consider.
/// The runner decides which actually fire based on `actions.mode` and
/// the allow-list. Authority-sensitive ordering is exposed via
/// `authority_tier()` — derived `Ord` was dropped in Phase 5 P5-1
/// (v1.9.0) when `PromptSendProposed` added a payload that cannot
/// cheaply participate in a total order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestedEffect {
    Notify,
    ArchiveLocal,
    /// Phase 5 P5-1 (v1.9.0): operator-facing prompt-send proposal.
    /// A rule builds this as a *proposal*; `app::EffectRunner` allows
    /// the proposal to pass the allow-list (`recommend_only` → true,
    /// `observe_only` → false) so the UI can render it as a pending
    /// actuation. Actual execution via `tmux send-keys` lands in P5-2+
    /// and stays gated behind explicit operator confirmation plus the
    /// `allow_auto_prompt_send` flag (safety-precedence asymmetry —
    /// env/CLI cannot promote that flag toward more permissive).
    PromptSendProposed {
        target_pane_id: String,
        slash_command: String,
    },
    /// Reserved — no current code path creates this; placeholder for the
    /// future destructive-effect allow-list gate. Never executed.
    SensitiveNotImplemented,
}

impl RequestedEffect {
    /// Authority tier for allow-list ordering. Higher = more sensitive.
    /// Replaces the Phase-1 `Ord` derive that was lost when
    /// `PromptSendProposed` added a payload in P5-1 (v1.9.0).
    pub fn authority_tier(&self) -> u8 {
        match self {
            RequestedEffect::Notify => 0,
            RequestedEffect::ArchiveLocal => 1,
            RequestedEffect::PromptSendProposed { .. } => 2,
            RequestedEffect::SensitiveNotImplemented => 3,
        }
    }
}

/// Cross-pane advisory. Emitted by `policy::Engine::evaluate_cross_pane`
/// when a rule observes overlap/concurrency across two or more panes.
/// Rendered in the alert queue alongside `SystemNotice`.
#[derive(Debug, Clone)]
pub struct CrossPaneFinding {
    pub kind: CrossPaneKind,
    pub anchor_pane_id: String,
    pub other_pane_ids: Vec<String>,
    pub reason: String,
    pub severity: Severity,
    pub source_kind: SourceKind,
    pub suggested_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossPaneKind {
    /// Gemini G-11: two or more panes producing output in the same
    /// working directory — risk of divergent edits.
    ConcurrentMutatingWork,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;

    #[test]
    fn severity_ordering_safe_lt_risk() {
        assert!(Severity::Safe < Severity::Good);
        assert!(Severity::Good < Severity::Concern);
        assert!(Severity::Concern < Severity::Warning);
        assert!(Severity::Warning < Severity::Risk);
    }

    #[test]
    fn recommendation_carries_source_kind_and_reason() {
        let r = Recommendation {
            action: "raw archive + preview",
            reason: "log storm detected".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: None,
            profile: None,
        };
        assert_eq!(r.severity, Severity::Warning);
        assert_eq!(r.source_kind, SourceKind::Heuristic);
    }

    #[test]
    fn requested_effect_authority_tier_preserves_allow_list_gradient() {
        // Gradient: notify < archive < prompt_send_proposed < sensitive.
        // The allow-list gate in app::EffectRunner + future P5-2 actuation
        // path use authority_tier() in place of the former `Ord` derive
        // (dropped when PromptSendProposed gained a payload in P5-1).
        let proposal = RequestedEffect::PromptSendProposed {
            target_pane_id: "%1".into(),
            slash_command: "/compact".into(),
        };
        assert!(
            RequestedEffect::Notify.authority_tier()
                < RequestedEffect::ArchiveLocal.authority_tier()
        );
        assert!(RequestedEffect::ArchiveLocal.authority_tier() < proposal.authority_tier());
        assert!(
            proposal.authority_tier() < RequestedEffect::SensitiveNotImplemented.authority_tier()
        );
    }

    #[test]
    fn prompt_send_proposed_carries_target_pane_and_slash_command() {
        // P5-1 data-shape contract: the proposal carries exactly the
        // target pane id + the slash command operator needs to confirm.
        // No raw tail, no free-form payload — this is metadata, by design
        // (audit-isolation rule stays intact; the audit log never needs
        // raw bytes from this effect).
        let p = RequestedEffect::PromptSendProposed {
            target_pane_id: "%7".into(),
            slash_command: "/compact".into(),
        };
        match &p {
            RequestedEffect::PromptSendProposed {
                target_pane_id,
                slash_command,
            } => {
                assert_eq!(target_pane_id, "%7");
                assert_eq!(slash_command, "/compact");
            }
            _ => panic!("expected PromptSendProposed"),
        }
        // Equality holds structurally (no Copy / Hash required).
        assert_eq!(p, p.clone());
    }

    #[test]
    fn cross_pane_finding_carries_anchor_and_others() {
        let f = CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentMutatingWork,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec!["%2".into(), "%3".into()],
            reason: "concurrent work on /repo".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: None,
        };
        assert_eq!(f.kind, CrossPaneKind::ConcurrentMutatingWork);
        assert_eq!(f.anchor_pane_id, "%1");
        assert_eq!(f.other_pane_ids.len(), 2);
        assert_eq!(f.severity, Severity::Warning);
    }
}
