//! Meter the existing brokered provider, retaining managed request correlation.
use super::*;
use tetonic_inference::{ChatRequest, ChatResponse, InferenceError, InferenceProvider, TokenSink};

pub(super) struct WorkUsageProvider {
    pub inner: Arc<dyn InferenceProvider>,
    pub store: SharedStore,
    pub actor: String,
    pub org: String,
    pub team: String,
    pub work: String,
    pub own_limit: Option<u64>,
}
fn unavailable() -> InferenceError {
    InferenceError::Provider(
        "Work usage could not be recorded. No further model request was authorized.".into(),
    )
}
#[async_trait]
impl InferenceProvider for WorkUsageProvider {
    async fn fabric_snapshot(&self) -> tetonic_inference::FabricSnapshot {
        self.inner.fabric_snapshot().await
    }

    async fn prewarm(&self, model: &str, keep_alive: Option<&str>) -> Result<(), InferenceError> {
        self.inner.prewarm(model, keep_alive).await
    }

    async fn chat(
        &self,
        mut req: ChatRequest,
        sink: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let meta = req.fabric.as_ref().ok_or_else(unavailable)?;
        let (run, task, attempt) = (
            meta.run_id.clone().ok_or_else(unavailable)?,
            meta.task_id.clone().ok_or_else(unavailable)?,
            meta.attempt_id.clone().ok_or_else(unavailable)?,
        );
        let (actor, org, team, work) = (
            self.actor.clone(),
            self.org.clone(),
            self.team.clone(),
            self.work.clone(),
        );
        let call = uuid::Uuid::new_v4().to_string();
        let key = call.clone();
        let model = req.model.clone();
        let own_limit = self.own_limit.map(|v| i64::try_from(v).unwrap_or(i64::MAX));
        let remaining=self.store.write(move |db| db.begin_work_inference_with_limit(&actor,&org,&team,&work,&run,&task,&attempt,&key,&model,chrono::Utc::now().timestamp().max(0) as u64,own_limit)).await
            .map_err(|_|unavailable())?.map_err(|error| match error {
                StoreError::InvalidControlResource(_) => InferenceError::Provider("Work token allowance reached, or earlier usage is unconfirmed. Review Usage before starting more work.".into()),
                _=>unavailable(),
            })?;
        if let Some(remaining) = remaining {
            let cap = u32::try_from(remaining).unwrap_or(u32::MAX);
            req.max_tokens = Some(req.max_tokens.map_or(cap, |v| v.min(cap)));
        }
        // Dropping this future on cancellation leaves a durable pending call.
        // The reader identifies it as unconfirmed after the owner stops/expires.
        let response = self.inner.chat(req, sink).await;
        let (input, output) = response
            .as_ref()
            .map(|r| {
                (
                    r.usage.prompt_tokens.and_then(|v| i64::try_from(v).ok()),
                    r.usage.eval_tokens.and_then(|v| i64::try_from(v).ok()),
                )
            })
            .unwrap_or((None, None));
        self.store
            .write(move |db| db.finish_work_inference(&call, input, output))
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
        if let Some(limit) = remaining {
            if let Ok(_) = &response {
                match input.zip(output).and_then(|(i,o)|i.checked_add(o)) {
                    None=>return Err(InferenceError::Provider("The model did not report complete token usage. Work stopped; its remaining allowance is held for review.".into())),
                    Some(used) if used>limit=>return Err(InferenceError::Provider("The current model request exceeded the work token allowance. Further work stopped; review Usage.".into())),
                    _=>{},
                }
            }
        }
        response
    }
}

impl ResourceService {
    pub async fn team_work_usage(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<Vec<tetonic_memory::WorkUsage>, ResourceError> {
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
            .read(move |db| db.team_work_usage(&actor.principal_id, &org, &team))
            .await??)
    }
    pub async fn team_budget_setting(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<tetonic_memory::TeamBudgetSetting, ResourceError> {
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
            .read(move |db| db.team_budget_setting(&actor.principal_id, &org, &team))
            .await??)
    }
    pub async fn set_team_budget_setting(
        &self,
        credential: &str,
        org: String,
        team: String,
        request: String,
        expected: i64,
        tokens: Option<i64>,
    ) -> Result<tetonic_memory::TeamBudgetSetting, ResourceError> {
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
                db.set_team_budget_setting(
                    &actor.principal_id,
                    &org,
                    &team,
                    &request,
                    expected,
                    tokens,
                )
            })
            .await??)
    }
}
