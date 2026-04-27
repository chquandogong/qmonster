use std::time::Instant;

use crate::app::bootstrap::{Context, PanePressureCache};
use crate::app::effects::EffectRunner;
use crate::domain::audit::{AuditEvent, AuditEventKind};
use crate::domain::identity::Provider;
use crate::domain::recommendation::{Recommendation, RequestedEffect, Severity};
use crate::domain::signal::SignalSet;
use crate::notify::desktop::{NotifyBackend, summarize};
use crate::policy::engine::EvalOutput;
use crate::store::sink::EventSink;
use crate::tmux::polling::{PaneSource, PollingError};

/// Snapshot of effect permissions — cheap to pass by value, avoids
/// borrow-checker friction in the main loop. Computed once per
/// iteration from the current `QmonsterConfig`.
#[derive(Debug, Clone, Copy)]
struct EffectPermits {
    notify: bool,
    archive: bool,
}

/// One iteration of the observe loop. Pure over the side-effect
/// interfaces (ctx.sink, ctx.notifier) so tests can swap them out.
pub fn run_once<P, N>(
    ctx: &mut Context<P, N>,
    now: Instant,
) -> Result<Vec<PaneReport>, PollingError>
where
    P: PaneSource,
    N: NotifyBackend,
{
    run_once_with_target(ctx, now, None)
}

pub fn run_once_with_target<P, N>(
    ctx: &mut Context<P, N>,
    now: Instant,
    target: Option<&crate::tmux::types::WindowTarget>,
) -> Result<Vec<PaneReport>, PollingError>
where
    P: PaneSource,
    N: NotifyBackend,
{
    let panes = ctx.source.list_panes(target)?;
    let mut reports = Vec::with_capacity(panes.len());

    // Lifecycle bookkeeping (zombie pane / re-attach reset).
    let current_ids: Vec<String> = panes.iter().map(|p| p.pane_id.clone()).collect();
    let known: Vec<String> = ctx.known_pane_ids().to_vec();
    for id in &known {
        if !current_ids.contains(id) {
            ctx.lifecycle.forget(id);
        }
    }
    ctx.set_known_pane_ids(current_ids);

    let permits = {
        let runner = EffectRunner::new(&ctx.config);
        EffectPermits {
            notify: runner.permit(&RequestedEffect::Notify),
            archive: runner.permit(&RequestedEffect::ArchiveLocal),
        }
    };

    for pane in panes {
        let parse_tail = ctx
            .runtime_refresh_tail_overlays
            .remove(&pane.pane_id)
            .map(|overlay| merge_runtime_refresh_tail(&pane.tail, &overlay))
            .unwrap_or_else(|| pane.tail.clone());
        let raw = crate::domain::identity::RawPaneInput {
            pane_id: pane.pane_id.clone(),
            title: pane.title.clone(),
            current_command: pane.current_command.clone(),
            tail: parse_tail.clone(),
        };
        let resolved = ctx.resolver.resolve(&raw);

        let lc = ctx.lifecycle.observe(&pane.pane_id, pane.dead);
        record_lifecycle(&*ctx.sink, &pane.pane_id, lc, resolved.identity.provider);

        // Reset per-pane caches when the pane dies or re-attaches so stale
        // tail history doesn't bleed into the new pane lifetime.
        use crate::domain::lifecycle::PaneLifecycleEvent as L;
        if matches!(lc, L::BecameDead | L::Reappeared) {
            ctx.tail_history.remove(&pane.pane_id);
            ctx.idle_transition.remove(&pane.pane_id);
            ctx.idle_entered_at.remove(&pane.pane_id);
            ctx.pressure_metric_cache.remove(&pane.pane_id);
        }

        if pane.dead {
            reports.push(PaneReport {
                pane_id: pane.pane_id,
                session_name: pane.session_name,
                window_index: pane.window_index,
                provider: resolved.identity.provider,
                identity: resolved.clone(),
                signals: crate::domain::signal::SignalSet::default(),
                recommendations: vec![],
                effects: vec![],
                dead: true,
                current_path: pane.current_path.clone(),
                current_command: pane.current_command.clone(),
                cross_pane_findings: vec![],
                idle_state: None,
                idle_state_entered_at: None,
            });
            continue;
        }

        // Maintain a rolling tail history per pane so adapters can detect
        // stillness across consecutive snapshots (Slice 4, Task 9).
        let history_for_pane = ctx
            .tail_history
            .entry(pane.pane_id.clone())
            .or_insert_with(|| {
                crate::adapters::common::PaneTailHistory::new(ctx.config.idle.stillness_polls)
            });
        history_for_pane.push(parse_tail.clone());

        let parse_ctx = crate::adapters::ParserContext {
            identity: &resolved,
            tail: &parse_tail,
            pricing: &ctx.pricing,
            claude_settings: &ctx.claude_settings,
            history: history_for_pane,
        };
        let mut signals = crate::adapters::parse_for(&parse_ctx);
        apply_pressure_metric_cache(
            &mut ctx.pressure_metric_cache,
            &pane.pane_id,
            resolved.identity.provider,
            &mut signals,
        );
        let gates = crate::policy::gates::PolicyGates::from_config_and_identity(
            &ctx.config.token,
            &ctx.config.cost,
            &ctx.config.context,
            &ctx.config.quota,
            &ctx.config.security,
            resolved.identity.provider,
            resolved.confidence,
        );
        // Read last idle state BEFORE calling evaluate so the engine can
        // detect transitions (None→Some, Some(X)→Some(Y)). Update AFTER.
        let last_idle = ctx.idle_transition.get(&pane.pane_id).copied().flatten();
        let out: EvalOutput = ctx.policy.evaluate(&resolved, &signals, &gates, last_idle);

        // Update idle_entered_at only on cause transitions, not on every poll.
        match (last_idle, signals.idle_state) {
            (None, Some(_)) => {
                ctx.idle_entered_at.insert(pane.pane_id.clone(), now);
            }
            (Some(a), Some(b)) if a != b => {
                ctx.idle_entered_at.insert(pane.pane_id.clone(), now);
            }
            (Some(_), None) => {
                ctx.idle_entered_at.remove(&pane.pane_id);
            }
            _ => {} // Some→Same or None→None: preserve existing entered_at
        }
        ctx.idle_transition
            .insert(pane.pane_id.clone(), signals.idle_state);

        let idle_state = signals.idle_state;
        let entered_at = ctx.idle_entered_at.get(&pane.pane_id).copied();

        deliver_effects(permits, &out, &pane.pane_id, &pane.tail, now, ctx);

        for rec in &out.recommendations {
            ctx.sink
                .record(alert_event(&pane.pane_id, rec, resolved.identity.provider));
        }

        reports.push(PaneReport {
            pane_id: pane.pane_id,
            session_name: pane.session_name,
            window_index: pane.window_index,
            provider: resolved.identity.provider,
            identity: resolved,
            signals,
            recommendations: out.recommendations,
            effects: out.effects,
            dead: false,
            current_path: pane.current_path.clone(),
            current_command: pane.current_command.clone(),
            cross_pane_findings: vec![],
            idle_state,
            idle_state_entered_at: entered_at,
        });
    }

    // Cross-pane pass (Phase 3A). Pure policy call; side-effects below.
    let views: Vec<crate::policy::PaneView<'_>> = reports
        .iter()
        .filter(|r| !r.dead)
        .map(|r| crate::policy::PaneView {
            identity: &r.identity,
            signals: &r.signals,
            current_path: &r.current_path,
        })
        .collect();

    let findings = ctx.policy.evaluate_cross_pane(&views);

    for f in findings {
        if let Some(r) = reports.iter_mut().find(|r| r.pane_id == f.anchor_pane_id) {
            r.cross_pane_findings.push(f);
        }
    }

    Ok(reports)
}

