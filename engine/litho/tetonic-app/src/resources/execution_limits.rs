use super::*;
pub use tetonic_memory::{OrganizationExecutionLimits, TeamExecutionLimits};

impl ResourceService {
    pub async fn get_execution_limits(
        &self,
        credential: &str,
        org: String,
    ) -> Result<OrganizationExecutionLimits, ResourceError> {
        let action = ResourceAction::ReadOrganization {
            org_id: org.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let limits = self
            .store
            .read(move |db| db.get_execution_limits(&actor.principal_id, &org))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(limits)
    }

    pub async fn set_execution_limits(
        &self,
        credential: &str,
        org: String,
        expected_revision: u64,
        max_active_runs: u32,
        max_active_runs_per_principal: u32,
    ) -> Result<OrganizationExecutionLimits, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        self.store
            .write(move |db| {
                db.set_execution_limits(
                    &actor.principal_id,
                    &org,
                    expected_revision,
                    max_active_runs,
                    max_active_runs_per_principal,
                )
            })
            .await?
            .map_err(Into::into)
    }

    pub async fn get_team_execution_limits(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<TeamExecutionLimits, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let limits = self
            .store
            .read(move |db| db.get_team_execution_limits(&actor.principal_id, &org, &team))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(limits)
    }

    pub async fn set_team_execution_limits(
        &self,
        credential: &str,
        org: String,
        team: String,
        expected_revision: u64,
        max_active_runs: u32,
    ) -> Result<TeamExecutionLimits, ResourceError> {
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
        self.store
            .write(move |db| {
                db.set_team_execution_limits(
                    &actor.principal_id,
                    &org,
                    &team,
                    expected_revision,
                    max_active_runs,
                )
            })
            .await?
            .map_err(Into::into)
    }
}
