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

impl WorkService {
    pub(super) fn human_handoff(&self, work: &str) -> HumanHandoff {
        let app_scope = self.services.scope.clone();
        HumanHandoff {
            store: self.services.local.store().clone(),
            actor: app_scope.principal().into(),
            org: app_scope.organization().into(),
            team: app_scope.team().into(),
            work: work.into(),
            durable_wait_seconds: None,
            prepared_stop_binding: None,
        }
    }
    pub(super) async fn plan_directions(
        &self,
        source: &str,
    ) -> Result<Vec<tetonic_memory::PlanDirection>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let source = source.to_owned();
        self.services
            .local
            .store()
            .read(move |db| {
                db.plan_directions(
                    app_scope.principal(),
                    app_scope.organization(),
                    app_scope.team(),
                    &source,
                )
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|e| resource(e.into()))
    }
    pub async fn answer_plan_question(
        &self,
        work: &str,
        request: AnswerPlanQuestion,
    ) -> Result<tetonic_memory::WorkHumanQuestion, AppError> {
        let app_scope = self.services.authorized_scope().await?;
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
        // Receipt reads can succeed after completion. A new answer requires
        // a live attempt or a verified saved wait. Saving guidance never revives execution.
        if question.answer.is_none()
            && (task.state != "waiting_human"
                || (question.saved_wait.is_none()
                    && self
                        .services
                        .host
                        .app
                        .run_manager
                        .managed()
                        .binding(&tetonic_domain::AttemptId::new(question.attempt_id.clone()))
                        .is_none()))
        {
            return Err(AppError::InvalidRequest("This wait ended. Your text has not been sent; inspect the plan before starting more work.".into()));
        }
        let work = work.to_owned();
        self.services.local.store().write(move |db| {
            db.answer_work_human(tetonic_memory::AnswerWorkHuman {
                actor: app_scope.principal(),
                org: app_scope.organization(),
                team: app_scope.team(),
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
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(source)?;
        validate_request_id(&request.request_id)?;
        let _admission = self.services.admission.lock().await;
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
        self.services.local.store().write(move |db| {
            db.amend_plan_assignment(tetonic_memory::AmendPlanAssignment {
                actor: app_scope.principal(),
                org: app_scope.organization(),
                team: app_scope.team(),
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
