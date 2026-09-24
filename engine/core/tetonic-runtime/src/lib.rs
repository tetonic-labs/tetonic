//! Mandatory production runtime assembly (Architecture Consolidation II).

pub mod action_broker;
pub mod approval;
pub mod brain;
pub mod composite_adapter;
pub mod stream_adapter;
pub mod websocket_adapter;
mod assembly;
mod audit;
mod build;
mod capability_store;
mod executor;
mod policy;

pub use action_broker::RuntimeActionBroker;
pub use approval::{ApprovalKind, ProductionApproval};
pub use assembly::{
    wire_kernel_capability_helpers, AgentAssemblyParts, AssemblyError, AssemblyMode, EngineRuntime,
    TestRuntime,
};
pub use audit::NullAudit;
pub use brain::{DualProcessBrain, ScriptedBrain, SingleModelBrain};
pub use build::{base_agent_config, lsp_enabled_for_workspace, AgentConfigInput};
pub use capability_store::InMemoryCapabilityStore;
pub use executor::LocalAgentAttemptExecutor;
pub use policy::load_policy_engine;
pub use stream_adapter::{StreamMessage, StreamWorldAdapter};
pub use composite_adapter::CompositeWorldAdapter;
