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

/// Boolean + metric signals extracted by an adapter from a pane tail.
/// Phase 1 treats `context_pressure` / `token_count` / `cost_usd` as
/// display-only; they never gate recommendations (Codex CS-2).
#[derive(Debug, Clone, Default)]
pub struct SignalSet {
    pub waiting_for_input: bool,
    pub permission_prompt: bool,
    pub log_storm: bool,
    pub repeated_output: bool,
    pub verbose_answer: bool,
    pub error_hint: bool,
    pub subagent_hint: bool,
    pub output_chars: usize,
    pub task_type: TaskType,
    pub context_pressure: Option<MetricValue<f32>>,
    pub token_count: Option<MetricValue<u64>>,
    pub cost_usd: Option<MetricValue<f64>>,
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
        assert!(!s.waiting_for_input);
        assert!(!s.permission_prompt);
        assert!(!s.log_storm);
        assert!(!s.repeated_output);
        assert!(!s.verbose_answer);
        assert!(!s.error_hint);
        assert!(!s.subagent_hint);
    }
}
