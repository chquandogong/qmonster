use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;

/// A value paired with the authority level of its source. The UI and
/// policy engine must never drop the `SourceKind` label (r2 rule).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricValue<T> {
    pub value: T,
    pub source_kind: SourceKind,
    pub confidence: Option<f32>,
    pub provider: Option<Provider>,
}

impl<T> MetricValue<T> {
    pub fn new(value: T, source_kind: SourceKind) -> Self {
        Self {
            value,
            source_kind,
            confidence: None,
            provider: None,
        }
    }

    pub fn with_confidence(mut self, c: f32) -> Self {
        self.confidence = Some(c);
        self
    }

    pub fn with_provider(mut self, p: Provider) -> Self {
        self.provider = Some(p);
        self
    }
}

/// Task-type inference from the tail (observation-only in Phase 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskType {
    #[default]
    Unknown,
    LogTriage,
    CodeExploration,
    Review,
    SessionResume,
    Summary,
    Automation,
}

/// Slice 4: unified halt-state signal. A single labeled classification
/// for the pane's idle cause. None means the pane is producing output /
/// not idle.
/// See `.docs/claude/Qmonster-v0.4.0-2026-04-25-claude-slice-4-*` for
/// the full taxonomy and detection priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IdleCause {
    /// Adapter detected an explicit interactive permission/approval ask.
    PermissionWait,
    /// Adapter detected an explicit waiting-for-user-input phrase.
    InputWait,
    /// Adapter detected provider-specific rate-limit-hit text.
    LimitHit,
    /// Adapter detected its provider's idle-cursor pattern at the
    /// last visible non-empty line — assistant has finished.
    WorkComplete,
    /// Stillness fallback: tail unchanged across last K polls; cause
    /// not classifiable from markers alone (Heuristic source).
    Stale,
}

/// Provider runtime/config facts observed from provider status/slash
/// output or readable provider-local configuration. These are
/// display-only. Absence means "not observed from an available source",
/// not "disabled".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFactKind {
    PermissionMode,
    AutoMode,
    Sandbox,
    AllowedDirectory,
    AgentConfig,
    LoadedTool,
    LoadedSkill,
    LoadedPlugin,
    RestrictedTool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeFact {
    pub kind: RuntimeFactKind,
    pub value: String,
    pub source_kind: SourceKind,
    pub confidence: Option<f32>,
    pub provider: Option<Provider>,
}

impl RuntimeFact {
    pub fn new(kind: RuntimeFactKind, value: impl Into<String>, source_kind: SourceKind) -> Self {
        Self {
            kind,
            value: value.into(),
            source_kind,
            confidence: None,
            provider: None,
        }
    }

    pub fn with_confidence(mut self, c: f32) -> Self {
        self.confidence = Some(c);
        self
    }

    pub fn with_provider(mut self, p: Provider) -> Self {
        self.provider = Some(p);
        self
    }
}

/// Boolean + metric signals extracted by an adapter from a pane tail.
/// Phase 1 treats `context_pressure` / `token_count` / `cost_usd` as
/// display-only; they never gate recommendations (Codex CS-2).
#[derive(Debug, Clone, Default)]
pub struct SignalSet {
    // v1.14.0: idle_state owns the markers; permission_prompt/waiting_for_input bools are gone.
    pub idle_state: Option<IdleCause>,
    pub log_storm: bool,
    pub repeated_output: bool,
    pub verbose_answer: bool,
    pub error_hint: bool,
    pub subagent_hint: bool,
    pub output_chars: usize,
    pub task_type: TaskType,
    pub context_pressure: Option<MetricValue<f32>>,
    /// S3-3: Gemini-specific quota usage from the status table's
    /// `quota` column. Independent from `context_pressure`. None on
    /// providers that do not expose quota or when the status table
    /// is absent / mis-aligned.
    pub quota_pressure: Option<MetricValue<f32>>,
    pub token_count: Option<MetricValue<u64>>,
    pub cost_usd: Option<MetricValue<f64>>,
    pub model_name: Option<MetricValue<String>>,
    pub git_branch: Option<MetricValue<String>>,
    pub worktree_path: Option<MetricValue<String>>,
    pub reasoning_effort: Option<MetricValue<String>>,
    pub runtime_facts: Vec<RuntimeFact>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::origin::SourceKind;

    #[test]
    fn metric_value_preserves_value_and_source() {
        let m = MetricValue::new(0.72_f32, SourceKind::ProviderOfficial);
        assert_eq!(m.value, 0.72);
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert!(m.confidence.is_none());
    }

    #[test]
    fn metric_value_with_confidence() {
        let m = MetricValue::new(1200_u64, SourceKind::Estimated).with_confidence(0.5);
        assert_eq!(m.confidence, Some(0.5));
    }

    #[test]
    fn default_signal_set_has_no_alerts() {
        let s = SignalSet::default();
        assert!(s.idle_state.is_none());
        assert!(!s.log_storm);
        assert!(!s.repeated_output);
        assert!(!s.verbose_answer);
        assert!(!s.error_hint);
        assert!(!s.subagent_hint);
    }

    #[test]
    fn default_signal_set_has_no_model_name() {
        let s = SignalSet::default();
        assert!(s.model_name.is_none());
    }

    #[test]
    fn signal_set_can_carry_model_name_with_source_kind() {
        let s = SignalSet {
            model_name: Some(
                MetricValue::new("gpt-5.4".to_string(), SourceKind::ProviderOfficial)
                    .with_provider(Provider::Codex),
            ),
            ..Default::default()
        };
        let m = s.model_name.as_ref().unwrap();
        assert_eq!(m.value, "gpt-5.4");
        assert_eq!(m.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(m.provider, Some(Provider::Codex));
    }

    #[test]
    fn default_signal_set_has_no_git_branch_or_worktree_or_effort() {
        let s = SignalSet::default();
        assert!(s.git_branch.is_none());
        assert!(s.worktree_path.is_none());
        assert!(s.reasoning_effort.is_none());
        assert!(s.runtime_facts.is_empty());
    }

    #[test]
    fn signal_set_can_carry_observability_fields() {
        let s = SignalSet {
            git_branch: Some(
                MetricValue::new("main".to_string(), SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Codex),
            ),
            worktree_path: Some(
                MetricValue::new("~/Qmonster".to_string(), SourceKind::ProviderOfficial)
                    .with_provider(Provider::Codex),
            ),
            reasoning_effort: Some(
                MetricValue::new("xhigh".to_string(), SourceKind::ProviderOfficial)
                    .with_confidence(0.6)
                    .with_provider(Provider::Codex),
            ),
            ..SignalSet::default()
        };
        assert_eq!(s.git_branch.as_ref().unwrap().value, "main");
        assert_eq!(s.worktree_path.as_ref().unwrap().value, "~/Qmonster");
        assert_eq!(s.reasoning_effort.as_ref().unwrap().value, "xhigh");
        assert_eq!(s.reasoning_effort.as_ref().unwrap().confidence, Some(0.6));
    }

    #[test]
    fn runtime_fact_carries_kind_value_source_and_provider() {
        let fact = RuntimeFact::new(
            RuntimeFactKind::PermissionMode,
            "bypass permissions on",
            SourceKind::ProviderOfficial,
        )
        .with_confidence(0.9)
        .with_provider(Provider::Claude);

        assert_eq!(fact.kind, RuntimeFactKind::PermissionMode);
        assert_eq!(fact.value, "bypass permissions on");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(fact.confidence, Some(0.9));
        assert_eq!(fact.provider, Some(Provider::Claude));
    }
}
