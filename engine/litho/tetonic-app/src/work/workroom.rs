use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSettingsRequest {
    pub request_id: String,
    pub expected_revision: i64,
    #[serde(deserialize_with = "required_token_limit")]
    pub token_limit: Option<i64>,
}

// An omitted limit must never accidentally clear a saved allowance. `null` is
// the explicit reset command; the revision and request ID still guard the write.
fn required_token_limit<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}

impl WorkService {
    pub async fn set_budget_settings(
        &self,
        request: BudgetSettingsRequest,
    ) -> Result<tetonic_memory::TeamBudgetSetting, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(&request.request_id)?;
        let max = self
            .services
            .host
            .settings
            .reported_token_ceiling
            .unwrap_or(4096);
        if request
            .token_limit
            .is_some_and(|v| v <= 0 || v as u64 > max)
        {
            return Err(AppError::InvalidRequest(format!(
                "Choose an allowance between 1 and {max} tokens."
            )));
        }
        self.services
            .local
            .resources()
            .set_team_budget_setting(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
                request.request_id,
                request.expected_revision,
                request.token_limit,
            )
            .await
            .map_err(resource)
    }
    pub async fn get_local_work_data(
        &self,
        work_id: &str,
    ) -> Result<tetonic_memory::LocalWorkData, AppError> {
        validate_request_id(work_id)?;
        self.services
            .local
            .resources()
            .work_metadata(
                &self.services.host.credential,
                self.services.scope.clone(),
                work_id.into(),
            )
            .await
            .map_err(resource)
    }
    pub async fn set_local_work_data(
        &self,
        work_id: &str,
        notes: Option<Vec<String>>,
        status: Option<String>,
        lead_id: Option<String>,
        agent_ids: Option<Vec<String>>,
    ) -> Result<(), AppError> {
        validate_request_id(work_id)?;
        self.services
            .local
            .resources()
            .update_work_metadata(
                &self.services.host.credential,
                self.services.scope.clone(),
                work_id.into(),
                tetonic_memory::WorkMetadataPatch {
                    notes,
                    status,
                    lead_id,
                    agent_ids,
                },
            )
            .await
            .map_err(resource)
    }
    pub async fn get_work_item_notes(&self, work_id: &str) -> Result<Vec<String>, AppError> {
        Ok(self.get_local_work_data(work_id).await?.notes)
    }
    pub async fn set_work_item_notes(
        &self,
        work_id: &str,
        notes: Vec<String>,
    ) -> Result<(), AppError> {
        self.set_local_work_data(work_id, Some(notes), None, None, None)
            .await
    }

    pub async fn work_items(&self) -> Result<Vec<LocalWorkItem>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let items = self
            .services
            .local
            .resources()
            .list_team_work_items(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
            )
            .await
            .map_err(resource)?;
        let planning_ids = self.planning_ids().await?;
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            if planning_ids.contains_key(&item.work_id) {
                continue;
            }
            let agent_key = item
                .request_id
                .split_once('@')
                .map(|(_, key_part)| key_part.split_once('#').map_or(key_part, |(k, _)| k))
                .map(str::to_owned);
            let local_data = self.get_local_work_data(&item.work_id).await?;
            let plan_link = self.plan_link(&item.work_id).await?;
            let agent_key = plan_link
                .as_ref()
                .map(|link| link.agent_key.clone())
                .or(agent_key);
            // Presentation edits are not lifecycle commands. Once execution is
            // bound, use the same authorized journal projection as the inspector.
            // Summary reads deliberately skip transcript and artifact payload IO.
            let execution = if item.run_id.is_some() {
                Some(self.project_task_summary(item.clone()).await?)
            } else {
                None
            };
            let status = if let Some(task) = &execution {
                task.state.clone()
            } else if plan_link.is_some() {
                item.status
            } else {
                local_data.status.unwrap_or(item.status)
            };
            let lead_id = local_data.lead_id.clone();
            let agent_ids = local_data.agent_ids.clone();
            result.push(LocalWorkItem {
                id: item.work_id,
                title: item.title,
                status,
                agent_key: execution
                    .map(|task| task.agent_key)
                    .or_else(|| lead_id.clone().or(agent_key)),
                goal_id: item.goal_id,
                run_id: item.run_id,
                request_id: item.request_id,
                version: item.version,
                notes: local_data.notes,
                lead_id,
                agent_ids,
            });
        }
        Ok(result)
    }

    pub async fn create_work_item(
        &self,
        req: CreateWorkItemRequest,
    ) -> Result<LocalWorkItem, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(&req.id)?;
        let title = req.title.trim().to_string();
        if title.is_empty() || title.len() > 512 || title.contains('\0') {
            return Err(AppError::InvalidRequest(
                "Enter a title of 1–512 bytes.".into(),
            ));
        }
        let lead_id = req.lead_id.clone();
        let agent_ids = req.agent_ids.clone();
        let agent_key = lead_id
            .clone()
            .or_else(|| req.agent_key.clone())
            .unwrap_or_else(|| AGENT.into());
        let request_id = format!("{}@{}", req.id, agent_key);
        let item = self
            .services
            .local
            .resources()
            .create_team_work_item(
                &self.services.host.credential,
                crate::resources::CreateTeamWorkItem {
                    org: app_scope.organization().into(),
                    team: app_scope.team().into(),
                    work_id: req.id.clone(),
                    title: title.clone(),
                    request_id: request_id.clone(),
                    goal_id: req.goal_id.clone(),
                },
            )
            .await
            .map_err(resource)?;
        if lead_id.is_some() || agent_ids.is_some() {
            self.set_local_work_data(&req.id, None, None, lead_id.clone(), agent_ids.clone())
                .await?;
        }
        Ok(LocalWorkItem {
            id: item.work_id,
            title: item.title,
            status: item.status,
            agent_key: Some(agent_key),
            goal_id: item.goal_id,
            run_id: item.run_id,
            request_id: item.request_id,
            version: item.version,
            notes: Vec::new(),
            lead_id,
            agent_ids,
        })
    }

    pub async fn approvals(&self) -> Result<LocalApprovalsInspection, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let inspection = self
            .services
            .local
            .resources()
            .inspect_team_work(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
            )
            .await
            .map_err(resource)?;
        Ok(LocalApprovalsInspection {
            active_stops: inspection.active_stops,
            pending_approvals: inspection.pending_approvals,
            effort: inspection.effort,
        })
    }

    pub async fn resolve_approval(
        &self,
        approval_id: &str,
        req: ResolveApprovalRequest,
    ) -> Result<tetonic_memory::EffectApproval, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let now_unix = chrono::Utc::now().timestamp();
        self.services
            .local
            .resources()
            .resolve_effect_approval(
                &self.services.host.credential,
                crate::resources::ResolveEffectApproval {
                    org: app_scope.organization().into(),
                    team: app_scope.team().into(),
                    approval_id: approval_id.into(),
                    proposal_digest: req.proposal_digest,
                    allow: req.allow,
                    now_unix,
                },
            )
            .await
            .map_err(resource)
    }

    pub async fn teams(&self) -> Result<Vec<LocalTeamInfo>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let team = self
            .services
            .local
            .resources()
            .get_team(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
            )
            .await
            .map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("local team unavailable".into()))?;
        Ok(vec![LocalTeamInfo {
            id: team.team_id,
            name: team.name,
            org_id: team.org_id,
        }])
    }

    pub async fn digest(&self) -> Result<LocalDigestResponse, AppError> {
        let items = self.work_items().await?;
        let approvals = self.approvals().await?;
        let snapshot = self.snapshot().await?;

        let total = items.len();
        let completed = items.iter().filter(|i| i.status == "completed").count();
        let active = snapshot
            .tasks
            .iter()
            .filter(|t| matches!(t.state.as_str(), "running" | "starting"))
            .count();
        let pending_approvals = approvals.pending_approvals.len();

        let mut highlights = Vec::new();
        for task in snapshot.tasks.iter().take(5) {
            highlights.push(format!(
                "{}: [{}] {}",
                task.agent_name, task.state, task.input
            ));
        }

        let summary = if total == 0 && snapshot.tasks.is_empty() {
            "All systems ready. No active work items or tasks in flight.".to_string()
        } else {
            format!(
                "Team Standup: {} total work items ({} completed). Currently {} active task(s) in flight across {} agent(s). {} approval(s) requiring human authorization.",
                total, completed, active, snapshot.agents.len(), pending_approvals
            )
        };

        Ok(LocalDigestResponse {
            summary,
            total_work_items: total,
            completed_items: completed,
            active_items: active,
            pending_approvals_count: pending_approvals,
            highlights,
        })
    }
}
