//! Managed run service: identity jobs, inspection, replay and cancellation.

use crate::{commands::*, errors::AppError, events::*};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tetonic_domain::{
    AgentIdentity, AgentJobSpec, AttemptId, CandidateOutcome, ReplayGap, RunEventEnvelope, RunId,
    RunSnapshot, SessionId, TaskId,
};
use tetonic_memory::RecoverMutex;
use tetonic_run::{job_input_digest, RunSupervisor};
use tokio::sync::oneshot;

mod dispatch;
mod identity_job;

pub use tetonic_run::{ExecutionPolicy, FinalizationEffectDriver, FinalizationPolicy};

pub type IdentitySupplier = Arc<dyn Fn(&str) -> (AgentIdentity, AgentJobSpec) + Send + Sync>;

#[async_trait::async_trait]
pub trait RunService: Send + Sync {
    fn execution_service(&self) -> Arc<dyn RunService>;
    async fn execute_attempt(
        &self,
        attempt_id: AttemptId,
        agent: &mut tetonic_core::Agent,
        conversation: &mut tetonic_core::Conversation,
        invocation: tetonic_domain::AgentInvocation,
        on_step: &mut (dyn FnMut(tetonic_core::Step) + Send),
    ) -> CandidateOutcome;
    fn event_sink(&self) -> Arc<dyn ApplicationEventSink>;
    async fn create_run(&self, cmd: CreateRunCommand) -> Result<RunId, AppError>;
    async fn inspect_run(&self, cmd: InspectRunCommand) -> Result<RunSnapshot, AppError>;
    async fn resume_events(
        &self,
        cmd: ResumeRunEventsCommand,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, AppError>;
    async fn cancel_run(&self, cmd: CancelByRunCommand) -> Result<(), AppError>;
    fn run_id_for_attempt(&self, attempt_id: &AttemptId) -> Option<RunId>;
    fn task_id_for_attempt(&self, _attempt_id: &AttemptId) -> Option<TaskId> {
        None
    }
    fn active_job_spec(&self, _attempt_id: &AttemptId) -> Option<AgentJobSpec> {
        None
    }
    fn active_input_digest(&self, _attempt_id: &AttemptId) -> Option<String> {
        None
    }
    async fn start_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: &mut tetonic_core::Agent,
    ) -> Result<StartIdentityJobResult, AppError>;
    fn arm_attempt_join(
        &self,
        _attempt_id: &AttemptId,
    ) -> oneshot::Receiver<StartIdentityJobResult> {
        let (_tx, rx) = oneshot::channel();
        rx
    }
    async fn submit_identity_job(
        &self,
        _cmd: StartIdentityJobCommand,
        _agent: tetonic_core::Agent,
    ) -> Result<(AttemptId, oneshot::Receiver<StartIdentityJobResult>), AppError> {
        Err(AppError::InvalidRequest(
            "submit_identity_job not implemented".into(),
        ))
    }
    fn is_canceled_attempt(&self, _attempt_id: &AttemptId) -> bool {
        false
    }
    fn attempt_must_not_infer(&self, attempt_id: &AttemptId) -> bool {
        self.is_canceled_attempt(attempt_id)
    }
    async fn attempt_authority_revoked(&self, _attempt_id: &AttemptId) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct DefaultRunService {
    events: Arc<dyn ApplicationEventSink>,
    artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    identity_supplier: IdentitySupplier,
    pub(crate) managed: Arc<tetonic_run::ManagedRunService>,
    approvals: Arc<Mutex<Option<Arc<dyn crate::approval::ApprovalService>>>>,
    runtime: Arc<Mutex<Option<Arc<tetonic_runtime::EngineRuntime>>>>,
}

impl DefaultRunService {
    pub fn new(
        store: Option<tetonic_memory::SharedStore>,
        policy: Arc<tetonic_policy::PolicyEngine>,
        events: Arc<dyn ApplicationEventSink>,
        supervisor: Arc<dyn RunSupervisor>,
        artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    ) -> Self {
        let identity_supplier: IdentitySupplier = Arc::new(|job_input: &str| {
            let (def, rec) = ("sha256:default".to_string(), "rec_default".to_string());
            let id = AgentIdentity {
                id: tetonic_domain::IdentityId::new("identity_default"),
                owning_application: "tetonic-app".into(),
                bound_definition_digest: def.clone(),
                privilege_class: "default".into(),
                toolset_subscriptions: vec![],
                context_bindings: vec![],
                recovery_id: rec.clone(),
            };
            let spec = AgentJobSpec {
                identity_id: id.id.clone(),
                definition_digest: def,
                input_digest: job_input_digest(job_input),
                capability_bindings: vec![],
                artifact_bindings: vec![],
                recovery_id: rec,
            };
            (id, spec)
        });
        let managed = Arc::new(
            tetonic_run::ManagedRunService::new(
                supervisor.clone(),
                store.clone(),
                artifacts.clone(),
                policy.clone(),
            )
            .with_execution_policy(Arc::new(|_, _, _, _, _| {
                Err("execution policy is not configured".into())
            })),
        );
        let approvals = Arc::new(Mutex::new(None));
        let runtime = Arc::new(Mutex::new(None));
        managed.attach_hooks(Arc::new(super::observer::ProductRunHooks {
            bindings: Mutex::new(HashMap::new()),
            scanner: tetonic_secrets::ScannerEngine::default_engine(),
            events: events.clone(),
            approvals: approvals.clone(),
            runtime: runtime.clone(),
        }));
        Self {
            managed,
            events,
            artifacts,
            identity_supplier,
            approvals,
            runtime,
        }
    }

