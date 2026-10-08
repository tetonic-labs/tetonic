use super::*;
use tetonic_memory::{SaveWorkTeam, WorkTeam};

impl ResourceService {
    pub async fn work_teams(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<Vec<WorkTeam>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let rows = self
            .store
            .read(move |db| db.work_teams(&actor.principal_id, &org, &team))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(rows)
    }

    pub async fn save_work_team(
        &self,
        credential: &str,
        org: String,
        team: String,
        request: SaveWorkTeam,
    ) -> Result<WorkTeam, ResourceError> {
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
        Ok(self
            .store
            .write(move |db| db.save_work_team(&actor.principal_id, &org, &team, &request))
            .await??)
    }

    pub async fn work_team_for_work(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
    ) -> Result<Option<WorkTeam>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let row = self
            .store
            .read(move |db| db.work_team_for_work(&actor.principal_id, &org, &team, &work))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(row)
    }
}
