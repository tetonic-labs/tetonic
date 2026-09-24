//! Single-agent tool loop (Phase A).
//!
//! Drives the conversation: advertise tools → call the model → execute the
//! returned tool calls → feed results (including verbatim errors, for
//! self-correction) back → repeat until the model calls `finish`, answers with
//! no tool calls, or the effort cap is hit.
//!
//! The router/swarm layer is additive on top of this loop, not a rewrite of it.

pub mod agent;
mod config;
mod context;
mod conversation;
pub mod demuxer;
mod error;
pub mod filter;
mod hooks;
mod inference_binding;
mod monitor;
mod step;
mod tokenizer;
mod turn;

pub use agent::Agent;
pub use config::AgentConfig;
pub use context::ContextReport;
pub use conversation::Conversation;
pub use demuxer::{DemuxedChunk, TokenDemuxer};
pub use error::AgentError;
pub use filter::{FilterDecision, SensoryFilter};
pub use hooks::{
    AbortStaged, ApprovalHook, ApprovalRequest, AuditSink, CaptureWorkspaceVersion,
    ConfinementWarning, PostEditSnapshot, ResolveUnderRoot, SpawnHook, SpawnRequest,
};
pub use inference_binding::{AgentInferenceBinding, InvalidInferenceBinding};
pub use step::Step;
pub mod checkpoint;
pub use checkpoint::CheckpointManager;
pub use tetonic_domain::checkpoint::{AgentStateCheckpoint, AgentStateCheckpointHeader, CheckpointError};
pub use tetonic_domain::engine_config::{
    EngineConfig, EngineConfigError, InferenceConfig, InferenceProviderKind, NodeConfig,
    NodeMode, StorageConfig, StorageMode, TelemetryConfig, TelemetrySinkKind,
};
pub use tokenizer::{ExactTokenizer, HeuristicTokenizer, Tokenizer};
pub use turn::{validate_transition, TurnOpsEvent, TurnOpsHook, TurnState};