fn merge_runtime_refresh_tail(live_tail: &str, captured_tail: &str) -> String {
    if captured_tail.trim().is_empty() {
        return live_tail.to_string();
    }
    if live_tail.trim().is_empty() {
        return captured_tail.to_string();
    }
    // Runtime refresh captures provider facts from transient fullscreen
    // surfaces. Keep the live pane tail last so prompt-ready cursor
    // detection still reflects the pane's real post-refresh state.
    format!("{captured_tail}\n{live_tail}")
}

fn apply_pressure_metric_cache(
    cache_by_pane: &mut std::collections::HashMap<String, PanePressureCache>,
    pane_id: &str,
    provider: Provider,
    signals: &mut SignalSet,
) {
    if provider != Provider::Claude {
        cache_by_pane.remove(pane_id);
        return;
    }

    let cache = cache_by_pane.entry(pane_id.to_string()).or_default();
    if cache.provider != Some(provider) {
        *cache = PanePressureCache {
            provider: Some(provider),
            ..PanePressureCache::default()
        };
    }

    if let Some(metric) = signals.context_pressure {
        cache.context_pressure = Some(metric);
    } else {
        signals.context_pressure = cache.context_pressure;
    }
    if let Some(metric) = signals.quota_pressure {
        cache.quota_pressure = Some(metric);
    } else {
        signals.quota_pressure = cache.quota_pressure;
    }
    if let Some(metric) = signals.quota_5h_pressure {
        cache.quota_5h_pressure = Some(metric);
    } else {
        signals.quota_5h_pressure = cache.quota_5h_pressure;
    }
    if let Some(metric) = signals.quota_weekly_pressure {
        cache.quota_weekly_pressure = Some(metric);
    } else {
        signals.quota_weekly_pressure = cache.quota_weekly_pressure;
    }
}

