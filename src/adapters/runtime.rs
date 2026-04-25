use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{RuntimeFact, RuntimeFactKind};

pub(crate) fn push_provider_fact(
    facts: &mut Vec<RuntimeFact>,
    provider: Provider,
    kind: RuntimeFactKind,
    value: impl AsRef<str>,
    confidence: f32,
) {
    let value = clean_runtime_value(value.as_ref());
    if value.is_empty() {
        return;
    }

    let fact = RuntimeFact::new(kind, value, SourceKind::ProviderOfficial)
        .with_confidence(confidence)
        .with_provider(provider);
    if !facts.iter().any(|existing| {
        existing.kind == fact.kind
            && existing.provider == fact.provider
            && existing.value.eq_ignore_ascii_case(&fact.value)
    }) {
        facts.push(fact);
    }
}

pub(crate) fn clean_runtime_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('│')
        .trim()
        .trim_matches(|c: char| matches!(c, '`' | '"' | '\''))
        .trim()
        .to_string()
}

pub(crate) fn split_runtime_list(value: &str) -> Vec<String> {
    let cleaned = clean_runtime_value(value);
    if cleaned.is_empty() {
        return Vec::new();
    }
    if !cleaned.contains(',') && !cleaned.contains(';') {
        return vec![cleaned];
    }
    cleaned
        .split([',', ';'])
        .map(clean_runtime_value)
        .filter(|v| !v.is_empty())
        .collect()
}
