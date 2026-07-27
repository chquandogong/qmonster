//! herdr backend — acquisition via the `herdr` CLI socket-API surface.
//!
//! Mirrors the `tmux/` module contract: this module must never know
//! about providers or signals (r2 boundary). It emits
//! `RawPaneSnapshot`s only; the herdr `agent` string is passed through
//! opaquely as `agent_hint` and interpreted in `domain/identity.rs`.

// dead_code allow is temporary: consumers land with source.rs in the
// next commit of this branch.
#[allow(dead_code)]
pub(crate) mod commands;
pub mod types;
