//! Live Guide reads reuse scoped workspace owners, never a parallel inventory.
use super::*;

fn bounded(value: serde_json::Value) -> Result<serde_json::Value, AppError> {
    if value.to_string().len() > 36_000 {
        return Err(AppError::InvalidRequest("This inspection exceeds the conversation limit. Inspect a specific work item or use the workspace panels; no partial result was presented as complete.".into()));
    }
    Ok(value)
}

impl WorkService {
    pub(super) async fn director_resources(
        &self,
        source: &str,
    ) -> Result<serde_json::Value, AppError> {
        let scope = self.services.authorized_scope().await?;
        let snapshot = self.snapshot().await?;
        let selected = self.work_team(source).await?;
        let allowed = selected.as_ref().map(|team| team.agent_keys.as_slice());
        let eligible = snapshot
            .agents
            .iter()
            .filter(|a| {
                a.key != shaping::GUIDE
                    && !a.plan_coordinator
                    && allowed.is_none_or(|keys| keys.contains(&a.key))
            })
            .count();
        let connections = self.mcp_connections();
        let skills = self.workspace_skills()?;
        let approvals = self.approvals().await?;
        bounded(serde_json::json!({
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "scope": {"organization":scope.organization(),"team":scope.team()},
            "selected_team": selected,
            "agents": agent_observations(&snapshot, allowed),
            "agent_count": eligible,
            "teams": snapshot.work_teams.iter().take(24).collect::<Vec<_>>(),
            "team_count": snapshot.work_teams.len(),
            "skills": skills.iter().take(40).map(|s| serde_json::json!({"id":s.id,"name":s.name,"description":short(&s.description,240),"enabled":s.enabled})).collect::<Vec<_>>(),
            "connectors": connections.iter().take(24).map(|c| serde_json::json!({
                "id":c.id,"name":c.name,"enabled":c.enabled,"last_discovery_status":c.status,
                "tool_count":c.tools.len(),"tools":c.tools.iter().take(24).map(|t| serde_json::json!({"id":t.id,"name":t.name,"description":short(&t.description,200),"approved":t.approved})).collect::<Vec<_>>(),
                "partial":c.tools.len()>24
            })).collect::<Vec<_>>(),
            "execution_limits": self.services.execution,
            "usage": {
                "workspace_default_tokens_per_run":snapshot.budget_setting.token_limit,
                "host_max_tokens_per_run":snapshot.budget_max_tokens,
                "reported_tokens_including_discussions":snapshot.usage.iter().map(|u|u.input_tokens+u.output_tokens).sum::<i64>(),
                "held_tokens":snapshot.usage.iter().map(|u|u.held_tokens).sum::<i64>(),
                "unconfirmed_calls":snapshot.usage.iter().map(|u|u.pending_calls+u.unknown_calls).sum::<i64>(),
                "over_limit_work_count":snapshot.usage.iter().filter(|u|u.over_limit).count()
            },
            "active_work_records":snapshot.tasks.iter().filter(|t|t.purpose != WorkPurpose::Explore && active(&t.state)).count(),
            "pending_approvals":approvals.pending_approvals.len(),
            "active_stops":approvals.active_stops,
            "partial":eligible>24 || snapshot.work_teams.len()>24 || skills.len()>40 || connections.len()>24 || connections.iter().any(|c|c.tools.len()>24),
            "limits":"Inventory is not a grant. Match required tools and skills to each saved agent's tools; connectors have not been contacted by this inspection. Selected team membership is an assignment constraint. Limits are token/step/time allowances, not monetary or measured GPU/CPU capacity. Preserve dependency order; independent assignments can run in parallel. Do not treat workspace lifetime usage as a shared remaining allowance."
        }))
    }

