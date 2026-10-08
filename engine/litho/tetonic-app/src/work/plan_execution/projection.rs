//! Read-only execution receipts, task links and plan outcomes.
use super::*;

impl WorkService {
    pub(crate) async fn execution_receipt(
        &self,
        source: &str,
    ) -> Result<Option<HuddleExecution>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        // Authorize through the same resource door as the plan, including retries.
        self.services
            .local
            .resources()
            .huddle_plans(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
                source.into(),
            )
            .await
            .map_err(resource)?;
        let source = source.to_owned();
        self.services
            .local
            .store()
            .read(move |db| {
                db.huddle_execution(
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

    pub(crate) async fn plan_link(&self, work: &str) -> Result<Option<PlanTaskLink>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let work = work.to_owned();
        let lookup = work.clone();
        let receipt = self
            .services
            .local
            .store()
            .read(move |db| {
                db.huddle_execution_for_work(
                    app_scope.principal(),
                    app_scope.organization(),
                    app_scope.team(),
                    &lookup,
                )
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|e| resource(e.into()))?;
        Ok(receipt.map(|r| {
            let pin = r.assignments.iter().find(|p| p.work_id == work);
            let assignment = pin.and_then(|p| {
                r.content
                    .assignments
                    .iter()
                    .find(|a| a.key == p.assignment_key)
            });
            PlanTaskLink {
                information_context_id: format!("plan-{}", r.root_work_id),
                agent_key: pin.map_or(COORDINATOR.into(), |p| p.agent_key.clone()),
                definition_digest: pin.map_or(r.coordinator_digest.clone(), |p| {
                    p.definition_digest.clone()
                }),
                source_work_id: r.source_work_id,
                root_work_id: r.root_work_id,
                assignment_key: pin.map(|p| p.assignment_key.clone()),
                title: assignment.map_or(r.content.title.clone(), |a| a.title.clone()),
                depends_on: assignment
                    .map(|a| {
                        a.depends_on
                            .iter()
                            .filter_map(|key| {
                                r.assignments
                                    .iter()
                                    .find(|p| &p.assignment_key == key)
                                    .map(|p| p.work_id.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_else(|| r.assignments.iter().map(|p| p.work_id.clone()).collect()),
            }
        }))
    }

    pub(crate) async fn execution_view(
        &self,
        source: &str,
    ) -> Result<Option<PlanExecutionView>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let Some(receipt) = self.execution_receipt(source).await? else {
            return Ok(None);
        };
        let resources = self.services.local.resources();
        let mut assignments = vec![];
        let mut root = None;
        for work in std::iter::once(&receipt.root_work_id)
            .chain(receipt.assignments.iter().map(|p| &p.work_id))
        {
            if let Some(item) = resources
                .get_team_work_item(
                    &self.services.host.credential,
                    app_scope.organization().into(),
                    app_scope.team().into(),
                    work.clone(),
                )
                .await
                .map_err(resource)?
            {
                let task = self.project_task(item).await?;
                if work == &receipt.root_work_id {
                    root = Some(task)
                } else {
                    assignments.push(task)
                }
            }
        }
        let mut state = root
            .as_ref()
            .map_or("recovery_required".into(), |t| t.state.clone());
        let error = receipt
            .start_error
            .clone()
            .or_else(|| root.as_ref().and_then(|t| t.error.clone()));
        if error.is_some()
            && matches!(
                state.as_str(),
                "not_started" | "starting" | "recovery_required"
            )
        {
            state = "failed".into();
        }
        if state == "completed"
            && (assignments.len() != receipt.assignments.len()
                || assignments.iter().any(|t| t.state != "completed"))
        {
            state = "failed".into();
        }
        Ok(Some(PlanExecutionView {
            coordinator: self.pinned_coordinator_model(&receipt).await?,
            directions: self.plan_directions(source).await?,
            receipt,
            state,
            root,
            assignments,
            error,
        }))
    }
}
