use super::*;
use tetonic_memory::{HuddlePlan, PlanContent};

// Host-only commands: not deserializable employee authority or model tool calls.
pub(crate) enum PlanMutation {
    Save {
        request: String,
        expected: i64,
        brief_revision: i64,
        generation_id: String,
        generation_input: String,
        content: Option<PlanContent>,
    },
    Capture {
        revision: i64,
        content: PlanContent,
    },
    Agree {
        revision: i64,
        request: String,
    },
}
impl ResourceService {
    pub async fn huddle_generation_ids(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<Vec<(String, String)>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let result = self
            .store
            .read(move |db| db.huddle_generation_ids(&actor.principal_id, &org, &team))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(result)
    }
    pub async fn huddle_plans(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
    ) -> Result<Vec<HuddlePlan>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let result = self
            .store
            .read(move |db| db.huddle_plans(&actor.principal_id, &org, &team, &work))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(result)
    }
    pub(crate) async fn mutate_huddle_plan(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
        command: PlanMutation,
    ) -> Result<HuddlePlan, ResourceError> {
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
            .write(move |db| match command {
                PlanMutation::Save {
                    request,
                    expected,
                    brief_revision,
                    generation_id,
                    generation_input,
                    content,
                } => db.save_huddle_plan(
                    &actor.principal_id,
                    &org,
                    &team,
                    &work,
                    &request,
                    expected,
                    brief_revision,
                    &generation_id,
                    &generation_input,
                    content.as_ref(),
                ),
                PlanMutation::Capture { revision, content } => db.capture_huddle_plan(
                    &actor.principal_id,
                    &org,
                    &team,
                    &work,
                    revision,
                    &content,
                ),
                PlanMutation::Agree { revision, request } => db.agree_huddle_plan(
                    &actor.principal_id,
                    &org,
                    &team,
                    &work,
                    revision,
                    &request,
                ),
            })
            .await??)
    }
}
