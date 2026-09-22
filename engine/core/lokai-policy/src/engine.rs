//! Unified policy engine (D1) — remote inference, data class, tool gates.

use std::sync::Mutex;

use lokai_domain::ProjectPlacementPolicy;
use lokai_domain::{
    ActionKind, ActionPolicyOutcome, ApprovalRequirement, DataClass, PolicyDecision,
    PolicyEvaluator, ProposedAction,
};

use crate::classify::restrict_data_class;
use crate::fabric::{Destination, FabricJobDraft, PolicyContext};
use crate::mode::PolicyMode;
use crate::shell::shell_command_blocked;

#[derive(Debug, Clone)]
pub struct PolicySettings {
    pub mode: PolicyMode,
    pub verify_allowed: bool,
    pub mutations_allowed: bool,
    pub placement: ProjectPlacementPolicy,
}

impl Default for PolicySettings {
    fn default() -> Self {
        Self::default_settings()
    }
}

#[derive(Debug)]
pub struct PolicyEngine {
    mode: Mutex<PolicyMode>,
    verify_allowed: Mutex<bool>,
    mutations_allowed: Mutex<bool>,
    placement: Mutex<ProjectPlacementPolicy>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::from_settings(PolicySettings::default_settings())
    }
}

impl PolicySettings {
    pub fn default_settings() -> Self {
        Self {
            mode: PolicyMode::EstateStub,
            verify_allowed: true,
            mutations_allowed: true,
            placement: ProjectPlacementPolicy::default(),
        }
    }
}

impl PolicyEngine {
    pub fn new(mode: PolicyMode) -> Self {
        Self::from_settings(PolicySettings {
            mode,
            ..PolicySettings::default_settings()
        })
    }

    pub fn from_settings(settings: PolicySettings) -> Self {
        Self {
            mode: Mutex::new(settings.mode),
            verify_allowed: Mutex::new(settings.verify_allowed),
            mutations_allowed: Mutex::new(settings.mutations_allowed),
            placement: Mutex::new(settings.placement),
        }
    }

    pub fn settings(&self) -> PolicySettings {
        PolicySettings {
            mode: self.mode(),
            verify_allowed: self.verify_allowed(),
            mutations_allowed: self.mutations_allowed(),
            placement: self.project_placement_policy(),
        }
    }