    pub fn managed(&self) -> &Arc<tetonic_run::ManagedRunService> {
        &self.managed
    }

    pub fn with_execution_policy(mut self, policy: ExecutionPolicy) -> Self {
        let m = (*self.managed)
            .clone()
            .with_execution_policy(policy.clone());
        self.managed = Arc::new(m);
        self
    }

    pub fn attach_approvals(&self, approvals: Arc<dyn crate::approval::ApprovalService>) {
        *self.approvals.lock_recover() = Some(approvals);
    }

    pub fn attach_runtime(&self, runtime: Arc<tetonic_runtime::EngineRuntime>) {
        *self.runtime.lock_recover() = Some(runtime);
    }

    pub async fn cancel_run(&self, cmd: CancelByRunCommand) -> Result<(), AppError> {
        let run_id = RunId::new(cmd.run_id);
        self.managed.cancel_run(&run_id).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl RunService for DefaultRunService {
    fn execution_service(&self) -> Arc<dyn RunService> {
        Arc::new(self.clone())
    }

    async fn execute_attempt(
        &self,
        attempt_id: AttemptId,
        agent: &mut tetonic_core::Agent,
        conversation: &mut tetonic_core::Conversation,
        invocation: tetonic_domain::AgentInvocation,
        on_step: &mut (dyn FnMut(tetonic_core::Step) + Send),
    ) -> CandidateOutcome {
        self.execute_bound_attempt(attempt_id, agent, conversation, invocation, on_step)
            .await
    }

    fn event_sink(&self) -> Arc<dyn ApplicationEventSink> {
        self.events.clone()
    }

    async fn create_run(&self, cmd: CreateRunCommand) -> Result<RunId, AppError> {
        // Delegates RunCommand::CreateRun to supervisor via managed
        let session_id = cmd.session_id.filter(|s| !s.is_empty()).map(SessionId::new);
        let task_id = cmd.root_task_id.map(TaskId::new);
        let (identity, spec) = match (cmd.identity, cmd.job_spec) {
            (Some(id), Some(spec)) => (Some(id), Some(spec)),
            (Some(id), None) => {
                let spec = AgentJobSpec {
                    identity_id: id.id.clone(),
                    definition_digest: id.bound_definition_digest.clone(),
                    input_digest: job_input_digest(""),
                    capability_bindings: Vec::new(),
                    artifact_bindings: Vec::new(),
                    recovery_id: id.recovery_id.clone(),
                };
                (Some(id), Some(spec))
            }
            (None, Some(spec)) => (None, Some(spec)),
            (None, None) => {
                let (id, spec) = (self.identity_supplier)("");
                (Some(id), Some(spec))
            }
        };
        self.managed
            .create_run(session_id, task_id, identity, spec)
            .await
            .map_err(Into::into)
    }

    async fn inspect_run(&self, cmd: InspectRunCommand) -> Result<RunSnapshot, AppError> {
        self.managed
            .inspect_run(&RunId::new(cmd.run_id))
            .await
            .map_err(Into::into)
    }

    async fn resume_events(
        &self,
        cmd: ResumeRunEventsCommand,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, AppError> {
        let limit = cmd.limit.unwrap_or(256).min(1024) as usize;
        self.managed
            .resume_events(&RunId::new(cmd.run_id), cmd.after_sequence, limit)
            .await
            .map_err(Into::into)
    }

    async fn cancel_run(&self, cmd: CancelByRunCommand) -> Result<(), AppError> {
        self.cancel_run(cmd).await
    }

    fn run_id_for_attempt(&self, id: &AttemptId) -> Option<RunId> {
        self.managed.binding(id).map(|a| a.run_id)
    }
    fn task_id_for_attempt(&self, id: &AttemptId) -> Option<TaskId> {
        self.managed.binding(id).map(|a| a.task_id)
    }
    fn active_job_spec(&self, id: &AttemptId) -> Option<AgentJobSpec> {
        self.managed.binding(id).map(|a| a.job_spec)
    }
    fn active_input_digest(&self, id: &AttemptId) -> Option<String> {
        self.managed.binding(id).map(|a| a.job_spec.input_digest)
    }

    async fn start_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: &mut tetonic_core::Agent,
    ) -> Result<StartIdentityJobResult, AppError> {
        DefaultRunService::start_identity_job(self, cmd, agent).await
    }

    fn arm_attempt_join(&self, id: &AttemptId) -> oneshot::Receiver<StartIdentityJobResult> {
        DefaultRunService::arm_attempt_join(self, id)
    }

    async fn submit_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: tetonic_core::Agent,
    ) -> Result<(AttemptId, oneshot::Receiver<StartIdentityJobResult>), AppError> {
        DefaultRunService::submit_identity_job(self, cmd, agent).await
    }

    fn is_canceled_attempt(&self, id: &AttemptId) -> bool {
        self.managed.is_canceled(id)
    }

    fn attempt_must_not_infer(&self, id: &AttemptId) -> bool {
        self.managed.attempt_must_not_infer(id)
    }

    async fn attempt_authority_revoked(&self, id: &AttemptId) -> bool {
        self.managed.attempt_authority_revoked(id).await
    }
}

#[cfg(test)]
#[path = "regression_tests.rs"]
mod execution_regression_tests;