fn deliver_effects<N: NotifyBackend>(
    permits: EffectPermits,
    out: &EvalOutput,
    pane_id: &str,
    tail: &str,
    now: Instant,
    ctx_holder: &mut Context<impl PaneSource, N>,
) {
    for effect in &out.effects {
        match effect {
            RequestedEffect::Notify => {
                if permits.notify {
                    dispatch_notify(out, pane_id, now, ctx_holder);
                }
            }
            RequestedEffect::ArchiveLocal => {
                if permits.archive {
                    dispatch_archive(pane_id, tail, ctx_holder);
                }
            }
            // P5-1 scaffolding only: the proposal is surfaced via the
            // PaneReport.effects list for the UI layer to render. No
            // runtime dispatch here — the tmux send-keys call and the
            // operator-confirmation UX land in a later Phase-5 slice.
            RequestedEffect::PromptSendProposed { .. } => continue,
            // Always denied — there is no code path that produces it and
            // this arm doubles as a guardrail if a future rule slips.
            RequestedEffect::SensitiveNotImplemented => continue,
        }
    }
}

fn dispatch_archive<N: NotifyBackend>(
    pane_id: &str,
    tail: &str,
    ctx_holder: &mut Context<impl PaneSource, N>,
) {
    let Some(archive) = ctx_holder.archive.as_ref() else {
        return;
    };
    match archive.archive_if_long(pane_id, tail) {
        Ok(crate::store::ArchiveOutcome::Archived { path, bytes, .. }) => {
            ctx_holder.sink.record(AuditEvent {
                kind: AuditEventKind::ArchiveWritten,
                pane_id: pane_id.to_string(),
                severity: Severity::Safe,
                summary: format!("archived {bytes}B → {}", path.display()),
                provider: None,
                role: None,
            });
        }
        Ok(crate::store::ArchiveOutcome::Skipped { .. }) => {}
        Err(e) => {
            ctx_holder.sink.record(AuditEvent {
                kind: AuditEventKind::ArchiveWritten,
                pane_id: pane_id.to_string(),
                severity: Severity::Warning,
                summary: format!("archive failed: {e}"),
                provider: None,
                role: None,
            });
        }
    }
}

fn dispatch_notify<N: NotifyBackend>(
    out: &EvalOutput,
    pane_id: &str,
    now: Instant,
    ctx_holder: &mut Context<impl PaneSource, N>,
) {
    use crate::domain::recommendation::Severity;
    for rec in out
        .recommendations
        .iter()
        .filter(|r| r.severity >= Severity::Warning)
    {
        if ctx_holder
            .rate_limiter
            .should_fire(pane_id, rec.action, rec.severity, now)
        {
            let (title, body) = summarize(rec, pane_id);
            ctx_holder.notifier.notify(&title, &body, rec.severity);
        }
    }
}

fn record_lifecycle(
    sink: &dyn EventSink,
    pane_id: &str,
    lc: crate::domain::lifecycle::PaneLifecycleEvent,
    provider: Provider,
) {
    use crate::domain::lifecycle::PaneLifecycleEvent as L;
    let kind = match lc {
        L::BecameDead => AuditEventKind::PaneBecameDead,
        L::Reappeared => AuditEventKind::PaneReappeared,
        L::Appeared => AuditEventKind::PaneIdentityResolved,
        L::Unchanged => return,
    };
    sink.record(AuditEvent {
        kind,
        pane_id: pane_id.to_string(),
        severity: Severity::Safe,
        summary: format!("{:?}", lc),
        provider: Some(provider),
        role: None,
    });
}

fn alert_event(pane_id: &str, rec: &Recommendation, provider: Provider) -> AuditEvent {
    let kind = if rec.severity >= Severity::Warning {
        AuditEventKind::AlertFired
    } else {
        AuditEventKind::RecommendationEmitted
    };
    AuditEvent {
        kind,
        pane_id: pane_id.to_string(),
        severity: rec.severity,
        summary: format!("{}: {}", rec.action, rec.reason),
        provider: Some(provider),
        role: None,
    }
}

/// Compact per-iteration summary used by the UI and the `--once`
/// formatter. Carries enough of the pipeline output for Phase 1
/// surfacing requirements (VALIDATION.md §60-62).
#[derive(Debug, Clone)]
pub struct PaneReport {
    pub pane_id: String,
    pub session_name: String,
    pub window_index: String,
    pub provider: Provider,
    pub identity: crate::domain::identity::ResolvedIdentity,
    pub signals: crate::domain::signal::SignalSet,
    pub recommendations: Vec<Recommendation>,
    pub effects: Vec<RequestedEffect>,
    pub dead: bool,
    pub current_path: String,
    pub current_command: String,
    pub cross_pane_findings: Vec<crate::domain::recommendation::CrossPaneFinding>,
    /// Current idle cause, if any. Mirrors `signals.idle_state` but
    /// co-located with `idle_state_entered_at` so the UI can render
    /// the elapsed-time badge without extra lookups.
    pub idle_state: Option<crate::domain::signal::IdleCause>,
    /// Instant at which the pane entered its current `idle_state`.
    /// `None` when `idle_state` is `None` (pane is active).
    pub idle_state_entered_at: Option<std::time::Instant>,
}
