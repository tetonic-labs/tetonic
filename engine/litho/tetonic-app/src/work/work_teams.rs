use super::*;
pub use tetonic_memory::{SaveWorkTeam, WorkTeam, WorkTeamSelection};

impl WorkService {
    pub async fn work_teams(&self) -> Result<Vec<WorkTeam>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        self.services
            .local
            .resources()
            .work_teams(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
            )
            .await
            .map_err(resource)
    }

    pub async fn save_work_team(&self, request: SaveWorkTeam) -> Result<WorkTeam, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(&request.id)?;
        validate_request_id(&request.request_id)?;
        let agents = self.services.agents().await?;
        if request.agent_keys.iter().any(|key| {
            !agents
                .iter()
                .any(|a| &a.key == key && a.key != shaping::GUIDE && !a.plan_coordinator)
        }) {
            return Err(AppError::InvalidRequest(
                "Choose saved working agents for this team. The Guide coordinates separately."
                    .into(),
            ));
        }
        self.services.local.resources().save_work_team(&self.services.host.credential,app_scope.organization().into(),app_scope.team().into(),request).await.map_err(|e|match e {
            crate::resources::ResourceError::Conflict=>AppError::InvalidRequest("This team changed, or this save belongs to another edit. Reload the team before saving again.".into()),
            other=>resource(other),
        })
    }

    pub(super) async fn work_team(&self, work: &str) -> Result<Option<WorkTeam>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        self.services
            .local
            .resources()
            .work_team_for_work(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
                work.into(),
            )
            .await
            .map_err(resource)
    }

    pub(super) async fn planning_agents(&self, work: &str) -> Result<Vec<LocalAgent>, AppError> {
        let team = self.work_team(work).await?;
        Ok(self
            .services
            .agents()
            .await?
            .into_iter()
            .filter(|a| {
                a.key != shaping::GUIDE
                    && !a.plan_coordinator
                    && team.as_ref().is_none_or(|t| t.agent_keys.contains(&a.key))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn guide_receives_saved_roster_and_followups_keep_it_after_team_edit_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("teams.db");
        let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            "finish",
            serde_json::json!({"summary":"Here are the options."}),
            false,
        )
        .await;
        tokio::task::LocalSet::new()
            .run_until(async {
                let workspace =
                    LocalWorkspace::open(path.clone(), "qwen3.5:latest".into(), url.clone())
                        .await
                        .unwrap();
                let team = workspace
                    .save_work_team(SaveWorkTeam {
                        id: uuid::Uuid::new_v4().to_string(),
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 0,
                        name: "Research partners".into(),
                        purpose: "Review the available evidence".into(),
                        agent_keys: vec![AGENT.into()],
                    })
                    .await
                    .unwrap();
                let selection = WorkTeamSelection {
                    id: team.id.clone(),
                    revision: team.revision,
                };
                let id = uuid::Uuid::new_v4().to_string();
                assert!(workspace
                    .submit_to_team(
                        id.clone(),
                        "Explore".into(),
                        AGENT.into(),
                        None,
                        WorkPurpose::Work,
                        Some(selection.clone())
                    )
                    .await
                    .is_err());
                let first = workspace
                    .submit_to_team(
                        id.clone(),
                        "Compare two ideas".into(),
                        shaping::GUIDE.into(),
                        None,
                        WorkPurpose::Explore,
                        Some(selection.clone()),
                    )
                    .await
                    .unwrap();
                assert_eq!(first.work_team, Some(team.clone()));
                tokio::time::timeout(std::time::Duration::from_secs(15), async {
                    while workspace.task(&id).await.unwrap().state != "completed" {
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                })
                .await
                .unwrap();
                let prompt = calls.lock().unwrap()[0]["messages"].to_string();
                assert!(
                    prompt.contains("Research partners")
                        && prompt.contains("selected_team")
                        && prompt.contains("Review the available evidence")
                );
                workspace
                    .save_work_team(SaveWorkTeam {
                        id: team.id.clone(),
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 1,
                        name: "A new name".into(),
                        purpose: "New direction".into(),
                        agent_keys: team.agent_keys.clone(),
                    })
                    .await
                    .unwrap();
                assert!(workspace
                    .submit_to_team(
                        uuid::Uuid::new_v4().to_string(),
                        "New work".into(),
                        shaping::GUIDE.into(),
                        None,
                        WorkPurpose::Explore,
                        Some(selection.clone())
                    )
                    .await
                    .is_err());
                let retry = workspace
                    .submit_to_team(
                        id.clone(),
                        first.input.clone(),
                        shaping::GUIDE.into(),
                        None,
                        WorkPurpose::Explore,
                        Some(selection.clone()),
                    )
                    .await
                    .unwrap();
                assert_eq!(retry.run_id, first.run_id);
                assert_eq!(calls.lock().unwrap().len(), 1);
                drop(workspace);
                let reopened = LocalWorkspace::open(path, "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
                assert_eq!(reopened.work_teams().await.unwrap()[0].revision, 2);
                let reply = reopened
                    .submit_with_purpose(
                        uuid::Uuid::new_v4().to_string(),
                        "Explain the tradeoffs".into(),
                        shaping::GUIDE.into(),
                        Some(id),
                        WorkPurpose::Explore,
                    )
                    .await
                    .unwrap();
                assert_eq!(reply.work_team, Some(team));
                tokio::time::timeout(std::time::Duration::from_secs(15), async {
                    while reopened.task(&reply.id).await.unwrap().state != "completed" {
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                })
                .await
                .unwrap();
            })
            .await;
        server.abort();
    }
}
