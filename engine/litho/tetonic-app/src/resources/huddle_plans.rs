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
    pub(crate) async fn plan_continuation_links(
        &self,
        credential: &str,
        org: String,
        team: String,
        work: String,
    ) -> Result<Vec<tetonic_memory::PlanContinuation>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let rows = self
            .store
            .read(move |db| db.plan_continuation_links(&actor.principal_id, &org, &team, &work))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(rows)
    }

    pub(crate) async fn create_plan_continuation(
        &self,
        credential: &str,
        org: String,
        team: String,
        input: ContinuationDraft,
    ) -> Result<tetonic_memory::PlanContinuation, ResourceError> {
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
                db.create_plan_continuation(tetonic_memory::CreatePlanContinuation {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    receipt: &input.receipt,
                    guide_key: &input.guide_key,
                    brief: &input.brief,
                    content: &input.content,
                })
            })
            .await??)
    }

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
                } => db.save_huddle_plan(tetonic_memory::SaveHuddlePlan {
                    actor: &actor.principal_id,
                    org: &org,
                    team: &team,
                    work: &work,
                    request: &request,
                    expected,
                    brief_revision,
                    generation_id: &generation_id,
                    generation_input: &generation_input,
                    content: content.as_ref(),
                }),
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

pub(crate) struct ContinuationDraft {
    pub receipt: tetonic_memory::PlanContinuation,
    pub guide_key: String,
    pub brief: String,
    pub content: PlanContent,
}
