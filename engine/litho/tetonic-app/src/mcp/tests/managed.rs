use super::*;
use crate::local_workspace::{CreateLocalAgent, LocalWorkspace};

#[tokio::test]
async fn created_agents_use_only_selected_mcp_tools_through_managed_execution() {
    managed_tool_execution("search").await;
}

#[tokio::test]
async fn created_agents_execute_selected_action_tools_and_cannot_use_unselected_actions() {
    managed_tool_execution("delete_event").await;
}

async fn managed_tool_execution(tool_name: &str) {
    let feedback = if tool_name == "search" {
        "Tuesday 10:00 is available"
    } else {
        "Event deleted"
    };
    for (selected, with_files, stop) in [
        (true, false, false),
        (false, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let mut fixture = Fixture::new().await;
        let mut config: Value = serde_json::from_slice(&fixture.config).unwrap();
        config["connections"][0]["action_tools"] = json!(["delete_event"]);
        fixture.config = serde_json::to_vec(&config).unwrap();
        let catalog = McpRegistry::from_json(&fixture.config)
            .unwrap()
            .refresh("calendar")
            .await
            .unwrap();
        let search = catalog
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .unwrap()
            .id
            .clone();
        let (url, requests, server) = crate::tui_mvp_tests::inference_server_with_tool(
            true,
            &search,
            json!({"query":"Tuesday availability"}),
        )
        .await;
        tokio::task::LocalSet::new()
            .run_until(async {
                let dir = tempfile::tempdir().unwrap();
                let workspace = LocalWorkspace::open_with_workspace(
                    dir.path().join("state.db"),
                    "qwen3.5:latest".into(),
                    url,
                    with_files.then(|| dir.path().to_path_buf()),
                )
                .await
                .unwrap()
                .with_mcp_config(&fixture.config)
                .unwrap();
                workspace.discover_mcp("calendar").await.unwrap();
                let agent = workspace
                    .create_agent(CreateLocalAgent {
                        workspace_root: None,
                        provider: "ollama".into(),
                        hosted_consent: false,
                        hosted_tools_consent: false,
                        expected_workspace_root: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: "Calendar agent".into(),
                        purpose: "Use only granted calendar tools".into(),
                        model: "qwen3.5:latest".into(),
                        harness: "general".into(),
                        max_steps: 4,
                        max_seconds: 30,
                        max_tokens: 1024,
                        tools: Some(if selected {
                            vec![search.clone()]
                        } else {
                            vec![]
                        }),
                    })
                    .await
                    .unwrap();
                let id = uuid::Uuid::new_v4().to_string();
                if stop {
                    fixture.mode.store(3, Ordering::SeqCst);
                }
                workspace
                    .submit_for_agent(
                        id.clone(),
                        format!("Use the selected {tool_name} tool"),
                        agent.key.clone(),
                    )
                    .await
                    .unwrap();
                if stop {
                    tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        while !fixture
                            .calls
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|r| r["method"] == "tools/call")
                        {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .unwrap();
                    workspace.cancel(&id).await.unwrap();
                }
                let task = tokio::time::timeout(std::time::Duration::from_secs(15), async {
                    loop {
                        let task = workspace.task(&id).await.unwrap();
                        if matches!(task.state.as_str(), "completed" | "failed" | "canceled") {
                            break task;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(
                    task.state,
                    if stop { "canceled" } else { "completed" },
                    "{:?}",
                    task.error
                );
                let calls = requests.lock().unwrap().clone();
                let names = calls[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["function"]["name"].as_str().unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(names.contains(&search.as_str()), selected);
                assert!(!names.iter().any(|n| n.contains("delete_event")));
                assert_eq!(
                    fixture
                        .calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|r| r["method"] == "tools/call")
                        .count(),
                    usize::from(selected)
                );
                assert_eq!(
                    serde_json::to_string(&calls).unwrap().contains(feedback),
                    selected && !stop
                );
                if selected && !stop {
                    assert!(task.messages.iter().any(|m| m.content.contains(feedback)));
                }
                workspace
                    .submit_for_agent(id, format!("Use the selected {tool_name} tool"), agent.key)
                    .await
                    .unwrap();
                assert_eq!(
                    requests.lock().unwrap().len(),
                    if stop { 1 } else { 2 },
                    "retry must not rerun inference or the MCP tool"
                );
            })
            .await;
        server.abort();
    }
}
