//! Qmonster library entry — Phase 1 observe-first MVP.
//!
//! Pipeline: tmux::RawPaneSnapshot -> domain::IdentityResolver
//!        -> adapters::ProviderParser -> domain::SignalSet
//!        -> policy::Engine -> app::EffectRunner
//!        -> ui::ViewModel + store::EventSink
//!
//! See docs/ai/ARCHITECTURE.md and .docs/final/…r2.md §5 for the
//! non-negotiable boundaries enforced here.

pub mod adapters;
pub mod app;
pub mod domain;
pub mod notify;
pub mod policy;
pub mod store;
pub mod tmux;
pub mod ui;
