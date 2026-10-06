//! Work allowances use the existing team authority and memory transaction boundary.
use super::*;
use tetonic_memory::{WorkBudget, WorkBudgetReservation};

impl ResourceService {
    pub async fn authorize_work_budget(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
        request: String,
        tokens: i64,
    ) -> Result<WorkBudget, ResourceError> {
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
                db.authorize_work_budget(&actor.principal_id, &org, &team, &work, &request, tokens)
            })
            .await??)
    }

    pub async fn work_budget(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
    ) -> Result<WorkBudget, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ReadTeam {
                    org_id: org.clone(),
                    team_id: team.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .read(move |db| db.work_budget(&actor.principal_id, &org, &team, &work))
            .await??)
    }

    /// Allocation receipt only. The execution host must bind and enforce a
    /// reservation before dispatch; this API cannot authorize a tool/model call.
    pub async fn reserve_work_budget(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
        request: String,
        tokens: i64,
    ) -> Result<WorkBudgetReservation, ResourceError> {
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
                db.reserve_work_budget(&actor.principal_id, &org, &team, &work, &request, tokens)
            })
            .await??)
    }
}
