use super::*;
impl WorkService {
    pub async fn submit(&self, id: String, input: String) -> Result<LocalTask, AppError> {
        self.submit_for_agent(id, input, AGENT.into()).await
    }

    pub async fn submit_for_agent(
        &self,
        id: String,
        input: String,
        agent_key: String,
    ) -> Result<LocalTask, AppError> {
        self.submit_in_conversation(id, input, agent_key, None)
            .await
    }

    pub async fn submit_in_conversation(
        &self,
        id: String,
        input: String,
        agent_key: String,
        parent_id: Option<String>,
    ) -> Result<LocalTask, AppError> {
        self.submit_with_purpose(id, input, agent_key, parent_id, WorkPurpose::Work)
            .await
    }

    pub async fn submit_with_purpose(
        &self,
        id: String,
        input: String,
        agent_key: String,
        parent_id: Option<String>,
        purpose: WorkPurpose,
    ) -> Result<LocalTask, AppError> {
        self.submit_to_team(id, input, agent_key, parent_id, purpose, None)
            .await
    }

    pub async fn submit_to_team(
        &self,
        id: String,
        input: String,
        agent_key: String,
        parent_id: Option<String>,
        purpose: WorkPurpose,
        work_team: Option<WorkTeamSelection>,
    ) -> Result<LocalTask, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(&id)?;
        if work_team.is_some() && (purpose != WorkPurpose::Explore || agent_key != shaping::GUIDE) {
            return Err(AppError::InvalidRequest(
                "Send team requests through the Guide to shape and review assignments.".into(),
            ));
        }
        if agent_key == plan_execution::COORDINATOR {
            return Err(AppError::InvalidRequest(
                "Start an agreed plan to work with the team coordinator.".into(),
            ));
        }
        let input = input.trim().to_string();
        if input.is_empty() || input.len() > INPUT_LIMIT || input.contains('\0') {
            return Err(AppError::InvalidRequest(format!(
                "Enter a request of 1–{INPUT_LIMIT} UTF-8 bytes."
            )));
        }
        let _admission = self.services.admission.lock().await;
        let resources = self.services.local.resources();
        let registered = self.services.registered_agent(&agent_key).await?;
        let agent = self
            .services
            .agent_profile(agent_key.clone(), &registered)?;
        if purpose == WorkPurpose::Explore
            && (agent_key != shaping::GUIDE || !agent.tools.is_empty())
        {
            return Err(AppError::PolicyDenied(
                "Exploration requires the configured guide with no execution tools.".into(),
            ));
        }
        if let Some(parent) = &parent_id {
            if self.task(parent).await?.purpose != purpose {
                return Err(AppError::InvalidRequest("Keep exploration separate from execution. Start a new assignment when you are ready.".into()));
            }
        }
        let digest = registered.identity.bound_definition_digest;
        let request_id = if let Some(parent) = &parent_id {
            validate_request_id(parent)?;
            if parent == &id {
                return Err(AppError::InvalidRequest(
                    "A message cannot reply to itself.".into(),
                ));
            }
            format!("{id}@{agent_key}#{parent}")
        } else if agent_key == AGENT {
            id.clone()
        } else {
            format!("{id}@{agent_key}")
        };
        let title = if input.len() <= 120 && !input.contains('\n') {
            input.clone()
        } else {
            let first_line = input.lines().next().unwrap_or(&input).trim();
            let truncated = if first_line.len() > 100 {
                let mut end = 100;
                while !first_line.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &first_line[..end])
            } else {
                first_line.to_string()
            };
            if truncated.is_empty() {
                "Delegated Task".to_string()
            } else {
                truncated
            }
        };
        if let Some(existing) = resources
            .get_team_work_item(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
                id.clone(),
            )
            .await
            .map_err(resource)?
        {
            if existing.request_id != request_id
                || existing.purpose != purpose
                || existing.title != title
                || existing.input.as_deref() != Some(input.as_str())
            {
                return Err(AppError::InvalidRequest(
                    "This request ID already belongs to different input or a different agent."
                        .into(),
                ));
            }
            let pinned = self.work_team(&id).await?;
            let expected = if let Some(parent) = &parent_id {
                self.work_team(parent).await?.map(|t| WorkTeamSelection {
                    id: t.id,
                    revision: t.revision,
                })
            } else {
                work_team.clone()
            };
            if pinned.as_ref().map(|t| WorkTeamSelection {
                id: t.id.clone(),
                revision: t.revision,
            }) != expected
                || work_team
                    .as_ref()
                    .is_some_and(|s| expected.as_ref() != Some(s))
            {
                return Err(AppError::InvalidRequest(
                    "This request already belongs to a different team roster.".into(),
                ));
            }
            if existing.run_id.is_some() {
                return self.project_task(existing).await;
            }
        }
        let mut conversation_input = self
            .conversation_input(&id, &agent_key, parent_id.as_deref(), &input)
            .await?;
        // Reject missing credentials/models before creating an unstarted record.
        // Confirmed retries already returned above and retain their original run.
        self.services
            .check_limits(agent.max_steps, agent.max_seconds, agent.max_tokens)?;
        let mut settings = self.services.agent_execution_settings(&agent).await?;
        let work = resources
            .create_work_with_roster(
                &self.services.host.credential,
                crate::resources::CreateTeamWorkItem {
                    org: app_scope.organization().into(),
                    team: app_scope.team().into(),
                    work_id: id.clone(),
                    title,
                    request_id,
                    goal_id: None,
                },
                Some(input.clone()),
                purpose,
                Some((work_team, parent_id.clone())),
            )
            .await
            .map_err(resource)?;
        if work.run_id.is_some() {
            return self.project_task(work).await;
        }
        let mut director = None;
        let planning = self.planning_ids().await?.contains_key(&id);
        if planning {
            if purpose != WorkPurpose::Explore || agent_key != shaping::GUIDE || parent_id.is_some()
            {
                return Err(AppError::InvalidRequest("A planning request must use the configured Guide without conversation history.".into()));
            }
            settings.response_schema = Some(plans::response_schema());
        }
        if purpose == WorkPurpose::Explore {
            settings.mcp = None;
            settings.allowed_tools.clear();
            settings.workspace_root = None;
            if !planning {
                conversation_input = self.director_input(&id, conversation_input).await?;
                let (binding, receiver) = self.bind_director(&id).await?;
                settings.limits.work_director = true;
                settings
                    .allowed_tools
                    .insert(crate::resources::work_director::CONTROL.into());
                settings.plan_dispatch = Some(binding);
                director = Some(receiver);
            }
        }
        // New work inherits an explicit allowance from existing agent limits,
        // optionally narrowed by the owner's workspace default. Retries retain
        // the original funded amount even if defaults have since changed.
        let budget_id = id.clone();
        let budget = self
            .services
            .local
            .store()
            .read(move |db| {
                db.work_budget_if_present(
                    app_scope.principal(),
                    app_scope.organization(),
                    app_scope.team(),
                    &budget_id,
                )
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|e| resource(e.into()))?;
        let app_scope = self.services.authorized_scope().await?;
        if budget.is_none() {
            let setting = resources
                .team_budget_setting(
                    &self.services.host.credential,
                    app_scope.organization().into(),
                    app_scope.team().into(),
                )
                .await
                .map_err(resource)?;
            let allowance = setting
                .token_limit
                .unwrap_or(agent.max_tokens as i64)
                .min(agent.max_tokens as i64);
            resources
                .authorize_work_budget(
                    &self.services.host.credential,
                    app_scope.organization().into(),
                    app_scope.team().into(),
                    id.clone(),
                    format!("allowance/{id}"),
                    allowance,
                )
                .await
                .map_err(resource)?;
        }
        if parent_id.is_some() {
            settings.limits.max_input_bytes = conversations::CONVERSATION_LIMIT;
        }
        if purpose == WorkPurpose::Explore && !planning {
            settings.limits.max_input_bytes = conversations::CONVERSATION_LIMIT + 18_000;
        }
        let prepared = resources
            .prepare_general_revision(
                &self.services.host.credential,
                app_scope.organization().into(),
                agent_key.clone(),
                digest.clone(),
                conversation_input.clone(),
                settings.limits.clone(),
            )
            .await
            .map_err(resource)?;
        let grant_id = self
            .submission_grant(
                &id,
                &work.request_id,
                prepared
                    .start_command(id.clone())
                    .map_err(resource)?
                    .job_spec,
            )
            .await?;
        let (work, submission) = self
            .services
            .host
            .app
            .activate_team_work(
                &self.services.host.credential,
                self.services.host.verifier.clone(),
                TeamWorkLaunch {
                    organization_id: app_scope.organization().into(),
                    team_id: app_scope.team().into(),
                    work_id: id.clone(),
                    information_context_id: self.services.scope.context().to_owned(),
                    agent_key,
                    definition_digest: digest,
                    execution_grant_id: grant_id,
                    input: Some(conversation_input),
                    recovery_id: id,
                },
                settings,
            )
            .await?;
        // The shared Application owns execution. A disconnected browser cannot cancel it.
        if let Some(execution) = submission.execution {
            if let Some(receiver) = director {
                self.serve_director(execution, receiver);
            } else {
                tokio::task::spawn_local(async move {
                    let _ = execution.completion.await;
                });
            }
        }
        self.project_task(work).await
    }

    pub async fn cancel(&self, id: &str) -> Result<LocalTask, AppError> {
        let task = self.task(id).await?;
        if let Some(run_id) = task.run_id {
            self.services
                .host
                .app
                .run_manager
                .managed()
                .cancel_run(&tetonic_domain::RunId::new(run_id))
                .await
                .map_err(|_| {
                    AppError::InvalidRequest("Cancellation could not be confirmed.".into())
                })?;
        }
        self.task(id).await
    }
}
