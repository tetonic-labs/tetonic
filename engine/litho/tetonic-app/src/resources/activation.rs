//! Registered jobs enter the existing manager; no alternate lifecycle or store.
use super::*;
use crate::errors::AppError;
use tetonic_run::{managed::AdmissionContext, StartIdentityJobResult};

/// Employee-supplied selectors and input. Principal, grants, identity contents and
/// invocation are resolved by the host, never accepted from this request.
/// The recovery ID binds the exact granted job; it is not an idempotency token.
pub struct RegisteredAgentJob {
    pub organization_id: String,
    pub information_context_id: String,
    pub agent_key: String,
    pub definition_digest: String,
    pub execution_grant_id: String,
    pub input: String,
    pub recovery_id: String,
}

fn resource_error(error: ResourceError) -> AppError {
    match error {
        ResourceError::Denied => AppError::PolicyDenied("registered job access denied".into()),
        ResourceError::Storage | ResourceError::StorageRequired => {
            AppError::PersistenceFailed("registered job storage unavailable".into())
        }
        _ => AppError::InvalidRequest("invalid registered job".into()),
    }
}

impl crate::services::DefaultRunService {
    /// Trusted application host entry, requiring a Tokio LocalSet. The host owns
    /// the verifier, preparation ceilings, provider and resource-restricted tool
    /// executor. A stored job grant does not itself sandbox tools or reserve spend.
    /// Submission returns the existing managed attempt and completion receiver.
    pub async fn submit_registered_job(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        request: RegisteredAgentJob,
        limits: HarnessPreparationLimits,
        agent: tetonic_core::Agent,
    ) -> Result<
        (
            tetonic_domain::AttemptId,
            tokio::sync::oneshot::Receiver<StartIdentityJobResult>,
        ),
        AppError,
    > {
        let store = self
            .managed()
            .store()
            .cloned()
            .ok_or_else(|| resource_error(ResourceError::StorageRequired))?;
        // Both services are composed from the manager's store, not caller-selected
        // stores that could authorize one organization and execute in another.
        let resources = ResourceService {
            store: store.clone(),
            authority: Arc::new(membership::MembershipAuthority {
                store: store.clone(),
                verifier: verifier.clone(),
            }),
        };
        let contexts = ContextService { store, verifier };
        let prepared = resources
            .prepare_general_revision(
                credential,
                request.organization_id.clone(),
                request.agent_key.clone(),
                request.definition_digest.clone(),
                request.input,
                limits,
            )
            .await
            .map_err(resource_error)?;
        let command = prepared
            .start_command(request.recovery_id)
            .map_err(resource_error)?;
        let policy = prepared.execution_policy().map_err(resource_error)?;
        let authorization = contexts
            .bind_stored_execution_grant(
                credential,
                request.organization_id,
                request.information_context_id,
                request.agent_key,
                request.definition_digest,
                request.execution_grant_id,
            )
            .await
            .map_err(resource_error)?;
        authorization
            .authority
            .authorize(&authorization.scope, &command.identity, &command.job_spec)
            .await
            .map_err(|_| resource_error(ResourceError::Denied))?;
        // Preparation failures must not create a run or report an active attempt.
        policy(
            Some(&command.identity),
            &command.job_spec,
            agent.execution_role(),
            &agent.advertised_tool_names(),
            &command.invocation,
        )
        .map_err(|_| {
            AppError::InvalidRequest("executor does not match registered harness".into())
        })?;
        // Cloning retains the existing registry, supervisor, hooks and storage;
        // the definition validator is pinned only for this admission.
        self.managed()
            .as_ref()
            .clone()
            .with_execution_policy(policy)
            .submit_identity_job_with_context(
                command,
                agent,
                AdmissionContext {
                    authorization: Some(authorization),
                    ..Default::default()
                },
            )
            .await
            .map_err(Into::into)
    }
}

impl crate::Application {
    /// Host composition entry shared by transports. The caller must construct the
    /// agent using the configured inference/effect controls before submitting it.
    pub async fn submit_registered_job(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        request: RegisteredAgentJob,
        limits: HarnessPreparationLimits,
        agent: tetonic_core::Agent,
    ) -> Result<
        (
            tetonic_domain::AttemptId,
            tokio::sync::oneshot::Receiver<StartIdentityJobResult>,
        ),
        AppError,
    > {
        self.run_manager
            .submit_registered_job(credential, verifier, request, limits, agent)
            .await
    }
}
