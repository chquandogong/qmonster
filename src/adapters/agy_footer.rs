//! Slice C3: agy (Antigravity CLI) footer scrape.
//!
//! The agy TUI renders a configurable footer (model-name / context-used /
//! token-count items; see `~/.gemini/.../settings.json` `ui.footer.items`). The
//! captured pane tail therefore carries the model and, when those items are
//! enabled, context% and token-count as text. This parser extracts them; values
//! originate from agy (ProviderOfficial); each field is independent and best-effort.

/// Recognized Gemini model family prefixes shown in the agy footer/header
/// (e.g. "Gemini 3.5 Flash (High)", "Gemini 3.1 Pro (Low)").
const MODEL_PREFIXES: &[&str] = &["Gemini ", "Claude ", "GPT-OSS "];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgyFooter {
    pub model: Option<String>,
    pub context_used_pct: Option<f32>,
    pub context_window: Option<u64>,
    pub token_count: Option<u64>,
}

/// Parse the agy footer text out of a captured pane tail. Best-effort: any field
/// the footer does not carry stays `None`. `context_window` is never present in
/// the footer (the footer shows a percentage, not the window size) — it is only
/// ever filled by the sidefile path.
pub fn parse_agy_footer(tail: &str) -> AgyFooter {
    let mut out = AgyFooter::default();
    for raw in tail.lines() {
        let line = raw.trim();
        // model: the left-most occurrence of a known model family + "(level)".
        if out.model.is_none()
            && let Some(m) = extract_model(line)
        {
            out.model = Some(m);
        }
        if out.context_used_pct.is_none()
            && let Some(p) = extract_labeled_pct(line, "context-used")
        {
            out.context_used_pct = Some(p);
        }
        if out.token_count.is_none()
            && let Some(n) = extract_labeled_count(line, "token-count")
        {
            out.token_count = Some(n);
        }
    }
    out
}

fn extract_model(line: &str) -> Option<String> {
    let start = MODEL_PREFIXES.iter().filter_map(|p| line.find(p)).min()?;
    let candidate = line[start..].trim();
    // Require the "(level)" suffix the agy footer always renders so we don't grab
    // prose mentioning "Gemini".
    if candidate.contains('(') && candidate.ends_with(')') {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn extract_labeled_pct(line: &str, label: &str) -> Option<f32> {
    let idx = line.find(label)? + label.len();
    let rest = line[idx..].trim_start();
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return None;
    }
    // Require a literal '%' immediately after the digits
    let after_digits = &rest[num.len()..];
    if !after_digits.starts_with('%') {
        return None;
    }
    let pct: f32 = num.parse().ok()?;
    Some((pct / 100.0).clamp(0.0, 1.0))
}

fn extract_labeled_count(line: &str, label: &str) -> Option<u64> {
    let idx = line.find(label)? + label.len();
    let rest = line[idx..].trim_start();
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return None;
    }
    num.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_from_footer() {
        let tail = "> hello\n? for shortcuts                         Gemini 3.5 Flash (High)\n";
        let f = parse_agy_footer(tail);
        assert_eq!(f.model.as_deref(), Some("Gemini 3.5 Flash (High)"));
    }

    #[test]
    fn parses_context_and_token_items_when_present() {
        // Footer with context-used + token-count items enabled (showLabels on).
        let tail = "context-used 37%  token-count 34567  Gemini 3.1 Pro (High)\n";
        let f = parse_agy_footer(tail);
        assert!(
            f.context_used_pct
                .map(|v| (v - 0.37_f32).abs() < 1e-6)
                .unwrap_or(false),
            "context_used_pct must be ~0.37"
        );
        assert_eq!(f.context_window, None); // window size not shown in footer
        assert_eq!(f.token_count, Some(34567));
        assert_eq!(f.model.as_deref(), Some("Gemini 3.1 Pro (High)"));
    }

    #[test]
    fn absent_items_are_none() {
        let f = parse_agy_footer("> just a prompt, no footer line\n");
        assert!(f.model.is_none() && f.context_used_pct.is_none() && f.token_count.is_none());
    }
}
