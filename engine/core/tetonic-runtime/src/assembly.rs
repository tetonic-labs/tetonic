use std::sync::Arc;

use tetonic_core::{
    Agent, AuditSink, CaptureWorkspaceVersion, PostEditSnapshot, ResolveUnderRoot, SpawnHook,
};
use tetonic_domain::CapabilityError;
use tetonic_domain::ContextCompiler;
use tetonic_policy::PolicyEngine;
use thiserror::Error;

use crate::approval::{ApprovalKind, ProductionApproval};
use crate::capability_store::InMemoryCapabilityStore;

#[derive(Debug, Error)]
pub enum AssemblyError {
    #[error("production assembly requires an audit sink")]
    MissingAudit,
    #[error("production assembly requires an approval hook")]
    MissingApproval,
    #[error("capability persist failed: {0}")]
    CapabilityPersist(CapabilityError),
}

/// Session vs ephemeral assembly invariants (AC2-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblyMode {
    /// Daemon/editor or CLI with audit DB — persisted audit + host approval required.
    Session,
    /// CLI one-shot without store — `NullAudit` and verify-only approval allowed.
    CliEphemeral,
}

/// Required parts for [`EngineRuntime::assemble_agent`].
///
/// Composition builds `Agent` (and `Tools`) before this call. Runtime attaches
/// policy / brokers / compiler and capability hooks — it does not take `Tools`.
pub struct AgentAssemblyParts {
    pub agent: Agent,
    pub audit: Box<dyn AuditSink>,
    pub approval: ProductionApproval,
    pub spawn: Option<SpawnHook>,
    /// When set, replaces the default ProcessExecutor ProcessBroker (M6-1 ComputeBroker gate).
    pub process_broker: Option<Arc<dyn tetonic_domain::sinks::ProcessBroker>>,
    /// Context compiler supplied by composition; neutral runtime leaves this `None`.
    pub context_compiler: Option<Arc<dyn ContextCompiler>>,
    pub post_edit_snapshot: PostEditSnapshot,
    pub resolve_under_root: ResolveUnderRoot,
    pub capture_workspace_version: CaptureWorkspaceVersion,
}

/// Production runtime — mandatory policy and action broker; validates assembly.
pub struct EngineRuntime {
    policy: Arc<PolicyEngine>,
    action_broker: Arc<crate::action_broker::RuntimeActionBroker>,
    capability_store: Arc<InMemoryCapabilityStore>,
    artifact_store: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
}

impl EngineRuntime {
    pub fn new(
        policy: Arc<PolicyEngine>,
        _approval_hook: Option<tetonic_core::ApprovalHook>,
        artifact_store: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    ) -> Self {
        Self::new_with_capability_store(policy, _approval_hook, artifact_store, None)
            .expect("memory-only capability store")
    }

    /// Production runtime; when `capability_db` is set, capabilities persist and are
    /// revoked on construction (R6-1 restart semantics).
    pub fn new_with_capability_store(
        policy: Arc<PolicyEngine>,
        _approval_hook: Option<tetonic_core::ApprovalHook>,
        artifact_store: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
        capability_db: Option<Arc<lokai_memory::SharedStore>>,
    ) -> Result<Self, CapabilityError> {
        let capability_store = Arc::new(match capability_db {
            Some(store) => InMemoryCapabilityStore::with_durable(store)?,
            None => InMemoryCapabilityStore::new(),
        });
        let action_broker = Arc::new(crate::action_broker::RuntimeActionBroker::new(
            policy.clone(),
            capability_store.clone(),
        ));
        Ok(Self {
            policy,
            action_broker,
            capability_store,
            artifact_store,
        })
    }

    pub fn policy(&self) -> &Arc<PolicyEngine> {
        &self.policy
    }

    pub fn action_broker(&self) -> &Arc<crate::action_broker::RuntimeActionBroker> {
        &self.action_broker
    }

    pub fn capability_store(&self) -> &Arc<InMemoryCapabilityStore> {
        &self.capability_store
    }

    pub fn artifact_store(&self) -> &Arc<dyn tetonic_domain::artifact::ArtifactStore> {
        &self.artifact_store
    }

    /// Build a production [`Agent`] with mandatory audit, approval, and policy.
    /// Composition supplies a prebuilt `Agent`. Runtime does not take `Tools`.
    pub fn assemble_agent(
        &self,
        mode: AssemblyMode,
        parts: AgentAssemblyParts,
    ) -> Result<Agent, AssemblyError> {
        if mode == AssemblyMode::Session {
            if !parts.audit.audit_persists() {
                return Err(AssemblyError::MissingAudit);
            }
            if parts.approval.kind() == ApprovalKind::AllowAll {
                return Err(AssemblyError::MissingApproval);
            }
        }

        let approval = parts.approval.into_hook();
        let mut agent = parts
            .agent
            .with_policy(self.policy.clone())
            .with_action_broker(self.action_broker.clone())
            .with_audit(parts.audit)
            .with_approval(approval);
        if let Some(process_broker) = parts.process_broker {
            agent = agent.with_process_broker(process_broker);
        }
        if let Some(hook) = parts.spawn {
            agent = agent.with_spawn(hook);
        }
        if let Some(compiler) = parts.context_compiler {
            agent = agent.with_context_compiler(compiler);
        }
        Ok(wire_kernel_capability_helpers(
            agent,
            parts.post_edit_snapshot,
            parts.resolve_under_root,
            parts.capture_workspace_version,
        ))
    }
}

/// Install snapshot / jail / workspace-version helpers supplied by composition.
/// Production `lokai-runtime` does not import capability crates.
pub fn wire_kernel_capability_helpers(
    agent: Agent,
    post_edit_snapshot: PostEditSnapshot,
    resolve_under_root: ResolveUnderRoot,
    capture_workspace_version: CaptureWorkspaceVersion,
) -> Agent {
    agent
        .with_post_edit_snapshot(post_edit_snapshot)
        .with_resolve_under_root(resolve_under_root)
        .with_capture_workspace_version(capture_workspace_version)
}

/// Test-only assembly. Callers build `Agent` themselves (no `Tools` parameter).
pub struct TestRuntime;

impl TestRuntime {
    pub fn assemble_agent(agent: Agent) -> Agent {
        agent
    }
}

#[cfg(test)]
#[path = "assembly_tests.rs"]
mod tests;
