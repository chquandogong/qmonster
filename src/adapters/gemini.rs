use crate::adapters::ProviderParser;
use crate::adapters::common::parse_common_signals;
use crate::adapters::runtime::push_provider_fact;
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{IdleCause, MetricValue, RuntimeFactKind, SignalSet};

pub struct GeminiAdapter;

impl ProviderParser for GeminiAdapter {
    fn parse(&self, ctx: &crate::adapters::ParserContext) -> SignalSet {
        let mut set = parse_common_signals(ctx.tail);
        append_gemini_runtime_facts(&mut set, ctx.tail);
        if let Some(status) = parse_gemini_status(ctx.tail) {
            if let Some(model) = status.model {
                set.model_name = Some(
                    MetricValue::new(model, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            if let Some(branch) = status.branch {
                set.git_branch = Some(
                    MetricValue::new(branch, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            if let Some(workspace) = status.workspace {
                set.worktree_path = Some(
                    MetricValue::new(workspace, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
            if let Some(context) = status.context_pressure {
                set.context_pressure = Some(
                    MetricValue::new(context, SourceKind::ProviderOfficial)
                        .with_confidence(0.95)
                        .with_provider(Provider::Gemini),
                );
            }
        } else if let Some(context) = parse_gemini_context_pressure(ctx.tail) {
            set.context_pressure = Some(
                MetricValue::new(context, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Gemini),
            );
        }
        if set.idle_state.is_none() {
            set.idle_state = classify_idle_gemini(ctx.tail, ctx.history);
        }
        set
    }
}

#[derive(Default)]
struct GeminiStatus {
    context_pressure: Option<f32>,
    branch: Option<String>,
    sandbox: Option<String>,
    model: Option<String>,
    workspace: Option<String>,
}

fn parse_gemini_status(tail: &str) -> Option<GeminiStatus> {
    let lines: Vec<&str> = tail.lines().collect();
    for idx in (0..lines.len()).rev() {
        let line = lines[idx];
        if !is_gemini_status_header(line) {
            continue;
        }

        let header_cols = split_gemini_status_columns(line);
        for data_line in lines.iter().skip(idx + 1) {
            let trimmed = data_line.trim();
            if trimmed.is_empty() || is_gemini_separator(trimmed) {
                continue;
            }

            let data_cols = split_gemini_status_columns(data_line);
            let status = gemini_status_from_columns(&header_cols, &data_cols);
            if status.context_pressure.is_some()
                || status.branch.is_some()
                || status.sandbox.is_some()
                || status.model.is_some()
                || status.workspace.is_some()
            {
                return Some(status);
            }
            break;
        }
    }
    None
}

fn gemini_status_from_columns(header_cols: &[&str], data_cols: &[&str]) -> GeminiStatus {
    let mut status = GeminiStatus::default();
    for (idx, header) in header_cols.iter().enumerate() {
        let Some(value) = data_cols
            .get(idx)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        match header.trim().to_lowercase().as_str() {
            "branch" => status.branch = Some(value.to_string()),
            "sandbox" => status.sandbox = Some(value.to_string()),
            "/model" | "model" => status.model = Some(value.to_string()),
            "workspace (/directory)" | "workspace" | "directory" => {
                status.workspace = Some(value.to_string())
            }
            "context" => status.context_pressure = parse_used_percent(value),
            _ => {}
        }
    }
    status
}

fn parse_gemini_context_pressure(tail: &str) -> Option<f32> {
    parse_gemini_status(tail).and_then(|status| status.context_pressure)
}

fn append_gemini_runtime_facts(set: &mut SignalSet, tail: &str) {
    let lower = tail.to_lowercase();
    if lower.contains("yolo ctrl+y") {
        push_provider_fact(
            &mut set.runtime_facts,
            Provider::Gemini,
            RuntimeFactKind::AutoMode,
            "YOLO Ctrl+Y",
            0.75,
        );
    }
    if let Some(status) = parse_gemini_status(tail) {
        if let Some(sandbox) = status.sandbox {
            push_provider_fact(
                &mut set.runtime_facts,
                Provider::Gemini,
                RuntimeFactKind::Sandbox,
                sandbox,
                0.95,
            );
        }
        if let Some(workspace) = status.workspace {
            push_provider_fact(
                &mut set.runtime_facts,
                Provider::Gemini,
                RuntimeFactKind::AllowedDirectory,
                workspace,
                0.95,
            );
        }
    }
}

fn split_gemini_status_columns(line: &str) -> Vec<&str> {
    let mut cols = Vec::new();
    let mut start = 0;
    let mut whitespace_start = None;
    let mut whitespace_count = 0;

    for (idx, ch) in line.char_indices() {
        if ch.is_whitespace() {
            if whitespace_count == 0 {
                whitespace_start = Some(idx);
            }
            whitespace_count += 1;
            continue;
        }

        if whitespace_count >= 2 {
            if let Some(end) = whitespace_start {
                let col = line[start..end].trim();
                if !col.is_empty() {
                    cols.push(col);
                }
            }
            start = idx;
        }
        whitespace_start = None;
        whitespace_count = 0;
    }

    let col = line[start..].trim();
    if !col.is_empty() {
        cols.push(col);
    }
    cols
}

fn parse_used_percent(cell: &str) -> Option<f32> {
    let (num, rest) = cell.trim().split_once('%')?;
    if !rest.trim_start().starts_with("used") {
        return None;
    }
    let pct = num.trim().parse::<u8>().ok()?;
    if pct > 100 {
        return None;
    }
    Some(pct as f32 / 100.0)
}

fn classify_idle_gemini(
    tail: &str,
    history: &crate::adapters::common::PaneTailHistory,
) -> Option<IdleCause> {
    if gemini_limit_hit(tail) {
        return Some(IdleCause::LimitHit);
    }
    if gemini_idle_cursor(tail) {
        return Some(IdleCause::WorkComplete);
    }
    if history.is_still(history.capacity()) {
        return Some(IdleCause::Stale);
    }
    None
}

/// True when the tail contains Gemini's input placeholder in the live
/// idle area.
///
/// The `*  ` prefix (asterisk + 2 spaces) is the prompt glyph Gemini
/// CLI renders when it is waiting for the next user message.
///
/// We cannot just check the last non-empty line because production
/// Gemini renders a status table below the placeholder. We also cannot
/// scan the whole tail blindly: an old placeholder can stay in scrollback
/// after a new request has started, keeping the pane falsely IDLE until
/// enough output pushes it out. The suffix after the placeholder must
/// therefore be empty or only Gemini UI chrome/status rows.
fn gemini_idle_cursor(tail: &str) -> bool {
    let lines: Vec<&str> = tail.lines().collect();
    lines.iter().enumerate().any(|(idx, line)| {
        let t = line.trim_start();
        t.starts_with("*  ")
            && t.contains("Type your message")
            && gemini_suffix_is_idle_chrome(&lines[idx + 1..])
    })
}

fn gemini_suffix_is_idle_chrome(lines: &[&str]) -> bool {
    let mut saw_status_header = false;
    let mut saw_status_data = false;

    for raw in lines {
        let line = raw.trim();
        if line.is_empty() || is_gemini_separator(line) {
            continue;
        }

        if !saw_status_header && is_gemini_status_header(line) {
            saw_status_header = true;
            continue;
        }

        if saw_status_header && !saw_status_data && is_gemini_status_data(line) {
            saw_status_data = true;
            continue;
        }

        return false;
    }

    true
}

fn is_gemini_separator(line: &str) -> bool {
    !line.is_empty()
        && line
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '-' | '─' | '━' | '═'))
}

fn is_gemini_status_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("branch")
        && lower.contains("sandbox")
        && lower.contains("/model")
        && lower.contains("quota")
        && lower.contains("context")
        && lower.contains("memory")
}

fn is_gemini_status_data(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("% used") && lower.contains("sandbox")
}

/// True when the Gemini status block reports quota exhaustion.
///
/// The detection is intentionally narrow: a header row must contain ALL
/// of `quota`, `context`, and `memory` (the three Gemini status column
/// titles) AND the very next non-empty row must contain `100% used`.
/// Without all three header words the signal does not fire, which
/// prevents bare prose like "the quota was 100% used yesterday" from
/// triggering a spurious LimitHit.
fn gemini_limit_hit(tail: &str) -> bool {
    let lines: Vec<&str> = tail.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if lower.contains("quota") && lower.contains("context") && lower.contains("memory") {
            for next in lines.iter().skip(i + 1) {
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.contains("100% used") {
                    return true;
                }
                break;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ParserContext;
    use crate::adapters::common::PaneTailHistory;
    use crate::domain::identity::{
        IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
    };
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::{IdleCause, RuntimeFactKind};
    use crate::policy::claude_settings::ClaudeSettings;
    use crate::policy::pricing::PricingTable;

    fn id() -> ResolvedIdentity {
        ResolvedIdentity {
            identity: PaneIdentity {
                provider: Provider::Gemini,
                instance: 1,
                role: Role::Research,
                pane_id: "%3".into(),
            },
            confidence: IdentityConfidence::High,
        }
    }

    fn ctx<'a>(
        id: &'a ResolvedIdentity,
        tail: &'a str,
        pricing: &'a PricingTable,
        settings: &'a ClaudeSettings,
        history: &'a PaneTailHistory,
    ) -> ParserContext<'a> {
        ParserContext {
            identity: id,
            tail,
            pricing,
            claude_settings: settings,
            history,
        }
    }

    #[test]
    fn gemini_adapter_inherits_subagent_hint() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(
            &id,
            "Starting subagent: web-explorer",
            &pricing,
            &settings,
            &history,
        );
        let set = GeminiAdapter.parse(&c);
        assert!(set.subagent_hint);
    }

    #[test]
    fn gemini_type_your_message_placeholder_yields_work_complete() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "previous output\n*  Type your message or @path/to/file";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::WorkComplete));
    }

    #[test]
    fn gemini_status_table_populates_context_pressure_from_context_column() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      47% used      63% used     118.8 MB     cdf3f5ed      user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let metric = set
            .context_pressure
            .as_ref()
            .expect("context pressure parsed");
        assert!((metric.value - 0.63).abs() < f32::EPSILON);
        assert_eq!(metric.source_kind, SourceKind::ProviderOfficial);
        assert_eq!(metric.provider, Some(Provider::Gemini));
        assert_eq!(metric.confidence, Some(0.95));
        assert_eq!(
            set.model_name.as_ref().expect("model parsed").value,
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            set.git_branch.as_ref().expect("branch parsed").value,
            "main"
        );
        assert_eq!(
            set.worktree_path.as_ref().expect("workspace parsed").value,
            "~/projects/mission-spec"
        );
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::Sandbox && f.value == "no sandbox")
        );
        assert!(set.runtime_facts.iter().any(|f| {
            f.kind == RuntimeFactKind::AllowedDirectory && f.value == "~/projects/mission-spec"
        }));
    }

    #[test]
    fn gemini_context_pressure_reads_context_not_quota_column() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
