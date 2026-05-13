use std::time::Instant;

use crate::app::auto_snapshot::maybe_auto_snapshot;
use crate::app::bootstrap::{Context, PanePressureCache};
use crate::app::effects::EffectRunner;
use crate::app::system_notice::SystemNotice;
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
/// Auto-snapshot notices are discarded — `--once` mode has no
/// dashboard to display them, and callers that need them use
/// `run_once_with_target` directly.
pub fn run_once<P, N>(
    ctx: &mut Context<P, N>,
    now: Instant,
) -> Result<Vec<PaneReport>, PollingError>
where
    P: PaneSource,
    N: NotifyBackend,
{
    run_once_with_target(ctx, now, None).map(|(reports, _notices)| reports)
}

/// Returns `(Vec<PaneReport>, Vec<SystemNotice>)`. The second element
/// carries any `SystemNotice`s emitted by `maybe_auto_snapshot` this
/// tick; callers route them to the dashboard.
pub fn run_once_with_target<P, N>(
    ctx: &mut Context<P, N>,
    now: Instant,
    target: Option<&crate::tmux::types::WindowTarget>,
) -> Result<(Vec<PaneReport>, Vec<SystemNotice>), PollingError>
where
    P: PaneSource,
    N: NotifyBackend,
{
    let panes = ctx.source.list_panes(target)?;
    let mut reports = Vec::with_capacity(panes.len());
    let mut all_auto_notices: Vec<SystemNotice> = Vec::new();

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

    // Phase F F-6 (v1.32.0): refresh the account-level Codex rate
    // limits ONCE per polling tick, before iterating panes — every
    // Codex pane in the loop below shares the same snapshot. A read
    // failure clears the cache (so stale data doesn't linger on
    // screen) and drops the client (the next tick won't try again
    // until restart). Spawn happens at TUI startup, not here.
    if let Some(server) = ctx.codex_app_server.as_mut() {
        match server.read_rate_limits() {
            Ok(rl) => {
                ctx.codex_rate_limits = Some(rl);
            }
            Err(e) => {
                eprintln!("F-6: codex app-server read_rate_limits failed: {e}");
                ctx.codex_app_server = None;
                ctx.codex_rate_limits = None;
            }
        }
    }

    for pane in panes {
        let overlay = ctx
            .runtime_refresh_tail_overlays
            .get(&pane.pane_id)
            .cloned();
        let parse_tail = overlay
            .as_ref()
            .map(|overlay| merge_runtime_refresh_tail(&pane.tail, overlay))
            .unwrap_or_else(|| pane.tail.clone());
        if overlay
            .as_deref()
            .is_some_and(|overlay| !runtime_refresh_tail_overlay_persists(overlay))
        {
            ctx.runtime_refresh_tail_overlays.remove(&pane.pane_id);
        }
        let effective_command =
            enhance_command_with_descendant_cmdline(&pane.current_command, pane.pane_pid);
        let raw = crate::domain::identity::RawPaneInput {
            pane_id: pane.pane_id.clone(),
            title: pane.title.clone(),
            current_command: effective_command.clone(),
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
            ctx.runtime_refresh_tail_overlays.remove(&pane.pane_id);
            // Phase D D2 (v1.18.0): a re-spawned pane is a fresh
            // identity history. Drop the stored snapshot and any
            // dedup keys for that pane so a CLI swap inside the new
            // lifetime can fire its own drift finding.
            ctx.identity_history.remove(&pane.pane_id);
            let pane_id_owned = pane.pane_id.clone();
            ctx.reported_drifts.retain(|(p, _)| p != &pane_id_owned);
            ctx.identity_conflicts_logged.remove(&pane.pane_id);
            // Phase F F-9: a re-spawned pane is a fresh error history.
            // Carrying the old error rate forward would let a stale
            // burst trip the profile-switch rule on the new lifetime.
            ctx.recent_error_observations.remove(&pane.pane_id);
            // Phase H (v1.42.0): a re-spawned pane starts a fresh
            // auto-snapshot dedup window. Stale dedup state from the
            // previous lifetime would suppress the first snapshot of
            // the new lifetime.
            ctx.auto_snapshot_dedup.remove(&pane.pane_id);
            // Phase 7 v1 (v1.43.0): a re-spawned pane starts fresh
            // anomaly observation. Stale history from the previous
            // lifetime would corrupt the detector window counts and
            // carry over edge-triggered dedup state incorrectly.
            ctx.anomaly_history.remove(&pane.pane_id);
            ctx.anomaly_dedup
                .retain(|(pid, _kind), _v| pid != &pane.pane_id);
        }

        if matches!(
            resolved.confidence,
            crate::domain::identity::IdentityConfidence::Conflict
        ) {
            if ctx.identity_conflicts_logged.insert(pane.pane_id.clone()) {
                ctx.sink.record(AuditEvent {
                    kind: AuditEventKind::IdentitySuppressed,
                    pane_id: pane.pane_id.clone(),
                    severity: Severity::Concern,
                    summary: format!(
                        "identity conflict suppressed provider metrics: title={} command={}",
                        pane.title, effective_command
                    ),
                    provider: Some(resolved.identity.provider),
                    role: Some(resolved.identity.role),
                });
            }
        } else {
            ctx.identity_conflicts_logged.remove(&pane.pane_id);
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
                current_command: effective_command,
                cross_pane_findings: vec![],
                idle_state: None,
                idle_state_entered_at: None,
                recent_token_samples: Vec::new(), // F-3: dead pane; no samples needed
                anomalies: vec![],                // Phase 7 v1: dead pane; no anomaly eval
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
            pane_pid: pane.pane_pid,
            current_path: &pane.current_path,
        };
        let mut signals = crate::adapters::parse_for(&parse_ctx);
        if !matches!(
            resolved.confidence,
            crate::domain::identity::IdentityConfidence::Conflict
        ) && let Some(fact) = crate::app::cli_version::resolve_cli_version_fact(
            resolved.identity.provider,
            &parse_tail,
            pane.pane_pid,
            &mut ctx.cli_version_cache,
        ) {
            signals.runtime_facts.push(fact);
        }

        // Phase F F-6 (v1.32.0): apply the account-level rate-limits
        // snapshot to Codex panes. `is_none()` guards on the
        // pressure fields preserve the per-pane statusline values
        // (Codex bottom status `5h N% weekly N%`) when present;
        // resets_at fields are unconditional because the statusline
        // doesn't carry them.
        if matches!(
            resolved.identity.provider,
            crate::domain::identity::Provider::Codex
        ) && !matches!(
            resolved.confidence,
            crate::domain::identity::IdentityConfidence::Conflict
        ) && let Some(rl) = ctx.codex_rate_limits.as_ref()
        {
            apply_codex_rate_limits(&mut signals, rl);
        }

        apply_pressure_metric_cache(
            &mut ctx.pressure_metric_cache,
            &pane.pane_id,
            resolved.identity.provider,
            &mut signals,
        );
        let gates = crate::policy::gates::PolicyGates::from_inputs(
            crate::policy::gates::PolicyGateInputs {
                token: &ctx.config.token,
                cost: &ctx.config.cost,
                context: &ctx.config.context,
                quota: &ctx.config.quota,
                security: &ctx.config.security,
                cache: &ctx.config.cache,
                reset: &ctx.config.reset,
                profile_switch: &ctx.config.profile_switch,
                anomaly: &ctx.config.anomaly,
                provider: resolved.identity.provider,
                confidence: resolved.confidence,
            },
        );
        // Read last idle state BEFORE calling evaluate so the engine can
        // detect transitions (None→Some, Some(X)→Some(Y)). Update AFTER.
        let last_idle = ctx.idle_transition.get(&pane.pane_id).copied().flatten();

        // F-7b: fetch historical samples BEFORE evaluate so the drift
        // detection rule has time-series context. Newest-first DESC.
        // Moving the read before evaluate (and the write after) ensures
        // the window passed to policy is strictly older than the current
        // iteration.
        //
        // F-3 (v1.49+ remediation): persistent read failures (corrupt
        // index, revoked permission, schema drift) now surface as
        // `TokenUsageReadFailed` audit events instead of disappearing
        // through `.ok()`. The sparkline still degrades to empty so
        // detection continues; per-pane dedup
        // (`Context.token_usage_read_failed_logged`) caps the audit-log
        // volume to one row per pane per session.
        let recent_token_samples: Vec<crate::store::TokenSample> = match ctx
            .token_usage_sink
            .as_ref()
            .map(|sink| sink.recent_samples(&pane.pane_id, 20))
        {
            Some(Ok(samples)) => {
                filter_token_samples_for_provider(samples, resolved.identity.provider)
            }
            Some(Err(e)) => {
                if ctx
                    .token_usage_read_failed_logged
                    .insert(pane.pane_id.clone())
                {
                    ctx.sink.record(AuditEvent {
                        kind: AuditEventKind::TokenUsageReadFailed,
                        pane_id: pane.pane_id.clone(),
                        severity: Severity::Concern,
                        summary: format!("recent_samples read failed: {e}"),
                        provider: Some(resolved.identity.provider),
                        role: Some(resolved.identity.role),
                    });
                }
                Vec::new()
            }
            None => Vec::new(),
        };

        // Phase F F-9 (dynamic profile switching): push the current
        // `error_hint` to the per-pane sliding window BEFORE engine
        // evaluation so the rule sees the freshest sample at index
        // 0. The window length tracks `gates.profile_switch_window`
        // so an operator who reduces the window mid-session sees
        // the buffer settle rather than wait for old entries to
        // age out.
        let recent_errors: Vec<bool> = {
            let cap = gates.profile_switch_window.max(1);
            let entry = ctx
                .recent_error_observations
                .entry(pane.pane_id.clone())
                .or_default();
            entry.push_front(signals.error_hint);
            while entry.len() > cap {
                entry.pop_back();
            }
            entry.iter().copied().collect()
        };

        // Phase 7 v1: push pre-rule observations into the pane's
        // AnomalyHistory. The post-rule observations (cache_drift_fires,
        // cross_pane_edit_paths) are pushed below after policy.evaluate.
        {
            let cap = gates.anomaly_window_polls.max(1);
            let entry = ctx.anomaly_history.entry(pane.pane_id.clone()).or_default();
            let now_unix_ms = current_unix_ms() as u64;
            entry.tick_unix_ms_samples.push_front(now_unix_ms);
            entry.tick_unix_secs_samples.push_front(now_unix_ms / 1000);
            let provider_label = format!("{:?}", resolved.identity.provider);
            entry
                .identity_snapshots
                .push_front((provider_label, pane.current_path.clone()));
            entry.error_hints.push_front(signals.error_hint);
            entry.error_hint_kinds.push_front(signals.error_hint_kind);
            // signals.cache_hit_ratio is Option<MetricValue<f64>>; cast to
            // f32 to match AnomalyHistory.cache_hit_ratios: VecDeque<Option<f32>>.
            entry
                .cache_hit_ratios
                .push_front(signals.cache_hit_ratio.as_ref().map(|m| m.value as f32));
            // Phase 7 v2 detectors (v1.45.0): push the 6 new
            // signal samples into AnomalyHistory. Cumulative
            // counters (cost_usd, input_tokens, output_tokens)
            // pass through `Option<...>` so missing samples don't
            // pollute the slope math; detectors filter None values.
            entry
                .cost_usd_samples
                .push_front(signals.cost_usd.as_ref().map(|m| m.value));
            entry
                .input_token_samples
                .push_front(signals.input_tokens.as_ref().map(|m| m.value));
            entry
                .output_token_samples
                .push_front(signals.output_tokens.as_ref().map(|m| m.value));
            entry
                .process_memory_samples
                .push_front(signals.process_memory_mb.as_ref().map(|m| m.value));
            entry
                .agent_memory_samples
                .push_front(signals.agent_memory_bytes.as_ref().map(|m| m.value));
            entry
                .subagent_hint_samples
                .push_front(signals.subagent_hint);
            entry.trim(cap);
        }

        let mut out: EvalOutput = ctx.policy.evaluate(
            &resolved,
            &signals,
            &gates,
            last_idle,
            &recent_token_samples,
            &recent_errors,
        );

        // F-7b: write current TokenSample AFTER evaluate so the historical
        // window passed to the policy represents strictly-older state.
        // F-3: persist token usage to SQLite when at least one of
        // the three fields is populated. UI consumers compute deltas
        // between samples for the sparkline; we never aggregate at
        // the writer. Failure here logs to stderr and does not
        // crash the polling loop (mirrors Codex Phase-2 audit-sink
        // durability pattern).
        // outer is `if let`; inner gates on a SignalSet predicate, not on the destructured value
        #[allow(clippy::collapsible_if)]
        if let Some(token_sink) = ctx.token_usage_sink.as_ref() {
            if signals.input_tokens.is_some()
                || signals.output_tokens.is_some()
                || signals.cost_usd.is_some()
                || signals.cached_input_tokens.is_some()
            {
                let ts_unix_ms = current_unix_ms();
                let sample = crate::store::TokenSample {
                    ts_unix_ms,
                    pane_id: pane.pane_id.clone(),
                    provider: resolved.identity.provider,
                    input_tokens: signals.input_tokens.as_ref().map(|m| m.value),
                    output_tokens: signals.output_tokens.as_ref().map(|m| m.value),
                    cost_usd: signals.cost_usd.as_ref().map(|m| m.value),
                    cached_input_tokens: signals.cached_input_tokens.as_ref().map(|m| m.value),
                };
                if let Err(e) = token_sink.record_sample(&sample) {
                    eprintln!("F-3: token_usage record_sample failed: {e}");
                }
            }
        }

        if let Some(cost_sink) = ctx.cost_usage_sink.as_ref()
            && let Some(cost) = signals.cost_usd.as_ref()
        {
            let obs = crate::store::CostObservation {
                ts_unix_ms: current_unix_ms(),
                pane_id: pane.pane_id.clone(),
                provider: resolved.identity.provider,
                cumulative_cost_usd: cost.value,
            };
            if let Err(e) = cost_sink.record_observation(&obs) {
                eprintln!("cost: record_observation failed: {e}");
            }
            match cost_sink.claim_budget_alerts(&ctx.config.cost, current_unix_ms()) {
                Ok(alerts) => {
                    let mut should_notify = false;
                    for alert in alerts {
                        let rec =
                            crate::policy::rules::cost_budget::recommendation_for_budget_alert(
                                &alert,
                            );
                        should_notify |= rec.severity >= Severity::Warning;
                        out.recommendations.push(rec);
                    }
                    if should_notify && !out.effects.contains(&RequestedEffect::Notify) {
                        out.effects.push(RequestedEffect::Notify);
                    }
                }
                Err(e) => eprintln!("cost: claim_budget_alerts failed: {e}"),
            }
        }

        // Phase D D2 (v1.18.0): identity-drift detection. Pure rule;
        // the caller owns the per-session history map and the dedup
        // hashset. Skip when the pane just appeared (no prev snapshot)
        // or just re-spawned (history was cleared above).
        let current_snapshot = crate::policy::rules::identity_drift::IdentitySnapshot {
            provider: resolved.identity.provider,
            current_path: pane.current_path.clone(),
        };
        if let Some(prev_snapshot) = ctx.identity_history.get(&pane.pane_id).cloned() {
            for finding in crate::policy::rules::identity_drift::detect_identity_drift(
                &pane.pane_id,
                &prev_snapshot,
                &current_snapshot,
                &gates,
            ) {
                let key = (pane.pane_id.clone(), finding.dedup_key);
                if ctx.reported_drifts.insert(key) {
                    out.recommendations.push(finding.recommendation);
                }
            }
        }
        ctx.identity_history
            .insert(pane.pane_id.clone(), current_snapshot);

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

        // Phase H (v1.42.0): opt-in actuation hook on the F-7c
        // recommend_snapshot_before_reset advisory. Reads from the
        // recommendations the rule just produced; writes one snapshot
        // per (pane_id, quota_kind, window_id) triple where window_id
        // = quota_*_resets_at floored to the nearest minute. Notices
        // are forwarded to the dashboard via the caller's return value.
        // Both conditions gate before touching the dedup map so the
        // baseline (disabled) regression sees an empty map.
        if let Some(writer) = ctx.snapshot_writer.as_ref()
            && gates.reset_auto_snapshot
        {
            // Extract the fields we need before any mutable borrow of ctx
            // so the borrow checker can verify disjoint access.
            let quota_5h_resets_at = signals.quota_5h_resets_at.as_ref().map(|m| m.value);
            let quota_weekly_resets_at = signals.quota_weekly_resets_at.as_ref().map(|m| m.value);
            let recs: Vec<Recommendation> = out.recommendations.clone();
            let dedup = ctx
                .auto_snapshot_dedup
                .entry(pane.pane_id.clone())
                .or_default();
            // Pass `gates.reset_auto_snapshot` rather than `true` so the
            // dual-gate is explicit and self-documenting. The outer
            // `if let Some(writer) ... && gates.reset_auto_snapshot`
            // ensures we only allocate the dedup entry when both
            // conditions hold; the inner argument lets
            // `maybe_auto_snapshot` remain a self-contained unit-testable
            // function whose contract works in any caller context.
            maybe_auto_snapshot(
                &pane.pane_id,
                &recs,
                gates.reset_auto_snapshot,
                quota_5h_resets_at,
                quota_weekly_resets_at,
                dedup,
                writer,
                &*ctx.sink,
                ctx.recommendation_lifecycle_sink.as_ref(),
                &mut all_auto_notices,
            );
        }

        // Phase 7 v1: push post-rule observations and run eval_anomalies.
        // cache_drift_fires: F-7b action string starts with
        // "cache: drift detected" (src/policy/rules/cache.rs line ~210).
        // cross_pane_edit_paths: F-8 ConcurrentFileEdit runs AFTER the
        // per-pane loop (line ~480+), so this tick's cross-pane findings
        // are not yet available. Option A: push Vec::new() here so the
        // history window accumulates; the previous tick's paths are
        // already in the deque. One-tick lag is acceptable for v1 — the
        // detector tolerates it because it works on the rolling window.
        //
        // v1.44.0 (Phase 7 v2): this block was moved up to run BEFORE
        // deliver_effects + the audit alert_event loop, matching the
        // F-9b cost-budget pattern at lines 345-362. Otherwise the
        // promote block (below) pushes Notify after deliver_effects
        // already ran, silently dropping desktop notifications, and
        // promoted recommendations never reach the audit log.
        // Phase 7 v3 (c) (v1.47.0): `tick_history` is hoisted out of the
        // `anomalies` block so it remains available for the persistence
        // snapshot taken after the ring push below.
        let tick_history: crate::policy::rules::anomaly::AnomalyHistory;
        let anomalies = {
            let cache_drift_fired = out
                .recommendations
                .iter()
                .any(|r| r.action.starts_with("cache: drift detected"));
            let cap = gates.anomaly_window_polls.max(1);
            // Phase 7 v3 (c) (v1.47.0): lazy hydrate AnomalyHistory deques
            // from persisted snapshots on first observation per pane this
            // session. Staleness gate: skip if newest snapshot > 2× window.
            if !ctx.anomaly_history_replayed.contains(&pane.pane_id) {
                if let Some(anomaly_sink) = ctx.anomaly_sink.as_ref() {
                    let window_polls = gates.anomaly_window_polls;
                    let snapshots =
                        anomaly_sink.fetch_recent_history_snapshots(&pane.pane_id, window_polls);
                    let tick_seconds = (ctx.config.tmux.poll_interval_ms / 1000).max(1);
                    let staleness_threshold = (window_polls as u64) * tick_seconds * 2;
                    let now_secs = (current_unix_ms() / 1000) as u64;
                    if let Some((newest_ts, _)) = snapshots.first()
                        && now_secs.saturating_sub(*newest_ts) <= staleness_threshold
                    {
                        let history = ctx.anomaly_history.entry(pane.pane_id.clone()).or_default();
                        // snapshots is newest-first; replay oldest-first so
                        // deque ordering matches live push semantics:
                        for (tick, snap) in snapshots.iter().rev() {
                            crate::store::push_snapshot_into_history_at(history, *tick, snap);
                        }
                        history.trim(window_polls);
                    }
                }
                ctx.anomaly_history_replayed.insert(pane.pane_id.clone());
            }
            {
                let entry = ctx.anomaly_history.entry(pane.pane_id.clone()).or_default();
                entry.cache_drift_fires.push_front(cache_drift_fired);
                // Option A: empty Vec this tick; cross-pane findings from
                // this tick are not available until after the per-pane loop.
                entry.cross_pane_edit_paths.push_front(Vec::new());
                entry.trim(cap);
            }
            let history = ctx
                .anomaly_history
                .get(&pane.pane_id)
                .cloned()
                .unwrap_or_default();
            let result = crate::policy::rules::anomaly::eval_anomalies(
                &pane.pane_id,
                resolved.identity.provider,
                &history,
                &gates,
                &mut ctx.anomaly_dedup,
                (current_unix_ms() / 1000) as u64,
            );
            tick_history = history;
            result
        };

        // Phase 7 v2 (v1.44.0): promote Warning+ anomalies to Recommendations
        // and trigger Notify, mirroring F-9b cost-budget pattern.
        // Edge-triggered dedup is already enforced by eval_anomalies, so each
        // anomaly emits its Recommendation once per active window naturally.
        // Runs BEFORE deliver_effects + audit so promoted recs flow through
        // both the desktop notifier and the audit log.
        let mut should_notify_anomaly = false;
        for promoted in
            crate::policy::rules::anomaly::promote_anomalies_to_recommendations(&anomalies, &gates)
        {
            should_notify_anomaly |= promoted.severity >= Severity::Warning;
            out.recommendations.push(promoted);
        }
        if should_notify_anomaly && !out.effects.contains(&RequestedEffect::Notify) {
            out.effects.push(RequestedEffect::Notify);
        }

        // Phase 7 v3 (v1.46.0): push every visible signal into the
        // AnomalyEventsRing with its gate result, for the `n` overlay.
        // `anomalies` holds the post-eval_anomalies signal vec (visible
        // signals, post-visibility-filter); `gates` is the same
        // PolicyGates value used by the promote call above.
        // NOTE: this `promoted_bool` mirrors the gate inside
        // `promote_anomalies_to_recommendations`. Keep the two predicates
        // in sync — if one changes (e.g., per-pane override), update the
        // other in the same commit. Task 12's integration test verifies
        // they agree at default config.
        let now_secs: u64 = (current_unix_ms() / 1000) as u64;
        let mut events_pushed_this_tick: Vec<crate::domain::anomaly::AnomalyEvent> = Vec::new();
        for sig in &anomalies {
            let promoted_bool = sig.confidence >= gates.promote_min_confidence(sig.kind);
            let reason = anomaly_event_reason(sig);
            let event = crate::domain::anomaly::AnomalyEvent {
                timestamp: now_secs,
                pane_id: pane.pane_id.clone(),
                kind: sig.kind,
                confidence: sig.confidence,
                severity: sig.severity,
                promoted: promoted_bool,
                reason,
                evidence: sig.evidence.clone(),
            };
            ctx.anomaly_events_ring.push(event.clone());
            events_pushed_this_tick.push(event);
        }

        // Phase 7 v3 (c) (v1.47.0): persist visible anomalies + history
        // snapshot for this pane this tick. Single transaction for fsync
        // batching. Failures degrade gracefully (logged, do not block).
        if let Some(anomaly_sink) = ctx.anomaly_sink.as_ref() {
            let snap = crate::store::AnomalyHistorySnapshot::capture_tick(&tick_history);
            let result = anomaly_sink.with_transaction(|tx| {
                for sig_event in &events_pushed_this_tick {
                    let evidence_json =
                        crate::store::anomaly_sink::encode_evidence_json(&sig_event.evidence)?;
                    tx.execute(
                        "INSERT INTO anomaly_events
                         (ts_unix_secs, pane_id, kind, confidence, severity, promoted, reason, evidence_json)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        rusqlite::params![
                            sig_event.timestamp as i64,
                            sig_event.pane_id,
                            sig_event.kind.label(),
                            sig_event.confidence.label(),
                            sig_event.severity.label(),
                            sig_event.promoted as i64,
                            sig_event.reason,
                            evidence_json,
                        ],
                    )
                    .map_err(|e| crate::store::sqlite::SqliteError::Query(e.to_string()))?;
                }
                let cross_paths_json = serde_json::to_string(&snap.cross_pane_edit_paths)
                    .map_err(|e| crate::store::sqlite::SqliteError::Query(format!("json: {e}")))?;
                let (id_provider, id_path) = match &snap.identity {
                    Some((p, path)) => (Some(p.as_str()), Some(path.as_str())),
                    None => (None, None),
                };
                tx.execute(
                    "INSERT OR REPLACE INTO anomaly_history_snapshots
                     (pane_id, tick_unix_secs, identity_provider, identity_path,
                      error_hint, cache_hit_ratio, cache_drift_fire,
                      cross_pane_edit_paths_json, cost_usd, input_tokens, output_tokens,
                      process_memory_mb, agent_memory_bytes, subagent_hint)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    rusqlite::params![
                        pane.pane_id,
                        now_secs as i64,
                        id_provider,
                        id_path,
                        snap.error_hint.map(|b| b as i64),
                        snap.cache_hit_ratio.map(|r| r as f64),
                        snap.cache_drift_fire as i64,
                        cross_paths_json,
                        snap.cost_usd,
                        snap.input_tokens.map(|n| n as i64),
                        snap.output_tokens.map(|n| n as i64),
                        snap.process_memory_mb,
                        snap.agent_memory_bytes.map(|n| n as i64),
                        snap.subagent_hint as i64,
                    ],
                )
                .map_err(|e| crate::store::sqlite::SqliteError::Query(e.to_string()))?;
                Ok(())
            });
            if let Err(e) = result {
                eprintln!(
                    "anomaly_sink persistence failed (pane={}): {e}",
                    pane.pane_id
                );
            }
        }

        crate::app::insights_lifecycle::record_recommendation_events(
            ctx.recommendation_lifecycle_sink.as_ref(),
            &ctx.config,
            &pane.pane_id,
            resolved.identity.provider,
            resolved.identity.role,
            &out.recommendations,
            &out.effects,
        );

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
            current_command: effective_command,
            cross_pane_findings: vec![],
            idle_state,
            idle_state_entered_at: entered_at,
            recent_token_samples,
            anomalies,
        });
    }

    // Cross-pane pass (Phase 3A). Pure policy call; side-effects below.
    // Phase D D1 (v1.17.0): pre-compute the `session:window` label so
    // the cross-window rule can compare panes' window membership.
    let window_labels: Vec<String> = reports
        .iter()
        .map(|r| format!("{}:{}", r.session_name, r.window_index))
        .collect();
    let views: Vec<crate::policy::PaneView<'_>> = reports
        .iter()
        .zip(window_labels.iter())
        .filter(|(r, _)| !r.dead)
        .map(|(r, label)| crate::policy::PaneView {
            identity: &r.identity,
            signals: &r.signals,
            current_path: &r.current_path,
            window_label: label.as_str(),
        })
        .collect();

    // Phase D D1: cross-pane gate is a top-level (non-provider) gate.
    // Read security.cross_window_findings directly; identity confidence
    // is irrelevant for the cross-window classification.
    let cross_pane_gates = crate::policy::PolicyGates {
        cross_window_findings: ctx.config.security.cross_window_findings,
        cross_pane_file_findings: ctx.config.security.cross_pane_file_findings,
        ..Default::default()
    };
    let findings = ctx.policy.evaluate_cross_pane(&views, &cross_pane_gates);

    fold_finding_paths_into_history(&findings, &mut ctx.anomaly_history);

    for mut f in findings {
        enrich_cross_pane_finding_with_worktree_command(&mut f, &reports);
        if let Some(r) = reports.iter_mut().find(|r| r.pane_id == f.anchor_pane_id) {
            r.cross_pane_findings.push(f);
        }
    }

    Ok((reports, all_auto_notices))
}

