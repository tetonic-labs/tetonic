//! Hosted and local teammates use the same saved identities and managed lineage.
use super::*;
use crate::local_workspace::{plan_execution::tests as plans, PlanCommand, StartPlan};

struct TeamTransport {
    provider: &'static str,
    tool: String,
    calls: Mutex<Vec<Value>>,
}
#[async_trait::async_trait]
impl HostedTransport for TeamTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(body.clone());
        let (tool, args, id) = match calls.len() {
            1 => ("read_file", json!({"path":"brief.txt"}), "file"),
            2 => {
                assert!(parity::output(&body, self.provider, "file").contains("TEAM_FILE_CANARY"));
                (
                    self.tool.as_str(),
                    json!({"query":"Tuesday availability"}),
                    "calendar",
                )
            }
            3 => {
                assert!(parity::output(&body, self.provider, "calendar").contains("Tuesday 10:00"));
                (
                    "run_shell",
                    json!({"command":"echo TEAM_SHELL_CANARY>team-receipt.txt"}),
                    "shell",
                )
            }
            _ => {
                let output = parity::output(&body, self.provider, "shell");
                assert!(!output.contains("denied"), "{output}");
                (
                    "finish",
                    json!({"summary":"COMPARE_RESULT TEAM_FILE_CANARY Tuesday 10:00"}),
                    "done",
                )
            }
        };
        Ok(parity::completion(self.provider, tool, args, id))
    }
}

#[tokio::test]
async fn mixed_provider_teams_keep_saved_model_file_and_mcp_grants() {
    for provider in ["openai", "anthropic", "google"] {
        tokio::task::LocalSet::new()
            .run_until(async {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path().join("approved");
                std::fs::create_dir(&root).unwrap();
                std::fs::write(root.join("brief.txt"), "TEAM_FILE_CANARY").unwrap();
                let fixture = crate::mcp::tests::fixture::Fixture::new().await;
                let (url, local_calls, server) = plans::scripted_server(5).await;
                let mut workspace = LocalWorkspace::open_with_workspace(
                    dir.path().join("state.db"),
                    "qwen3.5:latest".into(),
                    url,
                    Some(root.clone()),
                )
                .await
                .unwrap()
                .with_mcp_config(&fixture.config)
                .unwrap();
                workspace.discover_mcp("calendar").await.unwrap();
                let tool = workspace.agent_catalog().await.unwrap().mcp_connections[0]
                    .tools
                    .iter()
                    .find(|t| t.name == "search")
                    .unwrap()
                    .id
                    .clone();
                workspace.keys = Arc::new(ProviderKeys {
                    store: workspace.keys.store.clone(),
                    vault: Arc::new(Vault::default()),
                });
                workspace
                    .save_provider_key(SaveProviderKey {
                        provider: provider.into(),
                        api_key: "disposable-provider-key".into(),
                    })
                    .await
                    .unwrap();
                let transport = Arc::new(TeamTransport {
                    provider,
                    tool: tool.clone(),
                    calls: Mutex::default(),
                });
                workspace.hosted_transport = Some(transport.clone());
                let agent = workspace
                    .create_agent(CreateLocalAgent {
                        provider: provider.into(),
                        hosted_consent: true,
                        hosted_tools_consent: true,
                        expected_workspace_root: Some(
                            tetonic_tools::Workspace::new(&root)
                                .unwrap()
                                .root()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: "Calendar teammate".into(),
                        purpose: "Use the supplied tools to prepare the comparison".into(),
                        model: format!("{provider}-fixture"),
                        harness: "general".into(),
                        max_steps: 4,
                        max_seconds: 30,
                        max_tokens: 1024,
                        tools: Some(vec!["read_file".into(), tool.clone(), "run_shell".into()]),
                    })
                    .await
                    .unwrap();
                let workspace = std::rc::Rc::new(workspace);
                let source = plans::seed_options(&workspace, false, true).await;
                let mut content = workspace.plan_view(&source).await.unwrap().plans[0]
                    .content
                    .clone()
                    .unwrap();
                content.assignments[0].agent_key = agent.key.clone();
                content.assignments[0].tools = agent.tools.clone();
                workspace
                    .update_plan(
                        &source,
                        PlanCommand::Revise {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            expected_revision: 1,
                            brief_revision: 1,
                            content,
                        },
                    )
                    .await
                    .unwrap();
                workspace
                    .update_plan(
                        &source,
                        PlanCommand::Agree {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            revision: 2,
                        },
                    )
                    .await
                    .unwrap();
                workspace
                    .start_plan(
                        &source,
                        StartPlan {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            revision: 2,
                        },
                    )
                    .await
                    .unwrap();
                let outcome = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    loop {
                        if let Some(approval) = workspace
                            .approvals()
                            .await
                            .unwrap()
                            .pending_approvals
                            .into_iter()
                            .next()
                        {
                            assert!(!root.join("team-receipt.txt").exists());
                            workspace
                                .resolve_approval(
                                    &approval.approval_id,
                                    ResolveApprovalRequest {
                                        allow: true,
                                        proposal_digest: approval.proposal_digest,
                                    },
                                )
                                .await
                                .unwrap();
                        }
                        let view = workspace.execution_view(&source).await.unwrap().unwrap();
                        if matches!(view.state.as_str(), "completed" | "failed" | "canceled") {
                            break view;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(
                    outcome.state,
                    "completed",
                    "{provider}: {}",
                    serde_json::to_string(&outcome).unwrap()
                );
                assert_eq!(
                    outcome.receipt.assignments[0].definition_digest,
                    agent.definition_digest
                );
                let root_run = outcome.root.as_ref().unwrap().run_id.as_ref().unwrap();
                assert!(outcome
                    .assignments
                    .iter()
                    .all(|a| a.run_id.as_ref() == Some(root_run)));
                let calls = transport.calls.lock().unwrap();
                assert_eq!(calls.len(), 4);
                assert!(std::fs::read_to_string(root.join("team-receipt.txt"))
                    .unwrap()
                    .contains("TEAM_SHELL_CANARY"));
                let names = parity::names(&calls[0], provider);
                assert!(
                    names.contains(&"read_file")
                        && names.contains(&tool.as_str())
                        && names.contains(&"ask_human")
                );
                assert!(!names.contains(&"write_file") && !names.contains(&"dispatch_assignment"));
                assert!(!calls
                    .iter()
                    .any(|c| c.to_string().contains("PRIVATE_CANARY_MUST_NOT_LEAK")));
                assert!(local_calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r["tools"].to_string().contains("dispatch_assignment"))
                    .all(|r| !r["tools"].to_string().contains(&tool)));
                assert_eq!(
                    fixture
                        .calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|r| r["method"] == "tools/call")
                        .count(),
                    1
                );
                server.abort();
            })
            .await;
    }
}
