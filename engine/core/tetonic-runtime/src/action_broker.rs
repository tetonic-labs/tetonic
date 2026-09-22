use std::sync::Arc;

use async_trait::async_trait;
use tetonic_core::{ApprovalHook, ApprovalRequest};
use tetonic_domain::sinks::ActionBroker;
use tetonic_domain::{
    prepare_proposed_action, ApprovalRequirement, CapabilityError, CapabilityId, IssuedCapability,
    PolicyDecision, ProposedAction,
};
use tetonic_policy::PolicyEngine;

use crate::capability_store::InMemoryCapabilityStore;

pub struct RuntimeActionBroker {
    policy_engine: Arc<PolicyEngine>,
    capability_store: Arc<InMemoryCapabilityStore>,
    attempt_approvals:
        std::sync::Mutex<std::collections::HashMap<tetonic_domain::AttemptId, ApprovalHook>>,
}

impl RuntimeActionBroker {
    pub fn new(
        policy_engine: Arc<PolicyEngine>,
        capability_store: Arc<InMemoryCapabilityStore>,
    ) -> Self {
        Self {
            policy_engine,
            capability_store,
            attempt_approvals: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn capability_store(&self) -> Arc<InMemoryCapabilityStore> {
        self.capability_store.clone()
    }

    pub fn register_attempt_approval(
        &self,
        attempt_id: tetonic_domain::AttemptId,
        hook: ApprovalHook,
    ) {
        self.attempt_approvals
            .lock()
            .unwrap()
            .insert(attempt_id, hook);
    }

    pub fn unregister_attempt_approval(&self, attempt_id: &tetonic_domain::AttemptId) {
        self.attempt_approvals.lock().unwrap().remove(attempt_id);
    }

    pub fn has_attempt_approval(&self, attempt_id: &tetonic_domain::AttemptId) -> bool {
        self.attempt_approvals
            .lock()
            .unwrap()
            .contains_key(attempt_id)
    }
}

#[async_trait]
impl ActionBroker for RuntimeActionBroker {
    async fn evaluate_and_issue(
        &self,
        action: &ProposedAction,
    ) -> Result<IssuedCapability, CapabilityError> {
        let action = prepare_proposed_action(action.clone());
        let outcome = self.policy_engine.evaluate_action(&action);

        if !outcome.decision.allowed() {
            let reason = match &outcome.decision {
                PolicyDecision::Deny { reason } => reason.clone(),
                PolicyDecision::Allow => "denied".into(),
            };
            return Err(CapabilityError::PolicyDenied(reason));
        }

        if outcome.approval == ApprovalRequirement::Interactive {
            let hook = action
                .attempt_id
                .as_ref()
                .and_then(|att| self.attempt_approvals.lock().unwrap().get(att).cloned());
            if let Some(hook) = hook {
                let req = ApprovalRequest {
                    call_id: action.action_id.to_string(),
                    kind: format!("{:?}", action.kind),
                    tool: broker_tool_name(&action.kind),
                    args: serde_json::to_value(&action.parameters).unwrap_or_default(),
                    attempt_id: action.attempt_id.as_ref().map(|a| a.to_string()),
                    ..Default::default()
                };
                let approved = hook(req).await;
                if !approved {
                    return Err(CapabilityError::ApprovalRequired);
                }
            } else {
                return Err(CapabilityError::ApprovalRequired);
            }
        }

        let capability_id = CapabilityId::new(format!("cap_{}", uuid::Uuid::new_v4().simple()));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cap = IssuedCapability {
            capability_id,
            session_id: action.session_id.clone(),
            run_id: action.run_id.clone(),
            task_id: action.task_id.clone(),
            attempt_id: action.attempt_id.clone(),
            agent_id: action.agent_id.clone(),
            action_kind: action.kind.clone(),
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: action.workspace_version.clone(),
            data_classification: action.data_class,
            issuance_timestamp: now,
            expiration: now + 300,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: tetonic_policy::policy_version().to_string(),
            approval_record_id: Some(action.action_id.to_string()),
            revoked: false,
        };
        self.capability_store.register(cap.clone())?;
        Ok(cap)
    }
}

fn broker_tool_name(kind: &tetonic_domain::ActionKind) -> String {
    match kind {
        tetonic_domain::ActionKind::ExecuteShell => "run_shell".into(),
        tetonic_domain::ActionKind::WriteFile => "write_file".into(),
        tetonic_domain::ActionKind::ReadFile => "read_file".into(),
        tetonic_domain::ActionKind::ExecuteProcess => "verify".into(),
        tetonic_domain::ActionKind::StartInternalService => "lsp".into(),
        other => format!("{other:?}"),
    }
}
