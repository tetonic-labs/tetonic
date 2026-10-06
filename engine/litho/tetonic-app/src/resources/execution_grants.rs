//! Stored job permission composition; grants do not reserve budgets or sandbox tools.
use super::*;
use tetonic_domain::{AgentIdentity, AgentJobSpec, ExecutionScope};
use tetonic_run::managed::{AuthorizedExecution, DelegationParent, ExecutionAuthority};

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
    parent: Option<DelegationParent>,
}
#[async_trait]
impl ExecutionAuthority for StoredGrant {
    async fn authorize(
        &self,
        scope: &ExecutionScope,
        _: &AgentIdentity,
        job: &AgentJobSpec,
    ) -> Result<(), ()> {
        let original_scope = scope;
        let (id, scope, job, at) = (
            self.id.clone(),
            scope.clone(),
            job.clone(),
            now().map_err(|_| ())?,
        );
        let parent = self.parent.as_ref().map(|parent| {
            let binding = parent.binding();
            (binding.run_id.0.clone(), binding.attempt_id.0.clone())
        });
        let allowed = self
            .store
            .read(move |db| match parent {
                Some((run, attempt)) => {
                    db.delegated_execution_grant_allows(&id, &scope, &job, &run, &attempt, at)
                }
                None => db.execution_grant_allows(&id, &scope, &job, at),
            })
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        if allowed {
            if let Some(parent) = &self.parent {
                parent.authorize_child_scope(original_scope).await?;
            }
            Ok(())
        } else {
            Err(())
        }
    }

    async fn revoked_during_execution(
        &self,
        scope: &ExecutionScope,
        identity: &AgentIdentity,
        job: &AgentJobSpec,
    ) -> bool {
        self.authorize(scope, identity, job).await.is_err()
    }
}
impl ResourceService {
    /// Explicit delegation permission for an already allocated child. This does
    /// not reserve a second allowance or launch an independent registered job.
    pub async fn derive_execution_grant(
        &self,
        credential: &str,
        org: String,
        team: String,
        request: tetonic_memory::DelegatedGrantRequest,
    ) -> Result<tetonic_memory::DelegatedExecutionGrant, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageTeam {
                    org_id: org.clone(),
                    team_id: team.clone(),
                },
            )
            .await?;
        let at = now()?;
        Ok(self
            .store
            .write(move |db| {
                db.derive_execution_grant(&actor.principal_id, &org, &team, &request, at)
            })
            .await??)
    }

    pub async fn get_execution_grant(
        &self,
        credential: &str,
        org: String,
        id: String,
    ) -> Result<Option<tetonic_memory::ExecutionGrant>, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .read(move |db| db.get_execution_grant(&actor.principal_id, &org, &id))
            .await??)
    }

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
        let mut authorization = self
            .bind_execution_authority(
                credential,
                org,
                context,
                agent_key,
                definition_digest,
                Arc::new(StoredGrant {
                    store: self.store.clone(),
                    id: grant_id.clone(),
                    parent: None,
                }),
            )
            .await?;
        authorization.grant_id = Some(grant_id);
        Ok(authorization)
    }

    /// Live parent authority comes from the managed host. Permission is rechecked at
    /// admission, execution gates and revocation polling through the same trait.
    /// Admission also verifies the stored child allocation and delivery identity.
    pub async fn bind_delegated_execution_grant(
        &self,
        credential: &str,
        parent: DelegationParent,
        command: crate::resources::BindDelegatedExecutionGrant,
    ) -> Result<AuthorizedExecution, ResourceError> {
        let crate::resources::BindDelegatedExecutionGrant {
            org,
            context,
            agent_key,
            definition_digest,
            grant_id,
        } = command;
        let mut authorization = self
            .bind_execution_authority(
                credential,
                org,
                context,
                agent_key,
                definition_digest,
                Arc::new(StoredGrant {
                    store: self.store.clone(),
                    id: grant_id.clone(),
                    parent: Some(parent),
                }),
            )
            .await?;
        authorization.grant_id = Some(grant_id);
        Ok(authorization)
    }
}

#[cfg(test)]
#[path = "delegated_grants_tests.rs"]
mod tests;