    pub(super) async fn director_work(
        &self,
        work_id: Option<&str>,
    ) -> Result<serde_json::Value, AppError> {
        let scope = self.services.authorized_scope().await?;
        if let Some(id) = work_id {
            let task = self.task(id).await?;
            if task.purpose == WorkPurpose::Explore {
                return Err(AppError::PolicyDenied("Work inspection does not expose other Guide conversations. Use inspect for this conversation's proposal.".into()));
            }
            let usage = self
                .services
                .local
                .resources()
                .team_work_usage(
                    &self.services.host.credential,
                    scope.organization().into(),
                    scope.team().into(),
                )
                .await
                .map_err(resource)?;
            return bounded(serde_json::json!({
                "observed_at":chrono::Utc::now().to_rfc3339(),
                "work_id":task.id,"link":format!("#work={}",task.id),"state":task.state,
                "agent_key":task.agent_key,"agent_name":task.agent_name,"plan":task.plan,
                "result_excerpt":task.messages.iter().rev().find(|m|m.role=="assistant").map(|m|short(&m.content,4000)),
                "error":task.error,"human_questions":task.human_questions,
                "usage":usage.iter().find(|u|u.work_id==task.id),
                "limits":"Result excerpts are bounded and may be truncated. A reply is not proof of an external action; use the recorded work state and evidence."
            }));
        }
        let snapshot = self.snapshot().await?;
        // An empty focus deliberately lists all authorized execution work, not
        // the current conversation's proposal or unrelated private discussions.
        let mut value = observation(&snapshot, "", &scope);
        value.as_object_mut().unwrap().remove("agents");
        bounded(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_reads_use_saved_resources_and_exclude_private_discussions() {
        let dir = tempfile::tempdir().unwrap();
        tokio::task::LocalSet::new()
            .run_until(async {
                let workspace = LocalWorkspace::open(
                    dir.path().join("inspection.db"),
                    "offline".into(),
                    "http://127.0.0.1:1".into(),
                )
                .await
                .unwrap();
                let discussion = uuid::Uuid::new_v4().to_string();
                let execution = uuid::Uuid::new_v4().to_string();
                for (id, title, purpose, agent) in [
                    (
                        &discussion,
                        "PRIVATE_DISCUSSION_CANARY",
                        WorkPurpose::Explore,
                        shaping::GUIDE,
                    ),
                    (
                        &execution,
                        "Review the supplied report",
                        WorkPurpose::Work,
                        AGENT,
                    ),
                ] {
                    workspace
                        .services
                        .local
                        .resources()
                        .create_team_work_item_for_purpose(
                            &workspace.services.host.credential,
                            crate::resources::CreateTeamWorkItem {
                                org: ORG.into(),
                                team: TEAM.into(),
                                work_id: id.clone(),
                                title: title.into(),
                                request_id: format!("{id}@{agent}"),
                                goal_id: None,
                            },
                            Some(title.into()),
                            purpose,
                        )
                        .await
                        .unwrap();
                }
                let resources = workspace.director_resources(&discussion).await.unwrap();
                assert!(resources["agents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["key"] == AGENT));
                assert!(
                    resources["agents"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|a| a["key"] != shaping::GUIDE
                            && a["key"] != plan_execution::COORDINATOR)
                );
                assert_eq!(
                    resources["usage"]["reported_tokens_including_discussions"],
                    0
                );
                assert_eq!(
                    resources["execution_limits"]["max_tokens"],
                    workspace.services.execution.max_tokens
                );
                assert!(!resources.to_string().contains("PRIVATE_DISCUSSION_CANARY"));
                let all = workspace.director_work(None).await.unwrap();
                assert_eq!(all["work"].as_array().unwrap().len(), 1);
                assert_eq!(all["work"][0]["id"], execution);
                assert!(!all.to_string().contains("PRIVATE_DISCUSSION_CANARY"));
                let detail = workspace.director_work(Some(&execution)).await.unwrap();
                assert_eq!(detail["state"], "not_started");
                assert!(detail["result_excerpt"].is_null());
                assert!(workspace.director_work(Some(&discussion)).await.is_err());
                assert!(workspace
                    .director_work(Some(&uuid::Uuid::new_v4().to_string()))
                    .await
                    .is_err());
                assert_eq!(
                    workspace.snapshot().await.unwrap().tasks.len(),
                    2,
                    "inspection must not create work"
                );
                assert!(workspace
                    .plan_view(&discussion)
                    .await
                    .unwrap()
                    .plans
                    .is_empty());
            })
            .await;
    }
}
