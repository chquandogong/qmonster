//! Phase D D2 (v1.18.0): identity-drift anomaly detection.
//!
//! Compares a pane's previously observed `Provider` and `current_path`
//! against the current snapshot and emits a passive `Severity::Concern`
//! recommendation when either has changed. Pure function; the caller
//! (event loop) owns the per-session history map and the dedup
//! hashset that prevents the same drift from re-firing every poll.
//!
//! Gated behind `PolicyGates.identity_drift_findings` (operator opts in
//! via `[security] identity_drift_findings = true`). Default is off
//! because routine CLI swaps inside the same pane are common operator
//! behavior, not anomalies — surfacing them by default would be noise.

use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::policy::gates::PolicyGates;

/// Snapshot of the operator-visible identity facts that drift detection
/// cares about. Built upstream by `app::event_loop` from each pane's
/// most recent `ResolvedIdentity` + `current_path`. Tiny by design so
/// the per-session history map can keep one entry per pane_id without
/// cloning expensive trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentitySnapshot {
    pub provider: Provider,
    pub current_path: String,
}

/// One identity-drift finding. Pairs the operator-visible
/// `Recommendation` with a stable dedup key so the caller can suppress
/// repeat firings without re-parsing reason text. Dedup keys are
/// formatted as `"<kind>:<from>→<to>"` (e.g.
/// `"provider:Claude→Codex"`, `"worktree:/repo-a→/repo-b"`); the
/// caller pairs them with the pane id when storing.
#[derive(Debug, Clone)]
pub struct DriftFinding {
    pub recommendation: Recommendation,
    pub dedup_key: String,
}

/// Pure detection: if the operator-visible identity has drifted from
/// `prev` to `current`, return the recommendations the caller should
/// emit (subject to the caller's dedup table).
///
/// Returns up to two `DriftFinding`s — one for provider drift and one
/// for worktree-path drift — so the operator can see both transitions
/// even when they happen in the same poll.
pub fn detect_identity_drift(
    pane_id: &str,
    prev: &IdentitySnapshot,
    current: &IdentitySnapshot,
    gates: &PolicyGates,
) -> Vec<DriftFinding> {
    let mut out = Vec::new();
    if !gates.identity_drift_findings {
        return out;
    }

    if prev.provider != current.provider && prev.provider != Provider::Unknown {
        let from = provider_label(prev.provider);
        let to = provider_label(current.provider);
        out.push(DriftFinding {
            dedup_key: format!("provider:{from}→{to}"),
            recommendation: Recommendation {
                action: "identity-drift: provider changed",
                reason: format!(
                    "pane {pane_id} provider drift: {from} → {to} — operator swapped CLI mid-pane; verify intent and re-run any per-provider checks",
                ),
                severity: Severity::Concern,
                source_kind: SourceKind::Estimated,
                suggested_command: None,
                side_effects: vec![],
                is_strong: false,
                next_step: None,
                profile: None,
            },
        });
    }

    if prev.current_path != current.current_path
        && !prev.current_path.is_empty()
        && !current.current_path.is_empty()
    {
        let from = &prev.current_path;
        let to = &current.current_path;
        out.push(DriftFinding {
            dedup_key: format!("worktree:{from}→{to}"),
            recommendation: Recommendation {
                action: "identity-drift: worktree changed",
                reason: format!(
                    "pane {pane_id} worktree drift: {from} → {to} — pane changed working directory; re-confirm the active project context",
                ),
                severity: Severity::Concern,
                source_kind: SourceKind::Estimated,
                suggested_command: None,
                side_effects: vec![],
                is_strong: false,
                next_step: None,
                profile: None,
            },
        });
    }

    out
}

