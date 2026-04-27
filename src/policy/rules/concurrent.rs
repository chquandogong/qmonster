use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{CrossPaneFinding, CrossPaneKind, Severity};
use crate::domain::signal::IdleCause;
use crate::policy::engine::PaneView;
use crate::policy::gates::PolicyGates;

/// Canonical contract (`docs/ai/VALIDATION.md:95-96`): concurrent-work
/// warning fires when two active panes touch the same file or git
/// branch. v1.15.23 narrows the earlier project-path proxy: panes must
/// now expose the same `current_path` + `git_branch` before a finding
/// fires. File-level detection remains deferred until providers expose a
/// trustworthy active-file signal.
///
/// Phase D D1 (v1.17.0) adds an opt-in cross-window split: when
/// `gates.cross_window_findings` is `true`, qualifying groups whose
/// panes live in two or more distinct `window_label`s emit a
/// `CrossWindowConcurrentWork` finding instead of the default
/// `ConcurrentMutatingWork` one. Same-window groups still emit
/// `ConcurrentMutatingWork` regardless of the gate; the cross-window
/// path is gated because operators legitimately keep the same repo
/// open in a scratch window next to a main implementation window.
pub fn eval_concurrent(panes: &[PaneView<'_>], gates: &PolicyGates) -> Vec<CrossPaneFinding> {
    use crate::domain::identity::Role;

    let qualifying: Vec<(&PaneView<'_>, ConcurrentKey, String)> = panes
        .iter()
        .filter(|v| matches!(v.identity.identity.role, Role::Main | Role::Review))
        .filter(|v| {
            !matches!(
                v.signals.idle_state,
                Some(IdleCause::InputWait) | Some(IdleCause::PermissionWait)
            )
        })
        .filter(|v| v.signals.output_chars >= 500)
        .filter_map(|v| concurrent_key(v).map(|key| (v, key, v.identity.identity.pane_id.clone())))
        .collect();

    // Group by path+branch; emit at most one finding per group.
    let mut out = Vec::new();
    let mut seen_keys: Vec<ConcurrentKey> = Vec::new();
    for (_, key, _) in qualifying.iter() {
        if seen_keys.contains(key) {
            continue;
        }
        seen_keys.push(key.clone());

        let mut same_key: Vec<&(&PaneView<'_>, ConcurrentKey, String)> = qualifying
            .iter()
            .filter(|(_, key2, _)| key2 == key)
            .collect();
        if same_key.len() < 2 {
            continue;
        }
        // Lexicographic order on pane_id.
        same_key.sort_by(|a, b| a.2.cmp(&b.2));
        let anchor = same_key[0].2.clone();
        let others: Vec<String> = same_key[1..].iter().map(|(_, _, id)| id.clone()).collect();

        let summary = if others.len() == 1 {
            format!("{} and {}", anchor, others[0])
        } else {
            format!("{} and {} other panes", anchor, others.len())
        };

        // Phase D D1: classify by window-label diversity. Empty labels
        // collapse into a single bucket — legacy callsites that never
        // populated `window_label` keep the original same-window
        // ConcurrentMutatingWork behavior.
        let mut windows: Vec<&str> = same_key.iter().map(|(v, _, _)| v.window_label).collect();
        windows.sort();
        windows.dedup();
        let cross_window = windows.len() >= 2;

        if cross_window {
            if !gates.cross_window_findings {
                continue;
            }
            let windows_summary = windows.join(", ");
            out.push(CrossPaneFinding {
                kind: CrossPaneKind::CrossWindowConcurrentWork,
                anchor_pane_id: anchor,
                other_pane_ids: others,
                reason: format!(
                    "concurrent mutating work on {summary} across windows {windows_summary} in {} on branch {} — same repo open in multiple windows; consolidate or coordinate explicitly",
                    key.path, key.branch
                ),
                severity: Severity::Concern,
                source_kind: SourceKind::Estimated,
                suggested_command: Some(
                    "# consolidate windows: tmux move-pane -s <pane_id> -t <other_window>".into(),
                ),
            });
        } else {
            out.push(CrossPaneFinding {
                kind: CrossPaneKind::ConcurrentMutatingWork,
                anchor_pane_id: anchor,
                other_pane_ids: others,
                reason: format!(
                    "concurrent mutating work on {summary} in {} on branch {} — risk of divergent edits; coordinate via research pane",
                    key.path, key.branch
                ),
                severity: Severity::Warning,
                source_kind: SourceKind::Estimated,
                suggested_command: Some(
                    "# coordinate via research pane: tmux select-pane -t <research_pane_id>".into(),
                ),
            });
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConcurrentKey {
    path: String,
    branch: String,
}

fn concurrent_key(view: &PaneView<'_>) -> Option<ConcurrentKey> {
    if view.current_path.is_empty() {
        return None;
    }
    let branch = view.signals.git_branch.as_ref()?.value.trim();
    if branch.is_empty() {
        return None;
    }
    Some(ConcurrentKey {
        path: view.current_path.to_string(),
        branch: branch.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{MetricValue, SignalSet};

    fn mk_id(role: Role, pane_id: &str) -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Claude,
                instance: 1,
                role,
                pane_id: pane_id.into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn busy_signals() -> SignalSet {
        SignalSet {
            output_chars: 800,
            ..SignalSet::default()
        }
    }

    fn busy_branch_signals(branch: &str) -> SignalSet {
        SignalSet {
            git_branch: Some(MetricValue::new(
                branch.to_string(),
                SourceKind::ProviderOfficial,
            )),
            ..busy_signals()
        }
    }

    #[test]
    fn two_main_panes_in_same_current_path_and_branch_trigger_finding() {
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
        ];
        let findings = eval_concurrent(&views, &PolicyGates::default());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].anchor_pane_id, "%1");
        assert_eq!(findings[0].other_pane_ids, vec!["%2".to_string()]);
        assert!(findings[0].reason.contains("branch main"));
    }

    #[test]
    fn same_current_path_without_branch_no_longer_co_qualifies() {
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_signals();
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
        ];
        let findings = eval_concurrent(&views, &PolicyGates::default());
        assert!(
            findings.is_empty(),
            "path-only concurrency was too noisy; require a shared branch"
        );
    }

    #[test]
    fn different_current_path_never_co_qualifies() {
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo-a",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo-b",
                window_label: "",
            },
        ];
        let findings = eval_concurrent(&views, &PolicyGates::default());
        assert!(
            findings.is_empty(),
            "Codex #1: different paths must not co-qualify"
        );
    }

    #[test]
    fn different_branches_in_same_current_path_do_not_co_qualify() {
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let main = busy_branch_signals("main");
        let feature = busy_branch_signals("feature");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &main,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &feature,
                current_path: "/repo",
                window_label: "",
            },
        ];
        let findings = eval_concurrent(&views, &PolicyGates::default());
        assert!(
            findings.is_empty(),
            "different branches narrow false positives"
        );
    }

    #[test]
    fn empty_current_path_does_not_co_qualify() {
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_signals();
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "",
                window_label: "",
            },
        ];
        let findings = eval_concurrent(&views, &PolicyGates::default());
        assert!(findings.is_empty(), "empty-path panes must not co-qualify");
    }

    #[test]
    fn single_pane_never_triggers() {
        let id_a = mk_id(Role::Main, "%1");
        let s = busy_signals();
        let views = vec![PaneView {
            identity: &id_a,
            signals: &s,
            current_path: "/repo",
            window_label: "",
        }];
        assert!(eval_concurrent(&views, &PolicyGates::default()).is_empty());
    }

    #[test]
    fn waiting_for_input_suppresses_finding() {
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let busy = busy_branch_signals("main");
        let waiting = SignalSet {
            idle_state: Some(IdleCause::InputWait),
            output_chars: 800,
            git_branch: Some(MetricValue::new(
                "main".to_string(),
                SourceKind::ProviderOfficial,
            )),
            ..SignalSet::default()
        };
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &busy,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &waiting,
                current_path: "/repo",
                window_label: "",
            },
        ];
        let findings = eval_concurrent(&views, &PolicyGates::default());
        assert!(
            findings.is_empty(),
            "pane waiting for input disqualifies the group"
        );
    }

    #[test]
    fn research_role_never_anchors() {
        let id_a = mk_id(Role::Research, "%1");
        let id_b = mk_id(Role::Research, "%2");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
        ];
        assert!(
            eval_concurrent(&views, &PolicyGates::default()).is_empty(),
            "Research-only group must not fire"
        );
    }

    #[test]
    fn anchor_pane_id_is_lexicographically_smallest_in_qualifying_set() {
        let id_z = mk_id(Role::Main, "%9");
        let id_a = mk_id(Role::Main, "%1");
        let id_m = mk_id(Role::Main, "%5");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_z,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_m,
                signals: &s,
                current_path: "/repo",
                window_label: "",
            },
        ];
        let findings = eval_concurrent(&views, &PolicyGates::default());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].anchor_pane_id, "%1");
        assert_eq!(
            findings[0].other_pane_ids,
            vec!["%5".to_string(), "%9".to_string()]
        );
    }

    #[test]
    fn output_chars_below_threshold_does_not_trigger() {
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let quiet = SignalSet {
            output_chars: 100,
            ..SignalSet::default()
        };
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &quiet,
                current_path: "/repo",
                window_label: "",
            },
            PaneView {
                identity: &id_b,
                signals: &quiet,
                current_path: "/repo",
                window_label: "",
            },
        ];
        assert!(eval_concurrent(&views, &PolicyGates::default()).is_empty());
    }

    // -----------------------------------------------------------------
    // Phase D D1 (v1.17.0) — cross-window concurrent-work correlation
    // -----------------------------------------------------------------

    fn gates_with_cross_window(enabled: bool) -> PolicyGates {
        PolicyGates {
            cross_window_findings: enabled,
            ..PolicyGates::default()
        }
    }

    #[test]
    fn cross_window_concurrent_work_fires_on_two_panes_in_different_windows_when_gate_enabled() {
        // Two healthy Main panes share `current_path` + `git_branch`
        // but live in different tmux windows. With the opt-in gate on,
        // the rule emits CrossWindowConcurrentWork (Concern, not the
        // default Warning ConcurrentMutatingWork) and names both
        // window labels in the reason text.
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "qmonster:0",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo",
                window_label: "scratch:0",
            },
        ];
        let findings = eval_concurrent(&views, &gates_with_cross_window(true));
        assert_eq!(
            findings.len(),
            1,
            "exactly one finding per cross-window group"
        );
        assert_eq!(findings[0].kind, CrossPaneKind::CrossWindowConcurrentWork);
        assert_eq!(findings[0].severity, Severity::Concern);
        assert_eq!(findings[0].anchor_pane_id, "%1");
        assert_eq!(findings[0].other_pane_ids, vec!["%2".to_string()]);
        assert!(
            findings[0].reason.contains("across windows"),
            "reason must call out cross-window scope: {:?}",
            findings[0].reason
        );
        assert!(findings[0].reason.contains("qmonster:0"));
        assert!(findings[0].reason.contains("scratch:0"));
    }

    #[test]
    fn cross_window_concurrent_work_does_not_fire_when_gate_disabled() {
        // Same cross-window scenario as above, but the operator has
        // not opted in. The rule must stay silent — no
        // CrossWindowConcurrentWork AND no ConcurrentMutatingWork
        // (the same-window kind is reserved for actual same-window
        // groups; cross-window panes are not "concurrent" by the
        // canonical contract).
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "qmonster:0",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo",
                window_label: "scratch:0",
            },
        ];
        let findings = eval_concurrent(&views, &gates_with_cross_window(false));
        assert!(
            findings.is_empty(),
            "cross-window detection is opt-in; no finding when gate is off, got: {findings:?}"
        );
    }

    #[test]
    fn same_window_path_branch_still_fires_concurrent_mutating_work_with_gate_enabled() {
        // Backward-compat: turning the cross-window gate on must NOT
        // change the same-window behavior. Two Main panes in the same
        // window sharing path+branch still get the original
        // ConcurrentMutatingWork Warning finding.
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo",
                window_label: "qmonster:0",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo",
                window_label: "qmonster:0",
            },
        ];
        let findings = eval_concurrent(&views, &gates_with_cross_window(true));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, CrossPaneKind::ConcurrentMutatingWork);
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn cross_window_does_not_fire_on_different_paths_across_windows() {
        // Two windows, same branch, but DIFFERENT current_paths must
        // not co-qualify — the existing path+branch key separates
        // them before window classification ever runs.
        let id_a = mk_id(Role::Main, "%1");
        let id_b = mk_id(Role::Main, "%2");
        let s = busy_branch_signals("main");
        let views = vec![
            PaneView {
                identity: &id_a,
                signals: &s,
                current_path: "/repo-a",
                window_label: "qmonster:0",
            },
            PaneView {
                identity: &id_b,
                signals: &s,
                current_path: "/repo-b",
                window_label: "scratch:0",
            },
        ];
        let findings = eval_concurrent(&views, &gates_with_cross_window(true));
        assert!(
            findings.is_empty(),
            "different paths must never co-qualify, even across windows"
        );
    }
}