fn enrich_cross_pane_finding_with_worktree_command(
    finding: &mut crate::domain::recommendation::CrossPaneFinding,
    reports: &[PaneReport],
) {
    use crate::domain::recommendation::CrossPaneKind;

    if finding.suggested_command.is_some()
        || !matches!(
            finding.kind,
            CrossPaneKind::ConcurrentMutatingWork | CrossPaneKind::ConcurrentFileEdit
        )
    {
        return;
    }

    let Some(anchor) = reports.iter().find(|r| r.pane_id == finding.anchor_pane_id) else {
        return;
    };
    let branch = anchor
        .signals
        .git_branch
        .as_ref()
        .map(|m| m.value.as_str())
        .unwrap_or("");
    if let Some(suggestion) =
        crate::app::worktree_info::suggest_worktree_split_command(&anchor.current_path, branch)
    {
        finding.suggested_command = Some(suggestion.command);
    }
}

/// Build the `reason` string the `n` overlay and the `anomaly_events`
/// SQLite table store for a visible `AnomalySignal`. Joins every
/// `AnomalyEvidence` row with ` | ` so multi-row detectors (today only
/// `ErrorBurst`, which adds a `dominant_kind` row alongside its rate
/// delta) keep both pieces of evidence visible end-to-end. Single-row
/// detectors keep their pre-v1.50.0 reason unchanged.
pub(crate) fn anomaly_event_reason(sig: &crate::domain::anomaly::AnomalySignal) -> String {
    sig.evidence
        .iter()
        .map(|e| format!("{}: {} → {}", e.metric_name, e.before, e.after))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Close the Phase 7 v2 `CrossPaneEditCluster` detector loop: the
/// per-pane block earlier in `run_loop_iteration` pushes `Vec::new()` to
/// the front of each pane's `AnomalyHistory.cross_pane_edit_paths`
/// deque (Option A 1-tick-lag pattern). Without this fold-back the
/// deque is empty forever and the detector never fires.
///
/// For every finding that carries a non-empty `paths` payload (today
/// only `CrossPaneKind::ConcurrentFileEdit`), append those paths to the
/// front entry of every attributed pane's deque (anchor + others). The
/// detector that runs on the *next* tick will then see real evidence,
/// matching the documented one-tick lag.
pub(crate) fn fold_finding_paths_into_history(
    findings: &[crate::domain::recommendation::CrossPaneFinding],
    history: &mut std::collections::HashMap<String, crate::policy::rules::anomaly::AnomalyHistory>,
) {
    for f in findings {
        if f.paths.is_empty() {
            continue;
        }
        let affected = std::iter::once(f.anchor_pane_id.as_str())
            .chain(f.other_pane_ids.iter().map(|s| s.as_str()));
        for pane_id in affected {
            if let Some(entry) = history.get_mut(pane_id)
                && let Some(front) = entry.cross_pane_edit_paths.front_mut()
            {
                front.extend(f.paths.iter().cloned());
            }
        }
    }
}

/// Phase F F-6 (v1.32.0): write the account-level rate-limits
/// snapshot from `codex app-server` into a per-Codex-pane SignalSet.
/// `is_none()` guards on `quota_5h_pressure` / `quota_weekly_pressure`
/// preserve the pane's own statusline values (Codex bottom status
/// `5h N% weekly N%`) when populated; resets_at fields are
/// unconditional because no other surface emits them today.
fn apply_codex_rate_limits(
    signals: &mut SignalSet,
    rl: &crate::adapters::codex_app_server::CodexRateLimits,
) {
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::MetricValue;
    let metric_pct = |pct: u8| {
        MetricValue::new((pct as f32) / 100.0, SourceKind::ProviderOfficial)
            .with_confidence(0.95)
            .with_provider(Provider::Codex)
    };
    let metric_ts = |ts: u64| {
        MetricValue::new(ts, SourceKind::ProviderOfficial)
            .with_confidence(0.95)
            .with_provider(Provider::Codex)
    };
    if let Some(w) = rl.primary.as_ref() {
        if signals.quota_5h_pressure.is_none() {
            signals.quota_5h_pressure = Some(metric_pct(w.used_percent));
        }
        signals.quota_5h_resets_at = Some(metric_ts(w.resets_at_unix_seconds));
    }
    if let Some(w) = rl.secondary.as_ref() {
        if signals.quota_weekly_pressure.is_none() {
            signals.quota_weekly_pressure = Some(metric_pct(w.used_percent));
        }
        signals.quota_weekly_resets_at = Some(metric_ts(w.resets_at_unix_seconds));
    }
}

pub fn current_unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn filter_token_samples_for_provider(
    samples: Vec<crate::store::TokenSample>,
    provider: Provider,
) -> Vec<crate::store::TokenSample> {
    if matches!(provider, Provider::Qmonster | Provider::Unknown) {
        return Vec::new();
    }
    samples
        .into_iter()
        .filter(|sample| sample.provider == provider)
        .collect()
}

/// Replace `pane_current_command` (the foreground process's basename
/// returned by tmux `#{pane_current_command}`) with the descendant's
/// full `/proc/<pid>/cmdline`. Applies to all panes — native binaries
/// (`claude`) get their argv flags, generic interpreters (`node`,
/// `python3`) resolve to the actual CLI script (`node /usr/bin/codex`).
///
/// Returns the original command unchanged when `pane_pid` is missing
/// or when the descendant walk fails (no descendants, walk error,
/// pane just exited). Operators still see the basename in those
/// cases — never blank.
fn enhance_command_with_descendant_cmdline(current_command: &str, pane_pid: Option<u32>) -> String {
    let Some(pid) = pane_pid else {
        return current_command.to_string();
    };
    crate::adapters::process_memory::read_descendant_cmdline(pid)
        .unwrap_or_else(|| current_command.to_string())
}

#[cfg(test)]
mod enhance_command_tests {
    use super::enhance_command_with_descendant_cmdline;

    #[test]
    fn passthrough_when_pane_pid_missing() {
        // No pid to walk → unchanged regardless of basename.
        assert_eq!(
            enhance_command_with_descendant_cmdline("claude", None),
            "claude"
        );
        assert_eq!(
            enhance_command_with_descendant_cmdline("node", None),
            "node"
        );
    }

    #[test]
    fn passthrough_when_descendant_walk_fails() {
        // Bogus pid → walk returns None → unchanged.
        assert_eq!(
            enhance_command_with_descendant_cmdline("claude", Some(99_999_999)),
            "claude"
        );
        assert_eq!(
            enhance_command_with_descendant_cmdline("node", Some(99_999_999)),
            "node"
        );
    }
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

fn runtime_refresh_tail_overlay_persists(captured_tail: &str) -> bool {
    let lower = captured_tail.to_lowercase();
    (lower.contains("select model") || lower.contains("model usage"))
        && (lower.contains("reset:") || lower.contains("resets:"))
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
            crate::app::insights_lifecycle::record_recommendation_outcome(
                ctx_holder.recommendation_lifecycle_sink.as_ref(),
                pane_id,
                "archive-preview-suggested",
                crate::store::RecommendationOutcome::Archived,
                format!("archived {bytes}B to {}", path.display()),
            );
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
            crate::app::insights_lifecycle::record_recommendation_outcome(
                ctx_holder.recommendation_lifecycle_sink.as_ref(),
                pane_id,
                "archive-preview-suggested",
                crate::store::RecommendationOutcome::Failed,
                format!("archive failed: {e}"),
            );
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
    /// Phase F F-3 (v1.24.0): recent token-usage samples for this
    /// pane, ordered ts_unix_ms DESC, capped at 20 rows. Populated
    /// from `SqliteTokenUsageSink::recent_samples` when the sink is
    /// open. Populated for every live pane every iteration (one
    /// indexed `SELECT ... LIMIT 20` per pane per poll, sub-ms cost
    /// against the same `qmonster.db` the sink writes); the UI layer
    /// decides which pane's vec to actually render. Empty vec when
    /// the sink is None (open failed at startup) or has no rows for
    /// this pane yet. Consumed by the UI sparkline to compute deltas
    /// between adjacent samples.
    pub recent_token_samples: Vec<crate::store::TokenSample>,
    /// Phase 7 v1 (v1.43.0): anomaly signals for this pane this
    /// tick, after edge-triggered dedup. Empty Vec when
    /// `[anomaly] enabled = false` (default), when no detector
    /// fired, or when `gates.anomaly_enabled` is false. One-tick
    /// lag on `CrossPaneEditCluster` — cross-pane findings are
    /// computed after the per-pane loop; this tick's
    /// `cross_pane_edit_paths` push carries `Vec::new()` and the
    /// previous tick's paths are already in the history window.
    pub anomalies: Vec<crate::domain::anomaly::AnomalySignal>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::codex_app_server::{CodexRateLimits, CodexRateWindow};
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::MetricValue;

    #[test]
    fn apply_codex_rate_limits_populates_resets_at_and_pressure_when_absent() {
        // Phase F F-6 contract: account-level rate-limits broadcast
        // to a Codex pane that has no statusline-derived pressure
        // values. Both pressure fields and both resets_at fields
        // populate from the snapshot.
        let mut signals = SignalSet::default();
        let rl = CodexRateLimits {
            primary: Some(CodexRateWindow {
                used_percent: 12,
                window_duration_mins: 300,
                resets_at_unix_seconds: 1_700_000_000,
            }),
            secondary: Some(CodexRateWindow {
                used_percent: 7,
                window_duration_mins: 10080,
                resets_at_unix_seconds: 1_700_500_000,
            }),
        };
        apply_codex_rate_limits(&mut signals, &rl);
        assert!((signals.quota_5h_pressure.as_ref().unwrap().value - 0.12).abs() < 1e-6);
        assert!((signals.quota_weekly_pressure.as_ref().unwrap().value - 0.07).abs() < 1e-6);
        assert_eq!(
            signals.quota_5h_resets_at.as_ref().unwrap().value,
            1_700_000_000
        );
        assert_eq!(
            signals.quota_weekly_resets_at.as_ref().unwrap().value,
            1_700_500_000
        );
    }

    #[test]
    fn apply_codex_rate_limits_preserves_statusline_pressure_when_present() {
        // Statusline path takes priority for usedPercent (it's the
        // pane's own surface). The app-server snapshot only fills
        // resets_at — those have no statusline equivalent.
        let mut signals = SignalSet {
            quota_5h_pressure: Some(MetricValue::new(0.42, SourceKind::ProviderOfficial)),
            quota_weekly_pressure: Some(MetricValue::new(0.31, SourceKind::ProviderOfficial)),
            ..SignalSet::default()
        };
        let rl = CodexRateLimits {
            primary: Some(CodexRateWindow {
                used_percent: 99,
                window_duration_mins: 300,
                resets_at_unix_seconds: 1_700_000_000,
            }),
            secondary: Some(CodexRateWindow {
                used_percent: 99,
                window_duration_mins: 10080,
                resets_at_unix_seconds: 1_700_500_000,
            }),
        };
        apply_codex_rate_limits(&mut signals, &rl);
        assert!((signals.quota_5h_pressure.as_ref().unwrap().value - 0.42).abs() < 1e-6);
        assert!((signals.quota_weekly_pressure.as_ref().unwrap().value - 0.31).abs() < 1e-6);
        // resets_at always populates from snapshot.
        assert_eq!(
            signals.quota_5h_resets_at.as_ref().unwrap().value,
            1_700_000_000
        );
    }

    #[test]
    fn apply_codex_rate_limits_handles_missing_secondary_window() {
        let mut signals = SignalSet::default();
        let rl = CodexRateLimits {
            primary: Some(CodexRateWindow {
                used_percent: 10,
                window_duration_mins: 300,
                resets_at_unix_seconds: 100,
            }),
            secondary: None,
        };
        apply_codex_rate_limits(&mut signals, &rl);
        assert!(signals.quota_5h_pressure.is_some());
        assert!(signals.quota_5h_resets_at.is_some());
        assert!(signals.quota_weekly_pressure.is_none());
        assert!(signals.quota_weekly_resets_at.is_none());
    }

    #[test]
    fn fold_finding_paths_into_history_extends_front_for_anchor_and_others() {
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::{CrossPaneFinding, CrossPaneKind, Severity};
        use crate::policy::rules::anomaly::AnomalyHistory;
        use std::collections::HashMap;

        // Pre-state: per-pane loop pushed an empty Vec to the front of
        // each pane's deque (mirroring `entry.cross_pane_edit_paths.push_front(Vec::new())`).
        let mut history: HashMap<String, AnomalyHistory> = HashMap::new();
        for pane in ["%1", "%2", "%3"] {
            let mut h = AnomalyHistory::default();
            h.cross_pane_edit_paths.push_front(Vec::new());
            history.insert(pane.into(), h);
        }

        let findings = vec![CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentFileEdit,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec!["%2".into(), "%3".into()],
            reason: "concurrent edit".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Heuristic,
            suggested_command: None,
            paths: vec!["/repo/src/foo.rs".into()],
        }];

        fold_finding_paths_into_history(&findings, &mut history);

        for pane in ["%1", "%2", "%3"] {
            let front = history[pane].cross_pane_edit_paths.front().unwrap();
            assert_eq!(
                front,
                &vec!["/repo/src/foo.rs".to_string()],
                "{pane} front entry must carry the finding paths so detect_cross_pane_edit_cluster has real input next tick",
            );
        }
    }

    #[test]
    fn fold_finding_paths_into_history_skips_empty_paths_payload() {
        // Other CrossPaneKind variants (ConcurrentMutatingWork,
        // CrossWindowConcurrentWork) carry empty paths today. They must
        // not corrupt the front Vec — leave it untouched.
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::{CrossPaneFinding, CrossPaneKind, Severity};
        use crate::policy::rules::anomaly::AnomalyHistory;
        use std::collections::HashMap;

        let mut history: HashMap<String, AnomalyHistory> = HashMap::new();
        let mut h = AnomalyHistory::default();
        h.cross_pane_edit_paths.push_front(Vec::new());
        history.insert("%1".into(), h);

        let findings = vec![CrossPaneFinding {
            kind: CrossPaneKind::ConcurrentMutatingWork,
            anchor_pane_id: "%1".into(),
            other_pane_ids: vec![],
            reason: "directory-level".into(),
            severity: Severity::Warning,
            source_kind: SourceKind::Estimated,
            suggested_command: None,
            paths: Vec::new(),
        }];

        fold_finding_paths_into_history(&findings, &mut history);

        assert!(
            history["%1"]
                .cross_pane_edit_paths
                .front()
                .unwrap()
                .is_empty(),
        );
    }

    #[test]
    fn fold_finding_paths_into_history_round_trip_fires_detector() {
        // End-to-end contract: after fold-back, the detector that reads
        // `cross_pane_edit_paths` must actually see the path counts. We
        // simulate three ticks of finding the same path and assert
        // detect_cross_pane_edit_cluster fires at the threshold.
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::{CrossPaneFinding, CrossPaneKind, Severity};
        use crate::policy::rules::anomaly::{AnomalyHistory, detect_cross_pane_edit_cluster};
        use std::collections::HashMap;

        let mut history: HashMap<String, AnomalyHistory> = HashMap::new();
        history.insert("%1".into(), AnomalyHistory::default());

        for _ in 0..3 {
            history
                .get_mut("%1")
                .unwrap()
                .cross_pane_edit_paths
                .push_front(Vec::new());

            let findings = vec![CrossPaneFinding {
                kind: CrossPaneKind::ConcurrentFileEdit,
                anchor_pane_id: "%1".into(),
                other_pane_ids: vec![],
                reason: "edit".into(),
                severity: Severity::Warning,
                source_kind: SourceKind::Heuristic,
                suggested_command: None,
                paths: vec!["/repo/src/foo.rs".into()],
            }];
            fold_finding_paths_into_history(&findings, &mut history);
        }

        let sig = detect_cross_pane_edit_cluster(&history["%1"], 20, 3, 1_700_000_000);
        assert!(
            sig.is_some(),
            "detector must fire when fold-back has populated the deque across the threshold window",
        );
    }

    #[test]
    fn anomaly_event_reason_joins_every_evidence_row() {
        // ErrorBurst (the only multi-row detector today) must surface BOTH
        // the rate row AND the dominant_kind row so the n overlay tells
        // the operator what kind of error spiked, not just a rate change.
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
        use crate::domain::anomaly::{AnomalyEvidence, AnomalySignal};
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;

        let sig = AnomalySignal {
            kind: AnomalyKind::ErrorBurst,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            evidence: vec![
                AnomalyEvidence {
                    metric_name: "error_rate",
                    before: "0.30".into(),
                    after: "0.65".into(),
                    sample_count: 20,
                    source_kind: SourceKind::Estimated,
                },
                AnomalyEvidence {
                    metric_name: "dominant_kind",
                    before: "rust_panic".into(),
                    after: "8/13".into(),
                    sample_count: 13,
                    source_kind: SourceKind::Estimated,
                },
            ],
            window_polls: 20,
            detected_at: 1_700_000_000,
        };
        assert_eq!(
            anomaly_event_reason(&sig),
            "error_rate: 0.30 → 0.65 | dominant_kind: rust_panic → 8/13",
        );
    }

    #[test]
    fn anomaly_event_reason_single_row_detector_unchanged_pre_v1_50() {
        // Most detectors (IdentityChurn, CacheDiscontinuity, CrossPaneEditCluster,
        // CostSlope, TokenSlope, MemoryGrowth, SubagentSideEffect) emit
        // exactly one evidence row. Their pre-v1.50.0 reason format must
        // stay byte-identical so SQLite anomaly_events.reason readers don't
        // observe a change.
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
        use crate::domain::anomaly::{AnomalyEvidence, AnomalySignal};
        use crate::domain::origin::SourceKind;
        use crate::domain::recommendation::Severity;

        let sig = AnomalySignal {
            kind: AnomalyKind::IdentityChurn,
            confidence: AnomalyConfidence::High,
            severity: Severity::Warning,
            evidence: vec![AnomalyEvidence {
                metric_name: "provider:path",
                before: "Codex:/b".into(),
                after: "Claude:/a".into(),
                sample_count: 3,
                source_kind: SourceKind::ProviderOfficial,
            }],
            window_polls: 20,
            detected_at: 1_700_000_000,
        };
        assert_eq!(
            anomaly_event_reason(&sig),
            "provider:path: Codex:/b → Claude:/a",
        );
    }

    #[test]
    fn anomaly_event_reason_empty_evidence_returns_empty_string() {
        // Defensive: evidence is non-empty in all current detectors, but
        // the function used to fall back to String::new() via unwrap_or_default.
        // Preserve that contract.
        use crate::domain::anomaly::AnomalySignal;
        use crate::domain::anomaly::{AnomalyConfidence, AnomalyKind};
        use crate::domain::recommendation::Severity;

        let sig = AnomalySignal {
            kind: AnomalyKind::SubagentSideEffect,
            confidence: AnomalyConfidence::Medium,
            severity: Severity::Concern,
            evidence: vec![],
            window_polls: 20,
            detected_at: 1_700_000_000,
        };
        assert_eq!(anomaly_event_reason(&sig), "");
    }
}
