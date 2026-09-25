//! Stored job permission composition; grants do not reserve budgets or sandbox tools.
use super::*;
use tetonic_domain::{AgentIdentity, AgentJobSpec, ExecutionScope};
use tetonic_run::managed::{AuthorizedExecution, ExecutionAuthority};

fn now() -> Result<i64, ResourceError> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ResourceError::Invalid)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| ResourceError::Invalid)
}
struct StoredGrant {
    store: SharedStore,
    id: String,
}
#[async_trait]
impl ExecutionAuthority for StoredGrant {
    async fn authorize(
        &self,
        scope: &ExecutionScope,
        _: &AgentIdentity,
        job: &AgentJobSpec,
    ) -> Result<(), ()> {
        let (id, scope, job, at) = (
            self.id.clone(),
            scope.clone(),
            job.clone(),
            now().map_err(|_| ())?,
        );
        let allowed = self
            .store
            .read(move |db| db.execution_grant_allows(&id, &scope, &job, at))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        if allowed {
            Ok(())
        } else {
            Err(())
        }
    }
}
impl ResourceService {
    pub async fn issue_execution_grant(
        &self,
        credential: &str,
        grant: tetonic_memory::ExecutionGrant,
    ) -> Result<(), ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: grant.scope.organization_id.clone(),
                },
            )
            .await?;
        let at = now()?;
        self.store
            .write(move |db| db.issue_execution_grant(&actor.principal_id, &grant, at))
            .await??;
        Ok(())
    }
    pub async fn revoke_execution_grant(
        &self,
        credential: &str,
        org: String,
        id: String,
    ) -> Result<(), ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        let at = now()?;
        self.store
            .write(move |db| db.revoke_execution_grant(&actor.principal_id, &org, &id, at))
            .await??;
        Ok(())
    }
}
impl ContextService {
    /// A real stored job permission, combined with live credential/context checks.
    /// This is still host composition, not a complete employee activation endpoint.
    pub async fn bind_stored_execution_grant(
        &self,
        credential: &str,
        org: String,
        context: String,
        agent_key: String,
        definition_digest: String,
        grant_id: String,
    ) -> Result<AuthorizedExecution, ResourceError> {
        self.bind_execution_authority(
            credential,
            org,
            context,
            agent_key,
            definition_digest,
            Arc::new(StoredGrant {
                store: self.store.clone(),
                id: grant_id,
            }),
        )
        .await
    }
}
