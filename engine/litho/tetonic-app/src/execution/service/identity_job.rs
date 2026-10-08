//! Sessionless identity+job start, attempt execution and artifact provenance.

use super::DefaultRunService;
use crate::commands::{StartIdentityJobCommand, StartIdentityJobResult};
use crate::errors::AppError;
use tetonic_domain::{AttemptId, CandidateOutcome};

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
