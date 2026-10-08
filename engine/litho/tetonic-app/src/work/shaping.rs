use super::*;
pub(super) use crate::workspace::GUIDE;
pub use tetonic_memory::WorkPurpose;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveWorkBrief {
    pub request_id: String,
    pub expected_revision: i64,
    pub body: String,
}

impl WorkService {
    pub async fn work_briefs(&self, id: &str) -> Result<Vec<tetonic_memory::WorkBrief>, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(id)?;
        self.services
            .local
            .resources()
            .work_briefs(
                &self.services.host.credential,
                app_scope.organization().into(),
                app_scope.team().into(),
                id.into(),
            )
            .await
            .map_err(resource)
    }

    pub async fn save_work_brief(
        &self,
        id: &str,
        input: SaveWorkBrief,
    ) -> Result<tetonic_memory::WorkBrief, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        validate_request_id(id)?;
        validate_request_id(&input.request_id)?;
        self.services.local
            .resources()
            .save_work_brief(
                &self.services.host.credential,
                crate::resources::SaveWorkBrief {
                    org: app_scope.organization().into(),
                    team: app_scope.team().into(),
                    work: id.into(),
                    request: input.request_id,
                    expected: input.expected_revision,
                    body: input.body,
                },
            )
            .await
            .map_err(|e| match e {
            crate::resources::ResourceError::Conflict => AppError::InvalidRequest("This brief changed, or this save belongs to another edit. Reload the saved brief before applying your changes.".into()),
            other=>resource(other),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn exploration_reuses_managed_runs_without_execution_tools_and_keeps_versioned_brief() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("shaping.db");
        let root = tempfile::tempdir().unwrap();
        let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            "finish",
            serde_json::json!({"summary":"Two approaches are worth comparing."}),
            false,
        )
        .await;
        tokio::task::LocalSet::new().run_until(async {
            let workspace=LocalWorkspace::open_with_workspace(database.clone(),"qwen3.5:latest".into(),url.clone(),Some(root.path().to_path_buf())).await.unwrap();
            let id=uuid::Uuid::new_v4().to_string();
            assert!(workspace.submit_with_purpose(id.clone(),"Explore".into(),AGENT.into(),None,WorkPurpose::Explore).await.is_err());
            let first=workspace.submit_with_purpose(id.clone(),"I do not yet know which approach fits.".into(),GUIDE.into(),None,WorkPurpose::Explore).await.unwrap();
            assert_eq!(first.purpose,WorkPurpose::Explore);
            tokio::time::timeout(std::time::Duration::from_secs(20),async {
                loop {let task=workspace.task(&id).await.unwrap();if task.state=="completed" {break;}assert_ne!(task.state,"failed");tokio::time::sleep(std::time::Duration::from_millis(30)).await;}
            }).await.unwrap();
            let requests=calls.lock().unwrap().clone();
            let tools=requests[0]["tools"].as_array().unwrap();
            assert!(tools.iter().all(|t| matches!(t["function"]["name"].as_str(), Some("finish" | "work_plan"))));
            assert!(requests[0]["messages"].to_string().contains("compare alternatives"));
            assert!(workspace.submit(id.clone(),first.input.clone()).await.is_err());
            let saved=workspace.save_work_brief(&id,SaveWorkBrief {request_id:uuid::Uuid::new_v4().to_string(),expected_revision:0,body:"Consider the copper-lantern option first; no implementation approved.".into()}).await.unwrap();
            assert_eq!(saved.revision,1);
            assert_eq!(calls.lock().unwrap().len(),1);
            assert_eq!(workspace.snapshot().await.unwrap().tasks.len(),1);
            let prompt=workspace.conversation_input(&uuid::Uuid::new_v4().to_string(),GUIDE,Some(&id),"Explain the tradeoffs").await.unwrap();
            assert!(prompt.contains("copper-lantern"));
            assert!(prompt.contains("draft; not execution authority"));
            assert!(workspace.submit_with_purpose(uuid::Uuid::new_v4().to_string(),"Implement".into(),GUIDE.into(),Some(id.clone()),WorkPurpose::Work).await.is_err());
            drop(workspace);
            let reopened=LocalWorkspace::open(database,"qwen3.5:latest".into(),url).await.unwrap();
            assert_eq!(reopened.work_briefs(&id).await.unwrap(),vec![saved]);
            let retry=reopened.submit_with_purpose(id.clone(),first.input,GUIDE.into(),None,WorkPurpose::Explore).await.unwrap();
            assert_eq!(retry.run_id,first.run_id);
            assert_eq!(calls.lock().unwrap().len(),1);
        }).await;
        server.abort();
    }
}
