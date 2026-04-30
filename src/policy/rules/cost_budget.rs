use crate::domain::origin::SourceKind;
use crate::domain::recommendation::{Recommendation, Severity};
use crate::store::{CostBudgetAlert, CostBudgetAlertLevel};

pub fn recommendation_for_budget_alert(alert: &CostBudgetAlert) -> Recommendation {
    match alert.level {
        CostBudgetAlertLevel::Warning80 => Recommendation {
            action: "cost-budget: 80% reached",
            reason: format!(
                "SQLite cost ledger reached ${:.2} of ${:.2} budget ({:.0}%)",
                alert.spent_usd,
                alert.budget_usd,
                alert.threshold_pct * 100.0
            ),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: None,
            side_effects: vec![],
            is_strong: false,
            next_step: Some(
                "pace prompts, switch to a cheaper model, or wrap up high-cost panes".into(),
            ),
            profile: None,
        },
        CostBudgetAlertLevel::Critical100 => Recommendation {
            action: "cost-budget: exhausted",
            reason: format!(
                "SQLite cost ledger reached ${:.2} of ${:.2} budget ({:.0}%)",
                alert.spent_usd,
                alert.budget_usd,
                alert.threshold_pct * 100.0
            ),
            severity: Severity::Risk,
            source_kind: SourceKind::Estimated,
            suggested_command: None,
            side_effects: vec![],
            is_strong: true,
            next_step: Some(
                "press 's' to snapshot, then pause new prompts until the budget is reset or raised"
                    .into(),
            ),
            profile: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_budget_alert_becomes_warning_recommendation() {
        let rec = recommendation_for_budget_alert(&CostBudgetAlert {
            level: CostBudgetAlertLevel::Warning80,
            threshold_pct: 0.8,
            spent_usd: 160.0,
            budget_usd: 200.0,
        });

        assert_eq!(rec.action, "cost-budget: 80% reached");
        assert_eq!(rec.severity, Severity::Warning);
        assert_eq!(rec.source_kind, SourceKind::Estimated);
        assert!(!rec.is_strong);
        assert!(rec.reason.contains("$160.00"));
        assert!(
            rec.next_step
                .as_deref()
                .unwrap_or_default()
                .contains("pace")
        );
    }

    #[test]
    fn critical_budget_alert_becomes_strong_risk_recommendation() {
        let rec = recommendation_for_budget_alert(&CostBudgetAlert {
            level: CostBudgetAlertLevel::Critical100,
            threshold_pct: 1.0,
            spent_usd: 201.25,
            budget_usd: 200.0,
        });

        assert_eq!(rec.action, "cost-budget: exhausted");
        assert_eq!(rec.severity, Severity::Risk);
        assert!(rec.is_strong);
        assert!(rec.reason.contains("$201.25"));
        assert!(
            rec.next_step
                .as_deref()
                .unwrap_or_default()
                .contains("pause")
        );
    }
}
