//! Server-bound identifiers for one authenticated workspace. This is not a grant.
use super::*;

#[derive(Clone, Debug)]
pub struct ApplicationScope {
    principal: String,
    organization: String,
    team: String,
    context: String,
}

impl ApplicationScope {
    pub fn principal(&self) -> &str {
        &self.principal
    }
    pub fn organization(&self) -> &str {
        &self.organization
    }
    pub fn team(&self) -> &str {
        &self.team
    }
    pub fn context(&self) -> &str {
        &self.context
    }
}

impl LocalControl {
    /// Bind identifiers after authenticating the caller and checking team access.
    /// Operations must still use their current resource authorization; this value
    /// is deliberately neither deserializable nor a cached permission decision.
    pub async fn application_scope(
        &self,
        credential: &str,
        organization: String,
        team: String,
    ) -> Result<ApplicationScope, ResourceError> {
        let principal = self.credentials().verify(credential).await?;
        self.resources()
            .get_team(credential, organization.clone(), team.clone())
            .await?
            .ok_or(ResourceError::Denied)?;
        let context = self
            .contexts()
            .team_participation_context(credential, organization.clone(), team.clone())
            .await?;
        Ok(ApplicationScope {
            principal: principal.principal_id,
            organization,
            team,
            context,
        })
    }
}

impl ResourceService {
    /// Recheck current membership and credential lifetime, including principal binding.
    pub(crate) async fn validate_application_scope(
        &self,
        credential: &str,
        scope: &ApplicationScope,
    ) -> Result<(), ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ReadTeam {
                    org_id: scope.organization().into(),
                    team_id: scope.team().into(),
                },
            )
            .await?;
        if actor.principal_id != scope.principal() {
            return Err(ResourceError::Denied);
        }
        Ok(())
    }

    pub(crate) async fn work_metadata(
        &self,
        credential: &str,
        scope: ApplicationScope,
        work: String,
    ) -> Result<tetonic_memory::LocalWorkData, ResourceError> {
        self.validate_application_scope(credential, &scope).await?;
        Ok(self
            .store
            .read(move |db| {
                db.work_metadata(scope.principal(), scope.organization(), scope.team(), &work)
            })
            .await??)
    }

    pub(crate) async fn update_work_metadata(
        &self,
        credential: &str,
        scope: ApplicationScope,
        work: String,
        update: tetonic_memory::WorkMetadataPatch,
    ) -> Result<(), ResourceError> {
        self.validate_application_scope(credential, &scope).await?;
        self.store
            .write(move |db| {
                db.update_work_metadata(
                    scope.principal(),
                    scope.organization(),
                    scope.team(),
                    &work,
                    update,
                )
            })
            .await??;
        Ok(())
    }
}
