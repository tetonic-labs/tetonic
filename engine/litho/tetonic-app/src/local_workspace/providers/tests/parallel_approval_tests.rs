//! Identical commands from concurrent agents still require separate owner decisions.
use super::*;
use std::time::Duration;

struct Transport;

#[async_trait::async_trait]
impl HostedTransport for Transport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        // Respond to each agent's own history, independent of arrival order.
        let has_result = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call_output");
        Ok(if has_result {
            parity::completion(
                "openai",
                "finish",
                json!({"summary":"Command finished"}),
                "done",
            )
        } else {
            parity::completion(
                "openai",
                "run_shell",
                json!({"command":"echo terminal-proof>>receipt.txt"}),
                "same-call-id",
            )
        })
    }
}

pub(super) fn agent_request(name: &str, provider: &str, root: Option<String>) -> CreateLocalAgent {
    let hosted = provider != "ollama";
    CreateLocalAgent {
        provider: provider.into(),
        hosted_consent: hosted,
        hosted_tools_consent: hosted,
        expected_workspace_root: root,
        request_id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        purpose: "Use only explicitly approved commands".into(),
        model: if hosted {
            "fixture-model"
        } else {
            "qwen3.5:latest"
        }
        .into(),
        harness: "general".into(),
        max_steps: 3,
        max_seconds: 60,
        max_tokens: 1024,
        tools: Some(vec!["run_shell".into()]),
    }
}

pub(super) async fn pending(
    workspace: &LocalWorkspace,
    count: usize,
) -> Vec<tetonic_memory::EffectApproval> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let rows = workspace.approvals().await.unwrap().pending_approvals;
            if rows.len() == count {
                break rows;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("all independent agents must reach their own approval")
}

#[tokio::test]
async fn concurrent_identical_shell_calls_cannot_share_an_approval() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("work");
            std::fs::create_dir(&root).unwrap();
            let mut workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("state.db"),
                "offline".into(),
                "http://127.0.0.1:1".into(),
                Some(root.clone()),
            )
            .await
            .unwrap();
            workspace.keys = Arc::new(ProviderKeys {
                store: workspace.keys.store.clone(),
                vault: Arc::new(Vault::default()),
            });
            workspace
                .save_provider_key(SaveProviderKey {
                    provider: "openai".into(),
                    api_key: "fixture-key".into(),
                })
                .await
                .unwrap();
            workspace.hosted_transport = Some(Arc::new(Transport));
            let catalog = workspace.agent_catalog().await.unwrap();
            let mut work_ids = Vec::new();
            for name in ["First operator", "Second operator"] {
                let agent = workspace
                    .create_agent(agent_request(
                        name,
                        "openai",
                        catalog.workspace_root.clone(),
                    ))
                    .await
                    .unwrap();
                let id = uuid::Uuid::new_v4().to_string();
                workspace
                    .submit_for_agent(id.clone(), "Write the receipt".into(), agent.key)
                    .await
                    .unwrap();
                work_ids.push(id);
            }
            let rows = pending(&workspace, 2).await;
            let first = rows
                .iter()
                .find(|a| a.work_id.as_ref() == Some(&work_ids[0]))
                .unwrap();
            let second = rows
                .iter()
                .find(|a| a.work_id.as_ref() == Some(&work_ids[1]))
                .unwrap();
            let (a, b) = (
                first.proposal.as_ref().unwrap(),
                second.proposal.as_ref().unwrap(),
            );
            assert_eq!(a.command, b.command);
            assert_eq!(a.call_id, b.call_id);
            assert_eq!(a.parameter_digest, b.parameter_digest);
            assert_ne!(a.attempt_id, b.attempt_id);
            assert_ne!(first.proposal_digest, second.proposal_digest);
            assert!(!root.join("receipt.txt").exists());

            assert!(
                workspace
                    .resolve_approval(
                        &second.approval_id,
                        ResolveApprovalRequest {
                            allow: true,
                            proposal_digest: first.proposal_digest.clone(),
                        }
                    )
                    .await
                    .is_err(),
                "one attempt's digest must not approve another attempt"
            );
            workspace
                .resolve_approval(
                    &first.approval_id,
                    ResolveApprovalRequest {
                        allow: true,
                        proposal_digest: first.proposal_digest.clone(),
                    },
                )
                .await
                .unwrap();
            assert_eq!(settled(&workspace, &work_ids[0]).await.state, "completed");
            // The other executor had time to poll the decision while the first finished.
            let remaining = workspace.approvals().await.unwrap().pending_approvals;
            assert_eq!(remaining.len(), 1);
            assert_eq!(remaining[0].approval_id, second.approval_id);
            assert_eq!(workspace.task(&work_ids[1]).await.unwrap().state, "running");
            assert_eq!(
                std::fs::read_to_string(root.join("receipt.txt"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            let replay = workspace
                .keys
                .store
                .write({
                    let row = first.clone();
                    move |db| {
                        db.consume_shell_approval(
                            ORG,
                            TEAM,
                            &row.approval_id,
                            &row.proposal_digest,
                            chrono::Utc::now().timestamp(),
                        )
                    }
                })
                .await
                .unwrap();
            assert!(
                !matches!(replay, Ok(Some(true))),
                "finished approval cannot be reused"
            );

            workspace.cancel(&work_ids[1]).await.unwrap();
            assert_eq!(settled(&workspace, &work_ids[1]).await.state, "canceled");
            assert!(workspace
                .resolve_approval(
                    &second.approval_id,
                    ResolveApprovalRequest {
                        allow: true,
                        proposal_digest: second.proposal_digest.clone(),
                    }
                )
                .await
                .is_err());
            assert_eq!(
                std::fs::read_to_string(root.join("receipt.txt"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
        })
        .await;
}
