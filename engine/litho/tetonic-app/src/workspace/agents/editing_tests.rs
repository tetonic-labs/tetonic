use super::*;

#[tokio::test]
async fn edits_keep_identity_preserve_old_execution_and_run_new_tools_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("files");
    std::fs::create_dir(&root).unwrap();
    let database = dir.path().join("control.db");
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({"summary":"Done"}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_workspace(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                Some(root.clone()),
            )
            .await
            .unwrap();
            let original = workspace
                .services
                .agents()
                .await
                .unwrap()
                .into_iter()
                .find(|a| a.key == AGENT)
                .unwrap();
            let request = UpdateLocalAgent {
                agent_key: original.key.clone(),
                expected_definition_digest: original.definition_digest.clone(),
                configuration: CreateLocalAgent {
                    workspace_root: None,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    name: "Robin".into(),
                    purpose: "Use the blue-lantern method.".into(),
                    provider: "ollama".into(),
                    hosted_consent: false,
                    hosted_tools_consent: false,
                    expected_workspace_root: None,
                    model: original.model.clone(),
                    harness: "general".into(),
                    max_steps: 2,
                    max_seconds: 60,
                    max_tokens: 1024,
                    tools: Some(vec!["read_file".into(), "run_shell".into()]),
                },
            };
            let id = uuid::Uuid::new_v4().to_string();
            let first = workspace
                .submit(id.clone(), "Before edit".into())
                .await
                .unwrap();
            for bad_tools in [vec!["made_up_tool".into()], vec!["mcp_unknown".into()]] {
                let mut bad = request.clone();
                bad.configuration.tools = Some(bad_tools);
                assert!(workspace.update_agent(bad).await.is_err());
            }
            let mut hosted_without_consent = request.clone();
            hosted_without_consent.configuration.provider = "openai".into();
            hosted_without_consent.configuration.hosted_consent = true;
            assert!(workspace
                .update_agent(hosted_without_consent)
                .await
                .is_err());
            let changed = workspace.update_agent(request.clone()).await.unwrap();
            assert_eq!(changed.id, original.id);
            assert_eq!(changed.key, original.key);
            assert_eq!(changed.tools, vec!["read_file", "run_shell"]);
            assert_eq!(
                workspace
                    .update_agent(request.clone())
                    .await
                    .unwrap()
                    .definition_digest,
                changed.definition_digest
            );
            let mut stale = request.clone();
            stale.configuration.request_id = uuid::Uuid::new_v4().to_string();
            stale.configuration.name = "Stale".into();
            assert!(workspace.update_agent(stale).await.is_err());
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                while workspace.task(&id).await.unwrap().state != "completed" {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(workspace.task(&id).await.unwrap().run_id, first.run_id);
            let old_calls = calls.lock().unwrap().clone();
            assert!(!old_calls[0]["messages"]
                .to_string()
                .contains("blue-lantern"));
            assert!(!old_calls[0]["tools"].to_string().contains("run_shell"));
            drop(workspace);
            let reopened = LocalWorkspace::open_with_workspace(
                database,
                "qwen3.5:latest".into(),
                url,
                Some(root),
            )
            .await
            .unwrap();
            let profile = reopened
                .services
                .agents()
                .await
                .unwrap()
                .into_iter()
                .find(|a| a.key == AGENT)
                .unwrap();
            assert_eq!(profile.name, "Robin");
            assert_eq!(profile.definition_digest, changed.definition_digest);
            let next_id = uuid::Uuid::new_v4().to_string();
            reopened
                .submit(next_id.clone(), "After edit".into())
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                while reopened.task(&next_id).await.unwrap().state != "completed" {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            let new_calls = calls.lock().unwrap().clone();
            assert_eq!(new_calls.len(), 2);
            assert!(new_calls[1]["messages"]
                .to_string()
                .contains("blue-lantern"));
            assert!(new_calls[1]["tools"].to_string().contains("run_shell"));
            assert!(new_calls[1]["tools"].to_string().contains("read_file"));
            assert!(!new_calls[1]["tools"].to_string().contains("write_file"));
            assert_eq!(
                reopened
                    .services
                    .agents()
                    .await
                    .unwrap()
                    .iter()
                    .filter(|a| a.key == AGENT)
                    .count(),
                1
            );
        })
        .await;
    server.abort();
}