/// Stable label for a `Provider` used in drift reason text. Matches the
/// canonical `{provider}:{instance}:{role}` title vocabulary so the
/// operator sees the same name they use everywhere else.
fn provider_label(p: Provider) -> &'static str {
    match p {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Qmonster => "Qmonster",
        Provider::Unknown => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gates_with_drift(enabled: bool) -> PolicyGates {
        PolicyGates {
            identity_drift_findings: enabled,
            ..PolicyGates::default()
        }
    }

    #[test]
    fn provider_drift_emits_concern_when_gate_enabled() {
        let prev = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/home/op/repo".into(),
        };
        let current = IdentitySnapshot {
            provider: Provider::Codex,
            current_path: "/home/op/repo".into(),
        };
        let findings = detect_identity_drift("%1", &prev, &current, &gates_with_drift(true));
        assert_eq!(
            findings.len(),
            1,
            "provider-only drift produces exactly one finding"
        );
        let f = &findings[0];
        let rec = &f.recommendation;
        assert_eq!(rec.severity, Severity::Concern);
        assert_eq!(rec.action, "identity-drift: provider changed");
        assert_eq!(rec.source_kind, SourceKind::Estimated);
        assert!(
            rec.reason.contains("Claude") && rec.reason.contains("Codex"),
            "reason must name both endpoints: {:?}",
            rec.reason
        );
        assert!(
            rec.reason.contains("%1"),
            "reason must name the pane: {:?}",
            rec.reason
        );
        assert_eq!(
            f.dedup_key, "provider:Claude→Codex",
            "dedup key locks (kind, from→to) so the same drift fires once per session"
        );
    }

    #[test]
    fn worktree_drift_emits_concern_when_gate_enabled() {
        let prev = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/home/op/repo-a".into(),
        };
        let current = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/home/op/repo-b".into(),
        };
        let findings = detect_identity_drift("%2", &prev, &current, &gates_with_drift(true));
        assert_eq!(
            findings.len(),
            1,
            "worktree-only drift produces exactly one finding"
        );
        let f = &findings[0];
        assert_eq!(f.recommendation.action, "identity-drift: worktree changed");
        assert_eq!(f.recommendation.severity, Severity::Concern);
        assert!(
            f.recommendation.reason.contains("repo-a")
                && f.recommendation.reason.contains("repo-b"),
            "reason must name both paths: {:?}",
            f.recommendation.reason
        );
        assert_eq!(f.dedup_key, "worktree:/home/op/repo-a→/home/op/repo-b");
    }

    #[test]
    fn simultaneous_provider_and_worktree_drift_produces_two_findings() {
        // An operator who closes Claude in /repo and opens Codex in
        // /other-repo trips both rules in one poll. Both findings fire
        // so the operator can see the full transition; the dedup table
        // upstream still keys on (pane, kind, from→to) so they only
        // surface once each.
        let prev = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/home/op/repo".into(),
        };
        let current = IdentitySnapshot {
            provider: Provider::Codex,
            current_path: "/home/op/other-repo".into(),
        };
        let findings = detect_identity_drift("%3", &prev, &current, &gates_with_drift(true));
        assert_eq!(findings.len(), 2);
        let actions: Vec<_> = findings.iter().map(|f| f.recommendation.action).collect();
        assert!(actions.contains(&"identity-drift: provider changed"));
        assert!(actions.contains(&"identity-drift: worktree changed"));
    }

    #[test]
    fn drift_stays_silent_when_gate_disabled() {
        let prev = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/repo".into(),
        };
        let current = IdentitySnapshot {
            provider: Provider::Codex,
            current_path: "/other".into(),
        };
        let findings = detect_identity_drift("%1", &prev, &current, &gates_with_drift(false));
        assert!(
            findings.is_empty(),
            "drift detection is opt-in; default config must keep panes silent"
        );
    }

    #[test]
    fn no_drift_emits_nothing() {
        let snap = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/repo".into(),
        };
        let recs = detect_identity_drift("%1", &snap, &snap, &gates_with_drift(true));
        assert!(recs.is_empty());
    }

    #[test]
    fn drift_from_unknown_provider_does_not_fire() {
        // The resolver returns Provider::Unknown when it cannot tell
        // what the pane is running. A first-confirmed identification
        // (Unknown → Claude) is not a drift the operator caused — it's
        // Qmonster catching up — so suppress that direction.
        let prev = IdentitySnapshot {
            provider: Provider::Unknown,
            current_path: "/repo".into(),
        };
        let current = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/repo".into(),
        };
        let recs = detect_identity_drift("%1", &prev, &current, &gates_with_drift(true));
        assert!(
            recs.is_empty(),
            "Unknown→known is identity catch-up, not drift"
        );
    }

    #[test]
    fn drift_with_empty_paths_does_not_emit_worktree_finding() {
        // `current_path` may be empty when tmux returns no path (e.g.
        // a freshly spawned pane). Treat that as missing, not a drift.
        let prev = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "".into(),
        };
        let current = IdentitySnapshot {
            provider: Provider::Claude,
            current_path: "/repo".into(),
        };
        let recs = detect_identity_drift("%1", &prev, &current, &gates_with_drift(true));
        assert!(
            recs.is_empty(),
            "empty→present path is initial sighting, not drift"
        );
    }
}
