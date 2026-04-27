use crate::app::config::QmonsterConfig;
use crate::app::effects::EffectRunner;
use crate::app::event_loop::PaneReport;
use crate::domain::recommendation::RequestedEffect;

pub fn print_once_reports(reports: &[PaneReport], config: &QmonsterConfig) {
    for line in format_once_report_lines(reports, config) {
        println!("{line}");
    }
}

pub fn format_once_report_lines(reports: &[PaneReport], config: &QmonsterConfig) -> Vec<String> {
    let mut lines = Vec::new();

    for rep in reports {
        for f in &rep.cross_pane_findings {
            lines.push(format!(
                "[{}] [{}] CROSS-PANE: {} (anchor: {}, others: {})",
                f.severity.letter(),
                crate::ui::labels::source_kind_label(f.source_kind),
                f.reason,
                f.anchor_pane_id,
                f.other_pane_ids.join(", "),
            ));
        }
    }

    for rep in reports {
        for rec in rep.recommendations.iter().filter(|r| r.is_strong) {
            lines.push(crate::ui::alerts::format_strong_rec_body(rec, &rep.pane_id));
        }
    }

    let runner = EffectRunner::new(config);
    for rep in reports {
        for effect in &rep.effects {
            if let RequestedEffect::PromptSendProposed {
                target_pane_id,
                slash_command,
                ..
            } = effect
            {
                let accept_gated = runner.permit(effect);
                lines.push(crate::ui::alerts::format_prompt_send_proposal(
                    target_pane_id,
                    slash_command,
                    accept_gated,
                ));
            }
        }
    }

    for r in reports {
        lines.push(format!(
            "{}:{} {} {:?}:{}:{:?} confidence={:?} dead={}",
            r.session_name,
            r.window_index,
            r.pane_id,
            r.identity.identity.provider,
            r.identity.identity.instance,
            r.identity.identity.role,
            r.identity.confidence,
            r.dead
        ));
        lines.push(format!("  path: {}", r.current_path));
        lines.push(format!("  cmd: {}", r.current_command));
        let chips = crate::ui::panels::signal_chips(&r.signals);
        if !chips.is_empty() {
            lines.push(format!("  state: {}", chips.join(" | ")));
        }
        let metrics = crate::ui::panels::metric_row(&r.signals);
        if !metrics.is_empty() {
            lines.push(format!("  metrics: {metrics}"));
        }
        let runtime = crate::ui::panels::runtime_row(&r.signals);
        if !runtime.is_empty() {
            lines.push(format!("  runtime: {runtime}"));
        }
        if !r.effects.is_empty() {
            let names: Vec<String> = r.effects.iter().map(|e| format!("{e:?}")).collect();
            lines.push(format!("  effects: {}", names.join(" ")));
        }
        for rec in r.recommendations.iter().filter(|rec| !rec.is_strong) {
            lines.push(format!(
                "  {}",
                crate::ui::alerts::format_recommendation_body(rec, &r.pane_id)
            ));
            for line in crate::ui::panels::format_profile_lines(rec) {
                lines.push(format!("    {line}"));
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::recommendation::{
        CrossPaneFinding, CrossPaneKind, Recommendation, Severity,
    };
    use crate::domain::signal::SignalSet;

    fn base_report() -> PaneReport {
        PaneReport {
            pane_id: "%1".into(),
            session_name: "qwork".into(),
            window_index: "1".into(),
            provider: Provider::Claude,
            identity: ResolvedIdentity {
                identity: PaneIdentity {
                    provider: Provider::Claude,
                    instance: 1,
                    role: Role::Main,
                    pane_id: "%1".into(),
                },
                confidence: IdentityConfidence::High,
            },
            signals: SignalSet::default(),
            recommendations: vec![],
            effects: vec![],
            dead: false,
            current_path: "/repo".into(),
            current_command: "claude".into(),
            cross_pane_findings: vec![],
            idle_state: None,
            idle_state_entered_at: None,
        }
    }

    fn recommendation(is_strong: bool) -> Recommendation {
        Recommendation {
            action: "check token budget",
            reason: "context pressure high".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: Some("qmonster --once".into()),
            side_effects: vec![],
            is_strong,
            next_step: None,
            profile: None,
        }
    }

    #[test]
    fn once_report_lines_include_cross_pane_and_pane_summary() {
        let mut report = base_report();
        report.cross_pane_findings.push(CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentMutatingWork,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec!["%2".into()],
            reason: "same branch and path".into(),
            severity: Severity::Concern,
            source_kind: SourceKind::ProjectCanonical,
            suggested_command: None,
        });

        let lines = format_once_report_lines(&[report], &QmonsterConfig::defaults());

        assert!(lines.iter().any(|line| line.contains("CROSS-PANE")));
        assert!(lines.iter().any(|line| line == "  path: /repo"));
        assert!(lines.iter().any(|line| line == "  cmd: claude"));
    }

    #[test]
    fn once_report_lines_keep_strong_recs_before_pane_summary() {
        let mut report = base_report();
        report.recommendations.push(recommendation(true));

        let lines = format_once_report_lines(&[report], &QmonsterConfig::defaults());
        let strong_index = lines
            .iter()
            .position(|line| line.contains("CHECKPOINT"))
            .expect("strong recommendation line");
        let pane_index = lines
            .iter()
            .position(|line| line.starts_with("qwork:1 %1"))
            .expect("pane summary line");

        assert!(strong_index < pane_index);
    }

    #[test]
    fn once_report_lines_render_prompt_send_proposals() {
        let mut report = base_report();
        report.effects.push(RequestedEffect::PromptSendProposed {
            target_pane_id: "%1".into(),
            slash_command: "/status".into(),
            proposal_id: "%1:/status".into(),
        });

        let lines = format_once_report_lines(&[report], &QmonsterConfig::defaults());

        assert!(lines.iter().any(|line| line.contains("/status")));
        assert!(lines.iter().any(|line| line.starts_with("  effects:")));
    }
}
