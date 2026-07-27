//! herdr backend — acquisition via the `herdr` CLI socket-API surface.
//!
//! Mirrors the `tmux/` module contract: this module must never know
//! about providers or signals (r2 boundary). It emits
//! `RawPaneSnapshot`s only; the herdr `agent` string is passed through
//! opaquely as `agent_hint` and interpreted in `domain/identity.rs`.

pub(crate) mod commands;
pub mod source;
pub mod types;

pub use source::HerdrSource;
