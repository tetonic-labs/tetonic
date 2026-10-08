use super::*;
impl WorkService {
    pub async fn snapshot(&self) -> Result<LocalWorkspaceSnapshot, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let resources = self.services.local.resources();
        let team = resources
            .get_team(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
            )
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("local team unavailable".into()))?;
        let organization = resources
            .get_organization(
                &self.services.host.credential,
                app_scope.organization().into(),
            )
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("workspace unavailable".into()))?;
        let agent = resources
            .get_agent(
                &self.services.host.credential,
                app_scope.organization().into(),
                AGENT.into(),
            )
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("local agent unavailable".into()))?;
        let work = resources
            .list_team_work_items(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
            )
            .await
            .map_err(resource)?;
        let planning_ids = self.planning_ids().await?;
        let mut tasks = Vec::new();
        let mut planning_tasks = Vec::new();
        for item in work {
            // Planning runs belong in their huddle inspector, not as separate
            // top-level outcomes on the work map.
            if let Some(source) = planning_ids.get(&item.work_id) {
                let mut task = self.project_task(item).await?;
                task.planning_for = Some(source.clone());
                planning_tasks.push(task);
                continue;
            }
            tasks.push(self.project_task(item).await?);
        }
        Ok(LocalWorkspaceSnapshot {
            work_teams: self.work_teams().await?,
            usage: self
                .services
                .local
                .resources()
                .team_work_usage(
                    &self.services.host.credential,
                    app_scope.organization().into(),
                    app_scope.team().into(),
                )
                .await
                .map_err(resource)?,
            budget_setting: self
                .services
                .local
                .resources()
                .team_budget_setting(
                    &self.services.host.credential,
                    app_scope.organization().into(),
                    app_scope.team().into(),
                )
                .await
                .map_err(resource)?,
            budget_max_tokens: self
                .services
                .host
                .settings
                .reported_token_ceiling
                .unwrap_or(4096),
            shaping_agent_key: shaping::GUIDE.into(),
            organization: organization.name,
            team_id: app_scope.team().into(),
            team_name: team.name,
            agent_id: agent.identity.identity_id,
            agent_name: AGENT.into(),
            model: self.services.host.settings.model.clone(),
            input_limit: INPUT_LIMIT,
            agents: self.services.agents().await?,
            tasks,
            planning_tasks,
        })
    }

    pub async fn task(&self, id: &str) -> Result<LocalTask, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(id)?;
        let work = self
            .services
            .local
            .resources()
            .get_team_work_item(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
                id.into(),
            )
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("task unavailable".into()))?;
        self.project_task(work).await
    }

    pub(super) async fn project_task(
        &self,
        work: tetonic_memory::TeamWorkItem,
    ) -> Result<LocalTask, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let (request_id, parent_id) = conversations::split_parent(&work.request_id);
        let plan = self.plan_link(&work.work_id).await?;
        let key = plan
            .as_ref()
            .map(|p| p.agent_key.as_str())
            .unwrap_or_else(|| request_id.split_once('@').map_or(AGENT, |(_, key)| key));
        let stored = if let Some(link) = &plan {
            self.services
                .local
                .resources()
                .get_agent_revision(
                    &self.services.host.credential,
                    app_scope.organization().into(),
                    key.into(),
                    link.definition_digest.clone(),
                )
                .await
                .map_err(resource)?
                .ok_or(AppError::InferenceUnavailable)?
        } else {
            self.services.registered_agent(key).await?
        };
        let agent = self.services.agent_profile(key.into(), &stored)?;
        let context = plan
            .as_ref()
            .map(|p| p.information_context_id.clone())
            .unwrap_or_else(|| self.services.scope.context().to_owned());
        let bound_attempt = work.attempt_id.clone();
        let work_team = self
            .work_team(
                plan.as_ref()
                    .map_or(work.work_id.as_str(), |p| p.source_work_id.as_str()),
            )
            .await?;
        let mut task = LocalTask {
            work_team,
            human_questions: vec![],
            plan,
            planning_for: None,
            purpose: work.purpose,
            parent_id: parent_id.map(str::to_owned),
            error: None,
            id: work.work_id,
            input: work.input.unwrap_or(work.title),
            agent_key: agent.key,
            agent_name: agent.name,
            state: "not_started".into(),
            run_id: work.run_id,
            sequence: 0,
            messages: vec![],
        };
        let Some(run) = task.run_id.clone() else {
            return Ok(task);
        };
        let contexts = self.services.local.contexts();
        let snapshot = contexts
            .inspect_run(
                &self.services.host.credential,
                app_scope.organization().into(),
                context.clone(),
                run,
            )
            .await
            .map_err(resource)?;
        task.sequence = snapshot.sequence;
        // A child can finish while its parent is still active. Never substitute a
        // sibling's status, transcript or artifact for this work item's attempt.
        let attempt = bound_attempt.as_ref().and_then(|id| {
            snapshot
                .attempts
                .get(&tetonic_domain::AttemptId::new(id.clone()))
        });
        let bound_task = attempt.and_then(|a| snapshot.tasks.get(&a.task_id));
        task.state = match (&snapshot.state, bound_task.map(|t| &t.state)) {
            (_, Some(TaskState::Succeeded)) => "completed",
            (_, Some(TaskState::Failed | TaskState::Skipped)) => "failed",
            (_, Some(TaskState::Canceled)) => "canceled",
            (RunState::Canceled, _) => "canceled",
            (RunState::Canceling, _) => "canceling",
            (RunState::Failed, _) => "failed",
            (RunState::RecoveryRequired, _) => "recovery_required",
            (_, Some(TaskState::Running)) => "running",
            (_, Some(TaskState::Parked)) => "recovery_required",
            _ => "starting",
        }
        .into();
        if task.plan.is_some() {
            // Older finite-plan records used the generic retry policy but had
            // no retry owner. Show the terminal bound attempt, not false Starting.
            if task.state != "completed" {
                task.state = match attempt.map(|a| &a.state) {
                    Some(
                        tetonic_domain::AttemptState::TimedOut
                        | tetonic_domain::AttemptState::Failed,
                    ) => "failed".into(),
                    Some(tetonic_domain::AttemptState::Canceled) => "canceled".into(),
                    Some(
                        tetonic_domain::AttemptState::LeaseExpired
                        | tetonic_domain::AttemptState::Superseded,
                    ) => "recovery_required".into(),
                    _ => task.state,
                };
            }
        }
        let work_id = task.id.clone();
        let (questions, waiting) = self
            .services
            .local
            .store()
            .read(move |db| {
                let questions = db.work_human_questions(
                    app_scope.principal(),
                    app_scope.organization(),
                    app_scope.team(),
                    &work_id,
                )?;
                let now = chrono::Utc::now().timestamp() as u64;
                let waiting = questions.iter().any(|q| {
                    q.answer.is_none()
                        && db
                            .pending_work_human_question(
                                app_scope.principal(),
                                app_scope.organization(),
                                app_scope.team(),
                                &work_id,
                                &q.id,
                                now,
                            )
                            .is_ok()
                });
                Ok::<_, tetonic_memory::StoreError>((questions, waiting))
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|e| resource(e.into()))?;
        task.human_questions = questions;
        if waiting && matches!(task.state.as_str(), "running" | "recovery_required") {
            task.state = "waiting_human".into();
        }
        if task.state == "failed" {
            task.error = Some(
                task_failure_message(attempt.and_then(|a| a.failure_reason.as_deref())).into(),
            );
        }
        if let Some(history) = bound_task
            .and_then(|t| {
                t.binding
                    .activation
                    .as_ref()
                    .or_else(|| t.binding.delegation.as_ref().map(|d| &d.activation))
            })
            .map(|a| a.audit_session_id.clone())
        {
            let history = contexts
                .transcript(
                    &self.services.host.credential,
                    context.clone(),
                    history,
                    100,
                )
                .await
                .map_err(resource)?;
            // Delegation work stores its title; the scoped audit contains the
            // exact admitted input, including dependency contributions.
            if task.plan.is_some() {
                if let Some((_, _, input)) = history.iter().find(|(_, role, _)| role == "user") {
                    task.input = input.clone();
                }
            }
            task.messages = history
                .into_iter()
                .filter(|(_, role, content)| {
                    (role == "assistant" || role == "tool") && !content.trim().is_empty()
                })
                .map(|(id, role, content)| LocalMessage { id, role, content })
                .collect();
        }
        // `finish` may contain the entire answer in its arguments, with no assistant
        // text in the transcript. The accepted artifact is the durable outcome source.
        if task.state == "completed" {
            if let Some(output) = bound_task.and_then(|t| t.accepted_artifact.as_ref()) {
                let artifacts = contexts
                    .bind_artifacts(
                        &self.services.host.credential,
                        context.clone(),
                        self.services.host.app.host.runtime.artifact_store().clone(),
                    )
                    .await
                    .map_err(resource)?;
                let id = tetonic_domain::ArtifactId::new(output.artifact_id.clone());
                if let Ok(mut reader) = artifacts.open(&id).await {
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 4096];
                    let mut read_ok = true;
                    loop {
                        match reader.read_chunk(&mut buffer).await {
                            Ok(0) => break,
                            Ok(read) => {
                                if bytes.len() + read > 65_536 {
                                    read_ok = false;
                                    break;
                                }
                                bytes.extend_from_slice(&buffer[..read]);
                            }
                            Err(_) => {
                                read_ok = false;
                                break;
                            }
                        }
                    }
                    use sha2::Digest;
                    if read_ok
                        && format!("sha256:{:x}", sha2::Sha256::digest(&bytes)) == output.digest
                    {
                        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                            if value["v"] == 1 && value["outcome"] == "completed" {
                                if let Some(answer) =
                                    value["message"].as_str().filter(|s| !s.trim().is_empty())
                                {
                                    let has_assistant = task.messages.iter().any(|message| {
                                        message.role == "assistant" && message.content == answer
                                    });
                                    if !has_assistant {
                                        task.messages.push(LocalMessage {
                                            id: -1,
                                            role: "assistant".into(),
                                            content: answer.into(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(task)
    }
}

// Do not expose provider response bodies, tool output or internal store errors.
pub(super) fn task_failure_message(reason: Option<&str>) -> &'static str {
    let reason = reason.unwrap_or_default();
    if reason.contains("token allowance") || reason.contains("complete token usage") {
        return "Work stopped at its token allowance, or usage could not be confirmed. Open Usage to review the recorded amount and any held allowance.";
    }
    if reason.contains("requested model residency unavailable") {
        "The local inference server could not confirm the requested model was loaded. Completed contributions are saved. Check the model server before starting more work."
    } else if reason.contains("hosted HTTP status 401") || reason.contains("hosted HTTP status 403")
    {
        "The provider rejected access. Check the saved API key and this model's permissions."
    } else if reason.contains("hosted HTTP status 429") {
        "The provider's rate or usage limit was reached. Check your account and try again later."
    } else if reason.contains("hosted HTTP status 400") || reason.contains("hosted HTTP status 404")
    {
        "The provider could not accept this model request. Check the model ID and tool-calling support."
    } else if reason == "execution deadline exceeded" {
        "The run reached its time limit. Narrow the request or review the agent's available run limits before trying again."
    } else if reason.contains("secret") || reason.contains("disclosure policy") {
        "The engine blocked this prompt's disclosure. Remove sensitive credentials before trying again."
    } else if reason.contains("credential unavailable") {
        "The provider key is unavailable. Save it again in agent setup."
    } else {
        "The run could not finish. Check the model connection and the agent's run limits before trying again."
    }
}
