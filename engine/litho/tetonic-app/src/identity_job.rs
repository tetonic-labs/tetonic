//! Sessionless identity+job start and managed completion.

use super::{ActiveTurnRun, DefaultRunService};
use crate::commands::{StartIdentityJobCommand, StartIdentityJobResult};
use crate::errors::AppError;
use tetonic_domain::{AgentIdentity, AgentJobSpec, AttemptId, CandidateOutcome, TaskId};
use tetonic_memory::RecoverMutex;
use tetonic_orchestrator::{ChildAdmit, ChildJob};
pub(super) struct AppChildJob {
    pub(super) session_id: String,
    pub(super) runs: DefaultRunService,
}

#[async_trait::async_trait]
impl ChildJob for AppChildJob {
    async fn admit_child(&self, req: ChildAdmit) -> Result<AttemptId, String> {
        let role = req.role.map(|r| r.0);
        self.runs
            .start_child_attempt(&self.session_id, &req.agent_id, role, Some(req.job_input))
            .await
            .map_err(|e| e.to_string())
    }

    async fn complete_child(
        &self,
        attempt_id: AttemptId,
        outcome: CandidateOutcome,
    ) -> Result<(), String> {
        self.runs
            .complete_child_job(&self.session_id, &attempt_id, &outcome)
            .await
            .map_err(|e| e.to_string())
    }
}

impl DefaultRunService {
    pub async fn artifact_provenance(
        &self,
        artifact_id: &str,
    ) -> Result<tetonic_domain::artifact::ArtifactProvenanceBundle, AppError> {
        let id = tetonic_domain::ids::ArtifactId::new(artifact_id);
        let meta = self.artifacts.metadata(&id).await.map_err(|e| match e {
            tetonic_domain::artifact::ArtifactError::NotFound(aid) => {
                AppError::InvalidRequest(format!("artifact not found: {aid}"))
            }
            other => AppError::InternalViolation(other.to_string()),
        })?;
        Ok(tetonic_domain::artifact::ArtifactProvenanceBundle::from_metadata(&meta))
    }

    pub(super) async fn execute_bound_attempt(
        &self,
        attempt: AttemptId,
        agent: &mut tetonic_core::Agent,
        conversation: &mut tetonic_core::Conversation,
        invocation: tetonic_domain::AgentInvocation,
        on_step: &mut (dyn FnMut(tetonic_core::Step) + Send),
    ) -> CandidateOutcome {
        self.managed
            .execute_attempt(attempt, agent, conversation, invocation, on_step)
            .await
    }
    pub(super) async fn begin_job_run(
        &self,
        session_id: Option<&str>,
        identity: &AgentIdentity,
        spec: AgentJobSpec,
        role: Option<String>,
    ) -> Result<ActiveTurnRun, AppError> {
        let ticket = session_id
            .and_then(|id| self.session_dispatches.lock_recover().get(id).cloned())
            .unwrap_or_else(|| self.managed.reserve_dispatch().id);
        let binding = self
            .managed
            .admit_with_context(
                &ticket,
                tetonic_run::AdmitJob {
                    identity: identity.clone(),
                    job_spec: spec,
                    role,
                    parent_attempt: None,
                },
                tetonic_run::managed::AdmissionContext {
                    deadline: None,
                    authorization: None,
                    speculation: Some(tetonic_domain::SpeculationConfig {
                        allowed: true,
                        max_simultaneous_attempts: 2,
                        require_result_agreement: false,
                    }),
                    session_id: session_id.map(tetonic_domain::SessionId::new),
                    task_id: None,
                },
            )
            .await?;
        Ok(binding)
    }
    pub(super) async fn start_child_attempt(
        &self,
        session_id: &str,
        agent_id: &str,
        role: Option<String>,
        job_input: Option<String>,
    ) -> Result<AttemptId, AppError> {
        let parent = self
            .find_parent_active(session_id)
            .ok_or_else(|| AppError::InvalidRequest("no active turn run".into()))?;
        let mut spec = parent.job_spec.clone();
        spec.input_digest = tetonic_run::job_input_digest(
            job_input
                .as_deref()
                .ok_or_else(|| AppError::InvalidRequest("child job input is required".into()))?,
        );
        spec.recovery_id = format!(
            "{}:{}:{}",
            spec.recovery_id,
            agent_id,
            role.as_deref().unwrap_or("")
        );
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| AppError::InvalidRequest("child identity store unavailable".into()))?;
        let id = spec.identity_id.clone();
        let identity = store
            .read(move |db| tetonic_run::get_identity(db, &id))
            .await
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))?
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))?
            .ok_or_else(|| AppError::InvalidRequest("child identity missing".into()))?;
        if role
            .as_ref()
            .is_some_and(|r| !identity.toolset_subscriptions.contains(r))
        {
            return Err(AppError::InvalidRequest(
                "child role is not in identity toolset subscriptions".into(),
            ));
        }
        let ticket = self.managed.reserve_dispatch();
        let binding = self
            .managed
            .admit_with_context(
                &ticket.id,
                tetonic_run::AdmitJob {
                    identity,
                    job_spec: spec,
                    role,
                    parent_attempt: Some(parent.attempt_id),
                },
                tetonic_run::managed::AdmissionContext {
                    deadline: None,
                    authorization: None,
                    speculation: None,
                    session_id: Some(tetonic_domain::SessionId::new(session_id)),
                    task_id: Some(TaskId::new(format!("task_spawn_{agent_id}"))),
                },
            )
            .await?;
        Ok(binding.attempt_id)
    }
    pub(super) async fn complete_child_job(
        &self,
        session_id: &str,
        attempt: &AttemptId,
        outcome: &CandidateOutcome,
    ) -> Result<(), AppError> {
        self.finalize_attempt(
            attempt,
            session_id,
            outcome,
            matches!(outcome, CandidateOutcome::Canceled { .. }),
            None,
            false,
        )
        .await
        .map(|_| ())
    }
    pub(super) async fn start_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: &mut tetonic_core::Agent,
    ) -> Result<StartIdentityJobResult, AppError> {
        self.managed
            .start_identity_job(cmd, agent)
            .await
            .map_err(Into::into)
    }
    pub(super) async fn submit_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: tetonic_core::Agent,
    ) -> Result<
        (
            AttemptId,
            tokio::sync::oneshot::Receiver<StartIdentityJobResult>,
        ),
        AppError,
    > {
        self.managed
            .submit_identity_job(cmd, agent)
            .await
            .map_err(Into::into)
    }
}
