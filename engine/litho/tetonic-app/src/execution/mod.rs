//! Application adapters around the existing managed execution authority.
//!
//! Registered revision preparation and assembly live in `resources::registered`.
//! `tetonic-run` owns admission, attempts, leases, cancellation and finalization;
//! these adapters supply host effects and projections, never a second lifecycle.
pub mod attestation;
pub mod audit;
pub(crate) mod finalization;
mod observer;
mod service;
pub mod workspace_hooks;
pub use service::{DefaultRunService, FinalizationEffectDriver, FinalizationPolicy, RunService};