    pub fn verify_allowed(&self) -> bool {
        *self
            .verify_allowed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_verify_allowed(&self, allowed: bool) {
        *self
            .verify_allowed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = allowed;
    }

    pub fn mutations_allowed(&self) -> bool {
        *self
            .mutations_allowed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_mutations_allowed(&self, allowed: bool) {
        *self
            .mutations_allowed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = allowed;
    }

    pub fn project_placement_policy(&self) -> ProjectPlacementPolicy {
        self.placement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_project_placement_policy(&self, policy: ProjectPlacementPolicy) {
        *self.placement.lock().unwrap_or_else(|e| e.into_inner()) = policy;
    }

    pub fn mode(&self) -> PolicyMode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_mode(&self, mode: PolicyMode) {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner()) = mode;
    }

    pub fn check_remote_inference(
        &self,
        ctx: &PolicyContext,
        job: &FabricJobDraft,
    ) -> PolicyDecision {
        let effective = restrict_data_class(ctx.session_data_class, job.data_class);
        let effective_job = FabricJobDraft {
            data_class: effective,
            disclosure_tier: job.disclosure_tier,
            destination: job.destination.clone(),
            is_circle_job: job.is_circle_job,
        };
        match effective_job.destination {
            Destination::Local => PolicyDecision::Allow,
            Destination::CirclePeer { .. } => {
                if self.mode() != PolicyMode::Full || ctx.mode != PolicyMode::Full {
                    return PolicyDecision::Deny {
                        reason: "circle peer routing requires PolicyMode::Full".into(),
                    };
                }
                if let d @ PolicyDecision::Deny { .. } = self.check_disclosure_tier(&effective_job)
                {
                    return d;
                }
                self.check_data_class(effective, &effective_job.destination)
            }
            Destination::EstateWorker { .. } => {
                if effective_job.is_circle_job {
                    if self.mode() != PolicyMode::Full || ctx.mode != PolicyMode::Full {
                        return PolicyDecision::Deny {
                            reason: "circle jobs require PolicyMode::Full".into(),
                        };
                    }
                    if let d @ PolicyDecision::Deny { .. } =
                        self.check_disclosure_tier(&effective_job)
                    {
                        return d;
                    }
                }
                self.check_data_class(effective, &effective_job.destination)
            }
        }
    }

    fn check_disclosure_tier(&self, job: &FabricJobDraft) -> PolicyDecision {
        if !job.is_circle_job {
            return PolicyDecision::Allow;
        }
        match job.disclosure_tier {
            lokai_domain::DisclosureTier::MetadataOnly => PolicyDecision::Deny {
                reason: "circle jobs require disclosure_tier >= summary".into(),
            },
            _ => PolicyDecision::Allow,
        }
    }

    pub fn check_data_class(&self, class: DataClass, dest: &Destination) -> PolicyDecision {
        let mode = self.mode();
        match (class, dest) {
            (DataClass::Secret, Destination::Local) => PolicyDecision::Allow,
            (DataClass::Secret, _) => PolicyDecision::Deny {
                reason: "secret data class never leaves coordinator".into(),
            },
            (
                DataClass::RepositorySource,
                Destination::Local | Destination::EstateWorker { .. },
            ) => PolicyDecision::Allow,
            (DataClass::RepositorySource, Destination::CirclePeer { .. }) => {
                if mode == PolicyMode::Full {
                    PolicyDecision::Allow
                } else {
                    PolicyDecision::Deny {
                        reason: "repository_source on circle peers requires PolicyMode::Full"
                            .into(),
                    }
                }
            }
            (DataClass::SensitiveSource, Destination::Local) => PolicyDecision::Allow,
            (DataClass::SensitiveSource, Destination::EstateWorker { .. }) => PolicyDecision::Allow,
            (DataClass::SensitiveSource, Destination::CirclePeer { .. }) => {
                if mode == PolicyMode::Full {
                    PolicyDecision::Allow
                } else {
                    PolicyDecision::Deny {
                        reason: "sensitive_source routing requires PolicyMode::Full".into(),
                    }
                }
            }
            (DataClass::Public, Destination::Local) => PolicyDecision::Allow,
            (DataClass::Public, Destination::EstateWorker { .. }) => PolicyDecision::Allow,
            (DataClass::Public, Destination::CirclePeer { .. }) => {
                if mode == PolicyMode::Full {
                    PolicyDecision::Allow
                } else {
                    PolicyDecision::Deny {
                        reason: "public routing to circle peers requires PolicyMode::Full".into(),
                    }
                }
            }
        }
    }

    pub fn evaluate_action(&self, action: &ProposedAction) -> ActionPolicyOutcome {
        let _ctx = PolicyContext {
            mode: self.mode(),
            session_data_class: action.data_class,
        };
        let decision = match &action.kind {
            ActionKind::ExecuteProcess => {
                // If it's a verify process, check verify_allowed
                if action.parameters.process_class
                    == Some(lokai_domain::execution::ProcessClass::BuildVerification)
                    && !self.verify_allowed()
                {
                    PolicyDecision::deny("verify disabled by policy")
                } else {
                    PolicyDecision::Allow
                }
            }
            ActionKind::ExecuteShell => {
                let cmd = action
                    .parameters
                    .script_bytes
                    .as_deref()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or_default();
                if let Some(reason) = shell_command_blocked(cmd) {
                    PolicyDecision::deny(format!("shell command blocked: {reason}"))
                } else {
                    PolicyDecision::Allow
                }
            }
            ActionKind::WriteFile | ActionKind::DeleteFile if !self.mutations_allowed() => {
                PolicyDecision::deny("repository mutations disabled by policy")
            }
            ActionKind::Unknown => PolicyDecision::deny("unknown action kind"),
            _ => PolicyDecision::Allow,
        };
        let approval = match &action.kind {
            ActionKind::ExecuteProcess
                if action.parameters.process_class
                    == Some(lokai_domain::execution::ProcessClass::BuildVerification) =>
            {
                ApprovalRequirement::None
            }
            ActionKind::ExecuteShell => ApprovalRequirement::Interactive,
            ActionKind::StartInternalService => ApprovalRequirement::None,
            _ => ApprovalRequirement::None, // Fallback, could be fine-tuned
        };
        ActionPolicyOutcome { decision, approval }
    }
}

impl PolicyEvaluator for PolicyEngine {
    fn evaluate(&self, action: &ProposedAction) -> ActionPolicyOutcome {
        self.evaluate_action(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_domain::{
        ids::{AgentId, SessionId},
        ActionId, DisclosureTier,
    };

    fn ctx(class: DataClass) -> PolicyContext {
        PolicyContext {
            mode: PolicyMode::EstateStub,
            session_data_class: class,
        }
    }

    fn worker_job(class: DataClass) -> FabricJobDraft {
        FabricJobDraft {
            data_class: class,
            disclosure_tier: DisclosureTier::MetadataOnly,
            destination: Destination::EstateWorker {
                id: "worker_a".into(),
            },
            is_circle_job: false,
        }
    }

    #[test]
    fn secret_never_remote() {
        let engine = PolicyEngine::default();
        assert!(!engine
            .check_remote_inference(&ctx(DataClass::Secret), &worker_job(DataClass::Secret))
            .allowed());
    }

    #[test]
    fn session_secret_denies_looser_job_class() {
        let engine = PolicyEngine::default();
        let job = worker_job(DataClass::RepositorySource);
        assert!(!engine
            .check_remote_inference(&ctx(DataClass::Secret), &job)
            .allowed());
    }

    #[test]
    fn estate_stub_allows_repository_source_to_worker() {
        let engine = PolicyEngine::default();
        assert!(engine
            .check_remote_inference(
                &ctx(DataClass::RepositorySource),
                &worker_job(DataClass::RepositorySource)
            )
            .allowed());
    }

    #[test]
    fn estate_stub_denies_circle_peer() {
        let engine = PolicyEngine::default();
        let job = FabricJobDraft {
            data_class: DataClass::SensitiveSource,
            disclosure_tier: DisclosureTier::Summary,
            destination: Destination::CirclePeer {
                circle_id: "c".into(),
                peer_id: "p".into(),
            },
            is_circle_job: false,
        };
        assert!(!engine
            .check_remote_inference(&ctx(DataClass::SensitiveSource), &job)
            .allowed());
    }

    #[test]
    fn full_mode_allows_circle_peer_for_sensitive_source() {
        let engine = PolicyEngine::new(PolicyMode::Full);
        let ctx = PolicyContext {
            mode: PolicyMode::Full,
            session_data_class: DataClass::SensitiveSource,
        };
        let job = FabricJobDraft {
            data_class: DataClass::SensitiveSource,
            disclosure_tier: DisclosureTier::Summary,
            destination: Destination::CirclePeer {
                circle_id: "c".into(),
                peer_id: "p".into(),
            },
            is_circle_job: false,
        };
        assert!(engine.check_remote_inference(&ctx, &job).allowed());
    }

    #[test]
    fn circle_job_allowed_on_full_mode() {
        let engine = PolicyEngine::new(PolicyMode::Full);
        let ctx = PolicyContext {
            mode: PolicyMode::Full,
            session_data_class: DataClass::SensitiveSource,
        };
        let job = FabricJobDraft {
            data_class: DataClass::SensitiveSource,
            disclosure_tier: DisclosureTier::Summary,
            destination: Destination::EstateWorker { id: "w".into() },
            is_circle_job: true,
        };
        assert!(engine.check_remote_inference(&ctx, &job).allowed());
    }

    #[test]
    fn circle_job_denied_on_full_without_disclosure() {
        let engine = PolicyEngine::new(PolicyMode::Full);
        let ctx = PolicyContext {
            mode: PolicyMode::Full,
            session_data_class: DataClass::SensitiveSource,
        };
        let job = FabricJobDraft {
            data_class: DataClass::SensitiveSource,
            disclosure_tier: DisclosureTier::MetadataOnly,
            destination: Destination::EstateWorker { id: "w".into() },
            is_circle_job: true,
        };
        assert!(!engine.check_remote_inference(&ctx, &job).allowed());
    }

    #[test]
    fn default_engine_allows_verify_and_mutations() {
        let engine = PolicyEngine::default();
        assert!(engine.verify_allowed());
        assert!(engine.mutations_allowed());
    }

    #[test]
    fn evaluate_action_denies_verify_when_disabled() {
        let engine = PolicyEngine::default();
        engine.set_verify_allowed(false);
        let action = ProposedAction {
            action_id: ActionId::new("a1"),
            session_id: lokai_domain::ids::SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(lokai_domain::ids::AgentId::new("root")),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            kind: ActionKind::ExecuteProcess,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: "test".into(),
                executable_identity: Some("cargo".into()),
                resolved_path: None,
                arguments: vec!["test".into()],
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: None,
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: Some(lokai_domain::execution::ProcessClass::BuildVerification),
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: None,
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        assert!(!engine.evaluate_action(&action).decision.allowed());
    }

    #[test]
    fn evaluate_action_denies_mutations_when_disabled() {
        let engine = PolicyEngine::default();
        engine.set_mutations_allowed(false);
        let action = ProposedAction {
            action_id: ActionId::new("m1"),
            session_id: lokai_domain::ids::SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(lokai_domain::ids::AgentId::new("root")),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            kind: ActionKind::WriteFile,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: "test".into(),
                executable_identity: None,
                resolved_path: Some("a.rs".into()),
                arguments: vec![],
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: None,
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: None,
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: None,
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        assert!(!engine.evaluate_action(&action).decision.allowed());
    }

    #[test]
    fn evaluate_action_blocks_destructive_shell() {
        let engine = PolicyEngine::default();
        let action = ProposedAction {
            action_id: ActionId::new("sh1"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(AgentId::new("root")),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            kind: ActionKind::ExecuteShell,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: String::new(),
                executable_identity: None,
                resolved_path: None,
                arguments: vec![],
                shell_identity: Some("sh".into()),
                shell_mode: Some("one_shot".into()),
                script_bytes: Some(b"rm -rf /".to_vec()),
                working_directory: None,
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: None,
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: Some(serde_json::json!({"command":"rm -rf /"})),
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        assert!(!engine.evaluate_action(&action).decision.allowed());
    }

    #[test]
    fn policy_evaluator_trait_delegates() {
        let engine = PolicyEngine::default();
        engine.set_verify_allowed(false);
        let action = ProposedAction {
            action_id: ActionId::new("a1"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: Some(AgentId::new("root")),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            kind: ActionKind::ExecuteProcess,
            parameters: lokai_domain::execution::CanonicalActionParameters {
                digest: "digest".into(),
                executable_identity: Some("cargo".into()),
                resolved_path: None,
                arguments: vec!["test".into()],
                shell_identity: None,
                shell_mode: None,
                script_bytes: None,
                working_directory: Some("/tmp".into()),
                env_vars: None,
                stdin_source_classification: None,
                filesystem_access_scope: None,
                network_policy: None,
                resource_limits: None,
                process_class: Some(lokai_domain::execution::ProcessClass::BuildVerification),
                sandbox_profile: None,
                expected_output_limits: None,
                schema_version: 1,
                tool_arguments: None,
            },
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        };
        assert!(!PolicyEvaluator::evaluate(&engine, &action)
            .decision
            .allowed());
    }

    #[test]
    fn settings_roundtrip() {
        let engine = PolicyEngine::from_settings(PolicySettings {
            mode: PolicyMode::Full,
            verify_allowed: false,
            mutations_allowed: false,
            placement: ProjectPlacementPolicy::default(),
        });
        let s = engine.settings();
        assert_eq!(s.mode, PolicyMode::Full);
        assert!(!s.verify_allowed);
        assert!(!s.mutations_allowed);
    }
}