branch      sandbox         /model                     workspace (/directory)       quota         context      memory       session                    /auth
main        no sandbox      gemini-3.1-pro-preview     ~/projects/mission-spec      100% used     12% used     118.8 MB     cdf3f5ed      user@example.com";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        let metric = set
            .context_pressure
            .as_ref()
            .expect("context pressure parsed");
        assert!((metric.value - 0.12).abs() < f32::EPSILON);
        assert_eq!(
            set.idle_state,
            Some(IdleCause::LimitHit),
            "quota remains a separate limit signal"
        );
    }

    #[test]
    fn gemini_quota_100_used_with_full_status_columns_yields_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        // 3-column structural anchor: `quota`, `context`, `memory` words
        // present on header row; `100% used` on data row.
        let tail = "\
 branch  sandbox  /model  workspace  quota  context  memory  session  /auth
 main    no sandbox  gemini-3.1  ~/proj  100% used  0% used  119 MB  abc  user@x";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, Some(IdleCause::LimitHit));
    }

    #[test]
    fn gemini_quota_100_used_without_anchor_columns_does_not_fire_limit_hit() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        // Bare `100% used` in prose without the 3-column anchor must
        // not trigger LimitHit.
        let tail = "the quota was 100% used yesterday";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn gemini_yolo_hint_populates_runtime_auto_mode() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let c = ctx(&id, "YOLO Ctrl+Y", &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert!(
            set.runtime_facts
                .iter()
                .any(|f| f.kind == RuntimeFactKind::AutoMode && f.value == "YOLO Ctrl+Y")
        );
    }

    #[test]
    fn gemini_active_output_yields_none() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "✓  ReadFile foo.rs\n  5 lines";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }

    #[test]
    fn gemini_old_placeholder_in_scrollback_with_following_output_is_not_idle_cursor() {
        let id = id();
        let pricing = PricingTable::empty();
        let settings = ClaudeSettings::empty();
        let history = PaneTailHistory::empty();
        let tail = "\
*  Type your message or @path/to/file
──────────────────────────────────────
branch sandbox /model quota context memory
main no sandbox gemini-3.1 ~/proj 47% used 0% used 119 MB

Working on the new request now";
        let c = ctx(&id, tail, &pricing, &settings, &history);
        let set = GeminiAdapter.parse(&c);
        assert_eq!(set.idle_state, None);
    }
}
