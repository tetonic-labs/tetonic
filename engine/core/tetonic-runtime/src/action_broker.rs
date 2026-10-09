use std::sync::Arc;

use async_trait::async_trait;
use tetonic_core::{ApprovalHook, ApprovalRequest};
use tetonic_domain::sinks::{ActionBroker, PolicyEvaluator};
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

/// One host-assembled agent's approval callback over the shared policy and
/// capability store. Managed attempts are assigned after assembly; a scoped
/// callback validates that actual attempt instead of relying on legacy registry
/// registration timing.
struct BoundApprovalBroker {
    inner: Arc<RuntimeActionBroker>,
    approval: ApprovalHook,
    restrictions: Option<Arc<dyn PolicyEvaluator>>,
}

#[async_trait]
impl ActionBroker for BoundApprovalBroker {
    async fn evaluate_and_issue(
        &self,
        action: &ProposedAction,
    ) -> Result<IssuedCapability, CapabilityError> {
        self.inner
            .evaluate_with_approval(action, Some(&self.approval), self.restrictions.as_deref())
            .await
    }
}

impl RuntimeActionBroker {
    pub fn with_approval(self: &Arc<Self>, approval: ApprovalHook) -> Arc<dyn ActionBroker> {
        Arc::new(BoundApprovalBroker {
            inner: self.clone(),
            approval,
            restrictions: None,
        })
    }
    /// Extra restrictions can narrow host authority, never broaden it.
    pub fn with_restrictions(
        self: &Arc<Self>,
        approval: ApprovalHook,
        restrictions: Arc<dyn PolicyEvaluator>,
    ) -> Arc<dyn ActionBroker> {
        Arc::new(BoundApprovalBroker {
            inner: self.clone(),
            approval,
            restrictions: Some(restrictions),
        })
    }
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
        self.evaluate_with_approval(action, None, None).await
    }
}

impl RuntimeActionBroker {
    async fn evaluate_with_approval(
        &self,
        action: &ProposedAction,
        approval: Option<&ApprovalHook>,
        restrictions: Option<&dyn PolicyEvaluator>,
    ) -> Result<IssuedCapability, CapabilityError> {
        let action = prepare_proposed_action(action.clone());
        let mut outcome = self.policy_engine.evaluate_action(&action);
        if let Some(policy) = restrictions {
            let extra = policy.evaluate(&action);
            if outcome.decision.allowed() {
                outcome.decision = extra.decision;
            }
            if extra.approval == ApprovalRequirement::Interactive {
                outcome.approval = extra.approval;
            }
        }

        if !outcome.decision.allowed() {
            let reason = match &outcome.decision {
                PolicyDecision::Deny { reason } => reason.clone(),
                PolicyDecision::Allow => "denied".into(),
            };
            return Err(CapabilityError::PolicyDenied(reason));
        }

        let mut approved = false;
        if outcome.approval == ApprovalRequirement::Interactive {
            let hook = approval.cloned().or_else(|| {
                action
                    .attempt_id
                    .as_ref()
                    .and_then(|att| self.attempt_approvals.lock().unwrap().get(att).cloned())
            });
            if let Some(hook) = hook {
                let req = ApprovalRequest {
                    call_id: action.action_id.to_string(),
                    kind: format!("{:?}", action.kind),
                    tool: action
                        .parameters
                        .tool_name
                        .clone()
                        .unwrap_or_else(|| broker_tool_name(&action.kind)),
                    args: serde_json::to_value(&action.parameters).unwrap_or_default(),
                    attempt_id: action.attempt_id.as_ref().map(|a| a.to_string()),
                    ..Default::default()
                };
                approved = hook(req).await;
                if !approved {
                    return Err(CapabilityError::ApprovalRequired);
                }
            } else {
                return Err(CapabilityError::ApprovalRequired);
            }
        }

        // A person may tighten permissions while this action awaits approval.
        if let Some(policy) = restrictions {
            let current = policy.evaluate(&action);
            if let PolicyDecision::Deny { reason } = current.decision {
                return Err(CapabilityError::PolicyDenied(reason));
            }
            if current.approval == ApprovalRequirement::Interactive && !approved {
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

#[cfg(test)]
mod capability_policy_tests {
    use super::*;
    use tetonic_domain::{ActionKind, ActionPolicyOutcome};
    use tetonic_policy::capabilities::{capability_outcome, AutonomyTier, CapabilityPolicy};
    struct Restriction(AutonomyTier);
    impl PolicyEvaluator for Restriction {
        fn evaluate(&self, action: &ProposedAction) -> ActionPolicyOutcome {
            capability_outcome(
                &[CapabilityPolicy {
                    tier: self.0,
                    ..Default::default()
                }],
                &action.kind,
            )
        }
    }
    fn action(kind: ActionKind) -> ProposedAction {
        ProposedAction {
            action_id: tetonic_domain::ActionId::new("action"), session_id: tetonic_domain::SessionId::new("session"),
            run_id:None,task_id:None,attempt_id:None,agent_id:None,workspace_version:None,
            data_class:tetonic_domain::DataClass::RepositorySource,kind,
            parameters:serde_json::from_value(serde_json::json!({"digest":"","arguments":[],"schema_version":1,"tool_arguments":{"path":"receipt.txt"}})).unwrap(),
            requested_capabilities:Default::default(),trace_context:Default::default(),
        }
    }
    #[tokio::test]
    async fn automatic_cannot_relax_host_denials_or_mandatory_shell_review() {
        let host = Arc::new(PolicyEngine::default());
        host.set_mutations_allowed(false);
        let broker = Arc::new(RuntimeActionBroker::new(
            host,
            Arc::new(InMemoryCapabilityStore::new()),
        ));
        let deny: ApprovalHook = Arc::new(|_| Box::pin(async { false }));
        let scoped = broker.with_restrictions(deny, Arc::new(Restriction(AutonomyTier::Automatic)));
        assert!(matches!(
            scoped
                .evaluate_and_issue(&action(ActionKind::WriteFile))
                .await,
            Err(CapabilityError::PolicyDenied(_))
        ));
        let mut shell = action(ActionKind::ExecuteShell);
        shell.parameters.script_bytes = Some(b"echo test".to_vec());
        assert!(matches!(
            scoped.evaluate_and_issue(&shell).await,
            Err(CapabilityError::ApprovalRequired)
        ));
    }
    #[tokio::test]
    async fn ask_requires_approval_and_deny_never_calls_an_approval_hook() {
        let broker = Arc::new(RuntimeActionBroker::new(
            Arc::new(PolicyEngine::default()),
            Arc::new(InMemoryCapabilityStore::new()),
        ));
        assert!(matches!(
            broker
                .evaluate_with_approval(
                    &action(ActionKind::WriteFile),
                    None,
                    Some(&Restriction(AutonomyTier::ReviewChanges))
                )
                .await,
            Err(CapabilityError::ApprovalRequired)
        ));
        let unexpected: ApprovalHook = Arc::new(|_| {
            Box::pin(async { panic!("denied capabilities must not be offered for approval") })
        });
        assert!(matches!(
            broker
                .with_restrictions(unexpected, Arc::new(Restriction(AutonomyTier::ReadOnly)))
                .evaluate_and_issue(&action(ActionKind::WriteFile))
                .await,
            Err(CapabilityError::PolicyDenied(_))
        ));
    }
}
