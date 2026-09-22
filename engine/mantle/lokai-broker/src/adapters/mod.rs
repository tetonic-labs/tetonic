//! Execution target adapters (M6-1).

pub mod gated_process;
pub mod inference;
pub mod process;

pub use gated_process::BrokerGatedProcessBroker;
pub use inference::{redact_outbound, BrokerInferenceProvider, InferenceTargetAdapter};
pub use process::LocalProcessTargetAdapter;
