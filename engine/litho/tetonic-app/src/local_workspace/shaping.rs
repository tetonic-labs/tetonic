use super::*;
pub use tetonic_memory::WorkPurpose;

pub(super) const GUIDE: &str = "The Guide";
pub(super) const GUIDE_INSTRUCTIONS: &str = "Help the owner understand and shape an unclear problem before deciding what to do. Explain concepts at their level, distinguish evidence from assumptions, compare alternatives and consequences. Start with the most useful distinction and ask at most two timely questions. Default to a response under 180 words; go deeper when the owner asks. Avoid long checklists, generic intake questionnaires and a plan before the problem is understood. Keep their decisions and open questions visible. When useful, propose a concise editable brief covering understanding, evidence, options, choices, constraints and next steps. Exploration may end without a plan or implementation. You have no tools except finish: you cannot research external systems, edit files, create assignments, grant access or dispatch agents. Name missing context or access specifically and offer a supplied-document alternative. Never claim a proposed action has happened. Call finish with your full response as the summary.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveWorkBrief {
    pub request_id: String,
    pub expected_revision: i64,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn exploration_reuses_managed_runs_without_tools_and_keeps_versioned_brief() {
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
            assert!(tools.iter().all(|t| t["function"]["name"]=="finish"));
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

impl LocalWorkspace {
    pub async fn work_briefs(&self, id: &str) -> Result<Vec<tetonic_memory::WorkBrief>, AppError> {
        validate_request_id(id)?;
        self.local
            .resources()
            .work_briefs(&self.host.credential, ORG.into(), TEAM.into(), id.into())
            .await
            .map_err(resource)
    }

    pub async fn save_work_brief(
        &self,
        id: &str,
        input: SaveWorkBrief,
    ) -> Result<tetonic_memory::WorkBrief, AppError> {
        validate_request_id(id)?;
        validate_request_id(&input.request_id)?;
        self.local.resources().save_work_brief(&self.host.credential,ORG.into(),TEAM.into(),id.into(),input.request_id,input.expected_revision,input.body).await.map_err(|e| match e {
            crate::resources::ResourceError::Conflict => AppError::InvalidRequest("This brief changed, or this save belongs to another edit. Reload the saved brief before applying your changes.".into()),
            other=>resource(other),
        })
    }
}
