use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSettingsRequest {
    pub request_id: String,
    pub expected_revision: i64,
    pub token_limit: Option<i64>,
}

impl LocalWorkspace {
    pub async fn set_budget_settings(
        &self,
        request: BudgetSettingsRequest,
    ) -> Result<tetonic_memory::TeamBudgetSetting, AppError> {
        validate_request_id(&request.request_id)?;
        let max = self.host.settings.reported_token_ceiling.unwrap_or(4096);
        if request
            .token_limit
            .is_some_and(|v| v <= 0 || v as u64 > max)
        {
            return Err(AppError::InvalidRequest(format!(
                "Choose an allowance between 1 and {max} tokens."
            )));
        }
        self.local
            .resources()
            .set_team_budget_setting(
                &self.host.credential,
                ORG.into(),
                TEAM.into(),
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
        let work_id = work_id.to_string();
        self.keys
            .store
            .read(move |db| db.get_local_work_data(&work_id))
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|_| AppError::InferenceUnavailable)
    }

    pub async fn set_local_work_data(
        &self,
        work_id: &str,
        notes: Option<Vec<String>>,
        status: Option<String>,
        lead_id: Option<String>,
        agent_ids: Option<Vec<String>>,
    ) -> Result<(), AppError> {
        let work_id = work_id.to_string();
        self.keys
            .store
            .write(move |db| {
                db.set_local_work_data(
                    &work_id,
                    notes.as_deref(),
                    status.as_deref(),
                    lead_id.as_deref(),
                    agent_ids.as_deref(),
                )
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|_| AppError::InferenceUnavailable)
    }

    pub async fn get_work_item_notes(&self, work_id: &str) -> Result<Vec<String>, AppError> {
        let work_id = work_id.to_string();
        self.keys
            .store
            .read(move |db| db.get_local_work_notes(&work_id))
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|_| AppError::InferenceUnavailable)
    }

    pub async fn set_work_item_notes(
        &self,
        work_id: &str,
        notes: Vec<String>,
    ) -> Result<(), AppError> {
        let work_id = work_id.to_string();
        self.keys
            .store
            .write(move |db| db.set_local_work_notes(&work_id, &notes))
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|_| AppError::InferenceUnavailable)
    }

    pub async fn work_items(&self) -> Result<Vec<LocalWorkItem>, AppError> {
        let items = self
            .local
            .resources()
            .list_team_work_items(&self.host.credential, ORG.into(), TEAM.into())
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
            let local_data = self
                .get_local_work_data(&item.work_id)
                .await
                .unwrap_or_default();
            let plan_link = self.plan_link(&item.work_id).await?;
            let agent_key = plan_link
                .as_ref()
                .map(|link| link.agent_key.clone())
                .or(agent_key);
            let status = if plan_link.is_some() {
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
                agent_key: lead_id.clone().or(agent_key),
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
            .local
            .resources()
            .create_team_work_item(
                &self.host.credential,
                crate::resources::CreateTeamWorkItem {
                    org: ORG.into(),
                    team: TEAM.into(),
                    work_id: req.id.clone(),
                    title: title.clone(),
                    request_id: request_id.clone(),
                    goal_id: req.goal_id.clone(),
                },
            )
            .await
            .map_err(resource)?;
        if lead_id.is_some() || agent_ids.is_some() {
            let _ = self
                .set_local_work_data(&req.id, None, None, lead_id.clone(), agent_ids.clone())
                .await;
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
        let inspection = self
            .local
            .resources()
            .inspect_team_work(&self.host.credential, ORG.into(), TEAM.into())
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
        let now_unix = chrono::Utc::now().timestamp();
        self.local
            .resources()
            .resolve_effect_approval(
                &self.host.credential,
                crate::resources::ResolveEffectApproval {
                    org: ORG.into(),
                    team: TEAM.into(),
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
        let team = self
            .local
            .resources()
            .get_team(&self.host.credential, ORG.into(), TEAM.into())
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
