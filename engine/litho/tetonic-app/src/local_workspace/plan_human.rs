use super::*;
use crate::resources::plan_dispatch::HumanHandoff;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerPlanQuestion {
    pub request_id: String,
    pub question_id: String,
    pub answer: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmendPlanAssignment {
    pub request_id: String,
    pub expected_revision: i64,
    pub assignment_key: String,
    pub instructions: String,
}

impl LocalWorkspace {
    pub(super) fn human_handoff(&self, work: &str) -> HumanHandoff {
        HumanHandoff {
            store: self.keys.store.clone(),
            actor: OWNER.into(),
            org: ORG.into(),
            team: TEAM.into(),
            work: work.into(),
        }
    }
    pub(super) async fn plan_directions(
        &self,
        source: &str,
    ) -> Result<Vec<tetonic_memory::PlanDirection>, AppError> {
        let source = source.to_owned();
        self.keys
            .store
            .read(move |db| db.plan_directions(OWNER, ORG, TEAM, &source))
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|e| resource(e.into()))
    }
    pub async fn answer_plan_question(
        &self,
        work: &str,
        request: AnswerPlanQuestion,
    ) -> Result<tetonic_memory::WorkHumanQuestion, AppError> {
        validate_request_id(work)?;
        validate_request_id(&request.request_id)?;
        let task = self.task(work).await?;
        let question = task
            .human_questions
            .iter()
            .find(|q| q.id == request.question_id)
            .ok_or_else(|| {
                AppError::InvalidRequest("This request for your input is unavailable.".into())
            })?;
        // Receipt reads can succeed after completion. A new answer may only wake
        // a live attempt on this host; a persisted ID cannot revive execution.
        if question.answer.is_none()
            && (task.state != "waiting_human"
                || self
                    .host
                    .app
                    .run_manager
                    .managed()
                    .binding(&tetonic_domain::AttemptId::new(question.attempt_id.clone()))
                    .is_none())
        {
            return Err(AppError::InvalidRequest("This wait ended. Your text has not been sent; inspect the plan before starting more work.".into()));
        }
        let work = work.to_owned();
        self.keys.store.write(move |db| {
            db.answer_work_human(tetonic_memory::AnswerWorkHuman {
                actor: OWNER,
                org: ORG,
                team: TEAM,
                work: &work,
                id: &request.question_id,
                request: &request.request_id,
                answer: &request.answer,
                now: chrono::Utc::now().timestamp() as u64
            })
        }).await.map_err(|_|AppError::InferenceUnavailable)?.map_err(|_|AppError::InvalidRequest("This answer conflicted with a saved response or the wait ended. Reload the plan.".into()))
    }
    pub async fn amend_plan_assignment(
        &self,
        source: &str,
        request: AmendPlanAssignment,
    ) -> Result<tetonic_memory::PlanDirection, AppError> {
        validate_request_id(source)?;
        validate_request_id(&request.request_id)?;
        let _admission = self.admission.lock().await;
        let view = self
            .execution_view(source)
            .await?
            .ok_or_else(|| AppError::InvalidRequest("Start an agreed plan first.".into()))?;
        // Do not re-admit or widen a stopped/restarted coordinator.
        let retry = view
            .directions
            .iter()
            .any(|d| d.request_id == request.request_id);
        if !retry && !matches!(view.state.as_str(), "running" | "waiting_human") {
            return Err(AppError::InvalidRequest(
                "The plan is no longer active. Its saved contributions are retained.".into(),
            ));
        }
        let source = source.to_owned();
        self.keys.store.write(move |db| {
            db.amend_plan_assignment(tetonic_memory::AmendPlanAssignment {
                actor: OWNER,
                org: ORG,
                team: TEAM,
                source: &source,
                expected: request.expected_revision,
                request: &request.request_id,
                key: &request.assignment_key,
                instructions: &request.instructions,
                now: chrono::Utc::now().timestamp() as u64
            })
        }).await.map_err(|_|AppError::InferenceUnavailable)?.map_err(|_|AppError::InvalidRequest("The direction changed or affected work already started. Reload before changing upcoming work.".into()))
    }
}
