//! Slice 4: idle-state regression suite against captured/synthesized
//! real-tail fixtures. Each fixture asserts the matching adapter's
//! classify_idle returns the expected variant.

use qmonster::adapters::ParserContext;
use qmonster::adapters::ProviderParser;
use qmonster::adapters::claude::ClaudeAdapter;
use qmonster::adapters::codex::CodexAdapter;
use qmonster::adapters::common::PaneTailHistory;
use qmonster::adapters::gemini::GeminiAdapter;
use qmonster::domain::identity::{
    IdentityConfidence, PaneIdentity, Provider, ResolvedIdentity, Role,
};
use qmonster::domain::signal::IdleCause;
use qmonster::policy::claude_settings::ClaudeSettings;
use qmonster::policy::pricing::PricingTable;

fn id(p: Provider, r: Role) -> ResolvedIdentity {
    ResolvedIdentity {
        identity: PaneIdentity {
            provider: p,
            instance: 1,
            role: r,
            pane_id: "%1".into(),
        },
        confidence: IdentityConfidence::High,
    }
}

const CLAUDE_IDLE_CURSOR: &str = include_str!("fixtures/real/claude_idle_cursor.txt");
const CLAUDE_LIMIT_HIT: &str = include_str!("fixtures/real/claude_limit_hit.txt");
const CODEX_IDLE_CURSOR: &str = include_str!("fixtures/real/codex_idle_cursor.txt");
const CODEX_LIMIT_HIT: &str = include_str!("fixtures/real/codex_limit_hit.txt");
const GEMINI_IDLE: &str = include_str!("fixtures/real/gemini_idle.txt");
const GEMINI_QUOTA_FULL: &str = include_str!("fixtures/real/gemini_quota_full.txt");

#[test]
fn claude_idle_cursor_fixture_classifies_as_work_complete() {
    let id = id(Provider::Claude, Role::Main);
    let pricing = PricingTable::empty();
    let settings = ClaudeSettings::empty();
    let history = PaneTailHistory::empty();
    let c = ParserContext {
        identity: &id,
        tail: CLAUDE_IDLE_CURSOR,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
    };
    let s = ClaudeAdapter.parse(&c);
    assert_eq!(s.idle_state, Some(IdleCause::WorkComplete));
}

#[test]
fn claude_limit_hit_fixture_classifies_as_limit_hit() {
    let id = id(Provider::Claude, Role::Main);
    let pricing = PricingTable::empty();
    let settings = ClaudeSettings::empty();
    let history = PaneTailHistory::empty();
    let c = ParserContext {
        identity: &id,
        tail: CLAUDE_LIMIT_HIT,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
    };
    let s = ClaudeAdapter.parse(&c);
    assert_eq!(s.idle_state, Some(IdleCause::LimitHit));
}

#[test]
fn codex_idle_cursor_fixture_classifies_as_work_complete() {
    let id = id(Provider::Codex, Role::Review);
    let pricing = PricingTable::empty();
    let settings = ClaudeSettings::empty();
    let history = PaneTailHistory::empty();
    let c = ParserContext {
        identity: &id,
        tail: CODEX_IDLE_CURSOR,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
    };
    let s = CodexAdapter.parse(&c);
    assert_eq!(s.idle_state, Some(IdleCause::WorkComplete));
}

#[test]
fn codex_limit_hit_fixture_classifies_as_limit_hit() {
    let id = id(Provider::Codex, Role::Review);
    let pricing = PricingTable::empty();
    let settings = ClaudeSettings::empty();
    let history = PaneTailHistory::empty();
    let c = ParserContext {
        identity: &id,
        tail: CODEX_LIMIT_HIT,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
    };
    let s = CodexAdapter.parse(&c);
    assert_eq!(s.idle_state, Some(IdleCause::LimitHit));
}

#[test]
fn gemini_idle_fixture_classifies_as_work_complete() {
    let id = id(Provider::Gemini, Role::Research);
    let pricing = PricingTable::empty();
    let settings = ClaudeSettings::empty();
    let history = PaneTailHistory::empty();
    let c = ParserContext {
        identity: &id,
        tail: GEMINI_IDLE,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
    };
    let s = GeminiAdapter.parse(&c);
    assert_eq!(s.idle_state, Some(IdleCause::WorkComplete));
}

#[test]
fn gemini_quota_full_fixture_classifies_as_limit_hit() {
    let id = id(Provider::Gemini, Role::Research);
    let pricing = PricingTable::empty();
    let settings = ClaudeSettings::empty();
    let history = PaneTailHistory::empty();
    let c = ParserContext {
        identity: &id,
        tail: GEMINI_QUOTA_FULL,
        pricing: &pricing,
        claude_settings: &settings,
        history: &history,
        pane_pid: None, // F-1: test fixture; production wires via parse_ctx in event_loop.rs
    };
    let s = GeminiAdapter.parse(&c);
    assert_eq!(s.idle_state, Some(IdleCause::LimitHit));
}
