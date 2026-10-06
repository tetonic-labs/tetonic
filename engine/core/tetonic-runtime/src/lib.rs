//! Mandatory production runtime assembly (Architecture Consolidation II).

pub mod action_broker;
pub mod approval;
mod assembly;
mod audit;
pub mod brain;
mod build;
mod capability_store;
pub mod composite_adapter;
mod executor;
mod policy;
pub mod stream_adapter;
pub mod websocket_adapter;

pub use action_broker::RuntimeActionBroker;
pub use approval::{ApprovalKind, ProductionApproval};
pub use assembly::{
    wire_kernel_capability_helpers, AgentAssemblyParts, AssemblyError, AssemblyMode, EngineRuntime,
    TestRuntime,
};
pub use audit::NullAudit;
pub use brain::SingleModelBrain;
pub use build::{base_agent_config, lsp_enabled_for_workspace, AgentConfigInput};
pub use capability_store::InMemoryCapabilityStore;
pub use composite_adapter::CompositeWorldAdapter;
pub use executor::{LocalAgentAttemptExecutor, LocalWorldAttemptExecutor};
pub use policy::load_policy_engine;
pub use stream_adapter::{StreamMessage, StreamWorldAdapter};
