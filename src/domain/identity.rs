use serde::{Deserialize, Serialize};

/// Raw pane info fed into the resolver. Identity resolution lives in
/// `domain/`, never in `adapters/` (r2 non-negotiable).
#[derive(Debug, Clone)]
pub struct RawPaneInput {
    pub pane_id: String,
    pub title: String,
    pub current_command: String,
    pub tail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Provider {
    Claude,
    Codex,
    Gemini,
    Qmonster,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Main,
    Review,
    Research,
    Monitor,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PaneIdentity {
    pub provider: Provider,
    pub instance: u32,
    pub role: Role,
    pub pane_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdentityConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ResolvedIdentity {
    pub identity: PaneIdentity,
    pub confidence: IdentityConfidence,
}

#[derive(Debug, Default, Clone)]
pub struct IdentityResolver;

impl IdentityResolver {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve(&self, raw: &RawPaneInput) -> ResolvedIdentity {
        // Priority: canonical title "{provider}:{instance}:{role}" wins.
        if let Some(parsed) = parse_canonical_title(&raw.title) {
            return ResolvedIdentity {
                identity: PaneIdentity {
                    pane_id: raw.pane_id.clone(),
                    provider: parsed.0,
                    instance: parsed.1,
                    role: parsed.2,
                },
                confidence: IdentityConfidence::High,
            };
        }

        // Fallback: infer provider from pane title first, then from
        // current command, then from recent pane text. This keeps
        // non-canonical titles like "Claude Code" or "Gemini CLI"
        // useful, while still allowing generic bash/node panes to be
        // resolved from their visible output.
        let title_provider = detect_provider(&raw.title);
        let cmd_provider = detect_provider(&raw.current_command);
        let tail_provider = detect_provider(&raw.tail);

        let (provider, confidence) = if title_provider != Provider::Unknown {
            (title_provider, IdentityConfidence::Medium)
        } else if cmd_provider != Provider::Unknown {
            (cmd_provider, IdentityConfidence::Medium)
        } else if tail_provider != Provider::Unknown {
            (tail_provider, IdentityConfidence::Low)
        } else {
            (Provider::Unknown, IdentityConfidence::Unknown)
        };

        ResolvedIdentity {
            identity: PaneIdentity {
                pane_id: raw.pane_id.clone(),
                provider,
                instance: 1,
                role: Role::Unknown,
            },
            confidence,
        }
    }
}

fn parse_canonical_title(title: &str) -> Option<(Provider, u32, Role)> {
    let mut parts = title.trim().split(':');
    let provider_part = parts.next()?;
    let instance_part = parts.next()?;
    let role_part = parts.next()?;
    if parts.next().is_some() {
        return None; // too many segments
    }

    let provider = match provider_part {
        "claude" => Provider::Claude,
        "codex" => Provider::Codex,
        "gemini" => Provider::Gemini,
        "qmonster" => Provider::Qmonster,
        _ => return None,
    };
    let instance = instance_part.parse::<u32>().ok().filter(|n| *n >= 1)?;
    let role = match role_part {
        "main" => Role::Main,
        "review" => Role::Review,
        "research" => Role::Research,
        "monitor" => Role::Monitor,
        _ => return None,
    };
    Some((provider, instance, role))
}

fn detect_provider(s: &str) -> Provider {
    let lower = s.to_lowercase();
    // Check in priority order: qmonster first so the monitor pane does not
    // get mis-classified when its tail mentions other providers.
    if contains_word(&lower, "qmonster") {
        Provider::Qmonster
    } else if contains_word(&lower, "claude") {
        Provider::Claude
    } else if contains_word(&lower, "codex") {
        Provider::Codex
    } else if contains_word(&lower, "gemini") {
        Provider::Gemini
    } else {
        Provider::Unknown
    }
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token == needle)
        || haystack.contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(title: &str, cmd: &str, tail: &str) -> RawPaneInput {
        RawPaneInput {
            pane_id: "%1".into(),
            title: title.into(),
            current_command: cmd.into(),
            tail: tail.into(),
        }
    }

    #[test]
    fn claude_main_title_resolves_high_confidence() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("claude:1:main", "claude", ""));
        assert_eq!(out.identity.provider, Provider::Claude);
        assert_eq!(out.identity.role, Role::Main);
        assert_eq!(out.identity.instance, 1);
        assert_eq!(out.confidence, IdentityConfidence::High);
    }

    #[test]
    fn repeated_provider_instance_is_parsed_from_title() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("claude:3:research", "claude", ""));
        assert_eq!(out.identity.provider, Provider::Claude);
        assert_eq!(out.identity.role, Role::Research);
        assert_eq!(out.identity.instance, 3);
        assert_eq!(out.confidence, IdentityConfidence::High);
    }

    #[test]
    fn qmonster_monitor_pane_detected() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("qmonster:1:monitor", "qmonster", ""));
        assert_eq!(out.identity.provider, Provider::Qmonster);
        assert_eq!(out.identity.role, Role::Monitor);
    }

    #[test]
    fn command_hint_gives_medium_confidence_when_title_missing() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("bash", "codex", ""));
        assert_eq!(out.identity.provider, Provider::Codex);
        // Without title convention, role unknown and confidence not High.
        assert_eq!(out.identity.role, Role::Unknown);
        assert!(matches!(
            out.confidence,
            IdentityConfidence::Medium | IdentityConfidence::Low
        ));
    }

    #[test]
    fn non_canonical_title_hint_gives_medium_confidence() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("Claude Code", "node", ""));
        assert_eq!(out.identity.provider, Provider::Claude);
        assert_eq!(out.confidence, IdentityConfidence::Medium);
    }

    #[test]
    fn tail_text_hint_can_resolve_when_title_and_command_are_generic() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw(
            "bash",
            "bash",
            "OpenAI Codex is waiting for confirmation on the current patch",
        ));
        assert_eq!(out.identity.provider, Provider::Codex);
        assert_eq!(out.confidence, IdentityConfidence::Low);
    }

    #[test]
    fn tail_hint_only_gives_low_confidence() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("bash", "bash", "gemini version 1.2.3"));
        assert_eq!(out.identity.provider, Provider::Gemini);
        assert_eq!(out.confidence, IdentityConfidence::Low);
    }

    #[test]
    fn no_hints_yields_unknown() {
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("bash", "bash", "no signal here"));
        assert_eq!(out.identity.provider, Provider::Unknown);
        assert_eq!(out.identity.role, Role::Unknown);
        assert_eq!(out.confidence, IdentityConfidence::Unknown);
    }

    #[test]
    fn qmonster_takes_priority_over_claude_substring() {
        // Guard: the monitor pane's tail can mention 'claude' — still qmonster.
        let r = IdentityResolver::new();
        let out = r.resolve(&raw("qmonster:1:monitor", "qmonster", "claude:1:main idle"));
        assert_eq!(out.identity.provider, Provider::Qmonster);
    }
}
