//! Membership/credential layer for host-authorized execution. Content and
//! definition access never substitute for a separate execution grant decision.
use super::*;
use tetonic_domain::{AgentIdentity, AgentJobSpec, ExecutionScope};
use tetonic_run::managed::{AuthorizedExecution, ExecutionAuthority};

struct ScopedAuthority {
    store: SharedStore,
    credential: super::credential_binding::BoundCredential,
    scope: ExecutionScope,
    agent_key: String,
    definition_digest: String,
    grants: Arc<dyn ExecutionAuthority>,
}

#[async_trait]
impl ExecutionAuthority for ScopedAuthority {
    async fn authorize(
        &self,
        scope: &ExecutionScope,
        identity: &AgentIdentity,
        job: &AgentJobSpec,
    ) -> Result<(), ()> {
        if scope != &self.scope
            || identity.id != job.identity_id
            || identity.bound_definition_digest != self.definition_digest
            || job.definition_digest != self.definition_digest
        {
            return Err(());
        }
        self.credential
            .verify(&scope.principal_id)
            .await
            .map_err(|_| ())?;
        let scope = scope.clone();
        let key = self.agent_key.clone();
        let digest = self.definition_digest.clone();
        let expected = identity.clone();
        let allowed = self
            .store
            .read(move |db| -> Result<bool, StoreError> {
                if !db.context_access_in_organization(
                    &scope.principal_id,
                    &scope.information_context_id,
                    &scope.organization_id,
                )? {
                    return Ok(false);
                }
                let Some(registered) = db.get_organization_agent_revision(
                    &scope.principal_id,
                    &scope.organization_id,
                    &key,
                    &digest,
                )?
                else {
                    return Ok(false);
                };
                if registered.identity.identity_id != expected.id.0 {
                    return Ok(false);
                }
                let actual = tetonic_run::get_identity_revision(db, &expected.id, &digest)
                    .map_err(|_| StoreError::ControlAccessDenied)?;
                Ok(actual.as_ref() == Some(&expected))
            })
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        if !allowed {
            return Err(());
        }
        self.grants.authorize(&self.scope, identity, job).await?;
        self.credential
            .verify(&self.scope.principal_id)
            .await
            .map_err(|_| ())
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

impl ContextService {
    /// Trusted host composition only. The mandatory grants adapter must evaluate
    /// execution permission and resource/budget policy, independently of content
    /// membership. No permissive default or employee transport is provided.
    pub async fn bind_execution_authority(
        &self,
        credential: &str,
        organization: String,
        context: String,
        agent_key: String,
        definition_digest: String,
        grants: Arc<dyn ExecutionAuthority>,
    ) -> Result<AuthorizedExecution, ResourceError> {
        let actor = self.verifier.verify(credential).await?;
        let scope = ExecutionScope {
            principal_id: actor.principal_id,
            organization_id: organization,
            information_context_id: context,
        };
        Ok(AuthorizedExecution {
            grant_id: None,
            scope: scope.clone(),
            authority: Arc::new(ScopedAuthority {
                store: self.store.clone(),
                credential: super::credential_binding::BoundCredential::new(
                    self.verifier.clone(),
                    credential,
                ),
                scope,
                agent_key,
                definition_digest,
                grants,
            }),
        })
    }
}
