use super::*;

impl ResourceService {
    pub async fn work_briefs(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
    ) -> Result<Vec<tetonic_memory::WorkBrief>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let rows = self
            .store
            .read(move |db| db.work_briefs(&actor.principal_id, &org, &team, &work))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(rows)
    }

    pub async fn save_work_brief(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
        request: String,
        expected: i64,
        body: String,
    ) -> Result<tetonic_memory::WorkBrief, ResourceError> {
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
            .write(move |db| {
                db.save_work_brief(
                    &actor.principal_id,
                    &org,
                    &team,
                    &work,
                    &request,
                    expected,
                    &body,
                )
            })
            .await??)
    }
}
