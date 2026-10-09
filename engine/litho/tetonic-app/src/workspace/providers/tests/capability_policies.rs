//! Real registered agents, broker and files; only inference is a fixture.
use super::*;
use crate::local_workspace::{CapabilityScope, SaveCapabilityPolicy};
use tetonic_policy::capabilities::{AutonomyTier, CapabilityPolicy};

struct WriteTransport;
struct ConnectionTransport(String);
#[async_trait::async_trait]
impl HostedTransport for ConnectionTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let has_result = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["type"] == "function_call_output");
        Ok(if has_result {
            parity::completion(
                "openai",
                "finish",
                json!({"summary":"Connection checked"}),
                "done",
            )
        } else {
            parity::completion(
                "openai",
                &self.0,
                json!({"query":"Tuesday"}),
                "connection-call",
            )
        })
    }
}
#[async_trait::async_trait]
impl HostedTransport for WriteTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let has_result = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["type"] == "function_call_output");
        Ok(if has_result {
            parity::completion(
                "openai",
                "finish",
                json!({"summary":"Attempt finished"}),
                "done",
            )
        } else {
            parity::completion(
                "openai",
                "write_file",
                json!({"path":"receipt.txt","content":"approved work"}),
                "write-call",
            )
        })
    }
}
#[tokio::test]
async fn capability_policy_write_requires_exact_approval_and_live_deny_wins() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("files");
            std::fs::create_dir(&root).unwrap();
            let mut workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("db"),
                "offline".into(),
                "http://127.0.0.1:1".into(),
                Some(root.clone()),
            )
            .await
            .unwrap();
            workspace.services.keys = Arc::new(ProviderKeys {
                store: workspace.services.keys.store.clone(),
                vault: Arc::new(Vault::default()),
            });
            workspace
                .save_provider_key(SaveProviderKey {
                    provider: "openai".into(),
                    api_key: "fixture-key".into(),
                })
                .await
                .unwrap();
            workspace.services.hosted_transport = Some(Arc::new(WriteTransport));
            let mut request = parallel_approval_tests::agent_request(
                "Writer",
                "openai",
                workspace.agent_catalog().await.unwrap().workspace_root,
            );
            request.tools = Some(vec!["write_file".into()]);
            let agent = workspace.create_agent(request).await.unwrap();
            let policy = |scope, scope_id: String, revision, tier| SaveCapabilityPolicy {
                scope,
                scope_id,
                expected_revision: revision,
                request_id: uuid::Uuid::new_v4().to_string(),
                policy: Some(CapabilityPolicy {
                    tier,
                    ..Default::default()
                }),
            };
            workspace
                .save_capability_policy(policy(
                    CapabilityScope::Workspace,
                    "".into(),
                    0,
                    AutonomyTier::ReviewChanges,
                ))
                .await
                .unwrap();
            // A more permissive agent must not bypass the workspace approval.
            workspace
                .save_capability_policy(policy(
                    CapabilityScope::Agent,
                    agent.id.clone(),
                    0,
                    AutonomyTier::Automatic,
                ))
                .await
                .unwrap();
            for revoke in [true, false] {
                let id = uuid::Uuid::new_v4().to_string();
                workspace
                    .submit_for_agent(id.clone(), "Write receipt".into(), agent.key.clone())
                    .await
                    .unwrap();
                let approval = parallel_approval_tests::pending(&workspace, 1)
                    .await
                    .remove(0);
                let proposal = approval.proposal.as_ref().unwrap();
                assert_eq!(proposal.tool.as_deref(), Some("write_file"));
                assert!(proposal.command.contains("approved work"));
                assert!(
                    !root.join("receipt.txt").exists(),
                    "no write before approval"
                );
                if revoke {
                    workspace
                        .save_capability_policy(policy(
                            CapabilityScope::Workspace,
                            "".into(),
                            1,
                            AutonomyTier::ReadOnly,
                        ))
                        .await
                        .unwrap();
                }
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
                settled(&workspace, &id).await;
                assert_eq!(root.join("receipt.txt").exists(), !revoke);
                if revoke {
                    workspace
                        .save_capability_policy(policy(
                            CapabilityScope::Workspace,
                            "".into(),
                            2,
                            AutonomyTier::ReviewChanges,
                        ))
                        .await
                        .unwrap();
                }
            }
            assert_eq!(
                std::fs::read_to_string(root.join("receipt.txt")).unwrap(),
                "approved work"
            );
        })
        .await;
}

#[tokio::test]
async fn capability_policy_mcp_approval_binds_the_tool_and_endpoint_without_a_folder() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = crate::mcp::tests::fixture::Fixture::new().await;
            let dir = tempfile::tempdir().unwrap();
            let mut workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("db"),
                "offline".into(),
                "http://127.0.0.1:1".into(),
                None,
            )
            .await
            .unwrap()
            .with_mcp_config(&fixture.config)
            .unwrap();
            let connection = workspace.discover_mcp("calendar").await.unwrap();
            let tool = connection
                .tools
                .iter()
                .find(|t| t.name == "search")
                .unwrap()
                .id
                .clone();
            workspace.services.keys = Arc::new(ProviderKeys {
                store: workspace.services.keys.store.clone(),
                vault: Arc::new(Vault::default()),
            });
            workspace
                .save_provider_key(SaveProviderKey {
                    provider: "openai".into(),
                    api_key: "fixture-key".into(),
                })
                .await
                .unwrap();
            workspace.services.hosted_transport = Some(Arc::new(ConnectionTransport(tool.clone())));
            let mut request = parallel_approval_tests::agent_request("Scheduler", "openai", None);
            request.tools = Some(vec![tool.clone()]);
            let agent = workspace.create_agent(request).await.unwrap();
            workspace
                .save_capability_policy(SaveCapabilityPolicy {
                    scope: CapabilityScope::Agent,
                    scope_id: agent.id,
                    expected_revision: 0,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    policy: Some(CapabilityPolicy {
                        tier: AutonomyTier::ReviewChanges,
                        ..Default::default()
                    }),
                })
                .await
                .unwrap();
            let count = || {
                fixture
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|c| c["method"] == "tools/call")
                    .count()
            };
            // Declining a connection call must not send it to the MCP server.
            for allow in [false, true] {
                let id = uuid::Uuid::new_v4().to_string();
                workspace
                    .submit_for_agent(id.clone(), "Find a time".into(), agent.key.clone())
                    .await
                    .unwrap();
                let approval = parallel_approval_tests::pending(&workspace, 1)
                    .await
                    .remove(0);
                let proposal = approval.proposal.as_ref().unwrap();
                assert_eq!(proposal.tool.as_deref(), Some(tool.as_str()));
                assert_eq!(proposal.working_directory, format!("{}#{}", connection.endpoint, tool));
                assert!(proposal.command.contains("Tuesday"));
                assert_eq!(count(), 0);
                workspace
                    .resolve_approval(
                        &approval.approval_id,
                        ResolveApprovalRequest {
                            allow,
                            proposal_digest: approval.proposal_digest,
                        },
                    )
                    .await
                    .unwrap();
                settled(&workspace, &id).await;
                assert_eq!(count(), usize::from(allow));
            }
        })
        .await;
}
