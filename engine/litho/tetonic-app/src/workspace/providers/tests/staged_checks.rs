//! Real created agents, owner approvals, sandboxed commands and finalization;
//! only the provider's decisions are scripted. No live inference or paid usage.
use super::*;

struct CheckTransport {
    provider: &'static str,
    repair: bool,
    calls: Mutex<Vec<Value>>,
}
#[async_trait::async_trait]
impl HostedTransport for CheckTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(body.clone());
        let n = calls.len();
        let (tool, args) = match n {
            1 => (
                "write_file",
                json!({"path":"result.txt","content":"broken\n"}),
            ),
            2 | 4 => (
                "run_shell",
                json!({"view":"staged", "command": if cfg!(windows) { "findstr ready result.txt" } else { "grep -x ready result.txt" }}),
            ),
            3 => {
                let output = parity::output(&body, self.provider, "step-2");
                assert!(
                    output.contains("Check failed") && output.contains("exit code: 1"),
                    "{output}"
                );
                if self.repair {
                    (
                        "write_file",
                        json!({"path":"result.txt","content":"ready\n"}),
                    )
                } else {
                    (
                        "finish",
                        json!({"summary":"Model claims completion despite a failing check"}),
                    )
                }
            }
            5 => {
                let output = parity::output(&body, self.provider, "step-4");
                assert!(
                    output.contains("exit code: 0") && output.contains("ready"),
                    "{output}"
                );
                (
                    "finish",
                    json!({"summary":"Delivered result.txt after repairing and checking its exact pending contents."}),
                )
            }
            _ => panic!("unexpected fixture request {n}"),
        };
        Ok(parity::completion(
            self.provider,
            tool,
            args,
            &format!("step-{n}"),
        ))
    }
}

#[tokio::test]
async fn created_agents_check_repair_and_deliver_through_all_provider_adapters() {
    for provider in ["openai", "anthropic", "google"] {
        run(provider, true).await;
    }
}

#[tokio::test]
async fn model_claiming_done_after_failed_check_cannot_publish_pending_edits() {
    run("openai", false).await;
}

async fn run(provider: &'static str, repair: bool) {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("files");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("result.txt"), "original\n").unwrap();
            let mut workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("state.db"),
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
                    provider: provider.into(),
                    api_key: "fixture-key".into(),
                })
                .await
                .unwrap();
            let transport = Arc::new(CheckTransport {
                provider,
                repair,
                calls: Mutex::default(),
            });
            workspace.services.hosted_transport = Some(transport.clone());
            let agent = workspace
                .create_agent(CreateLocalAgent {
                    tool_pack_selection: None,
                    workspace_root: None,
                    expected_workspace_root: workspace
                        .agent_catalog()
                        .await
                        .unwrap()
                        .workspace_root,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    name: "Verifier".into(),
                    purpose: "Write, check, repair, deliver".into(),
                    provider: provider.into(),
                    model: "fixture-model".into(),
                    harness: "general".into(),
                    tools: Some(vec![
                        "write_file".into(),
                        "read_file".into(),
                        "run_shell".into(),
                    ]),
                    max_steps: 8,
                    max_seconds: 60,
                    max_tokens: 12288,
                    hosted_consent: true,
                    hosted_tools_consent: true,
                })
                .await
                .unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            workspace
                .submit_for_agent(id.clone(), "Produce and check the result".into(), agent.key)
                .await
                .unwrap();
            for _ in 0..if repair { 2 } else { 1 } {
                let approval = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    loop {
                        if let Some(a) = workspace
                            .approvals()
                            .await
                            .unwrap()
                            .pending_approvals
                            .into_iter()
                            .next()
                        {
                            break a;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "missing staged approval {provider}: {e}; calls={:?}",
                        transport.calls.lock().unwrap()
                    )
                });
                let proposal = approval.proposal.as_ref().unwrap();
                assert!(proposal.working_directory.ends_with("verify_overlay"));
                assert_eq!(proposal.digest(), approval.proposal_digest);
                assert_eq!(
                    std::fs::read_to_string(root.join("result.txt")).unwrap(),
                    "original\n",
                    "check cannot publish before finalization"
                );
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
            let result = settled(&workspace, &id).await;
            if repair {
                assert_eq!(result.state, "completed", "{:?}", result.error);
                assert_eq!(
                    std::fs::read_to_string(root.join("result.txt")).unwrap(),
                    "ready\n"
                );
            } else {
                assert_ne!(
                    result.state, "completed",
                    "a model's finish call is not a successful verification"
                );
                assert_eq!(
                    result.saved_drafts.len(),
                    1,
                    "failed finalization must expose retained output"
                );
                let draft = &result.saved_drafts[0];
                let calls_before = transport.calls.lock().unwrap().len();
                let review = workspace
                    .read_work_draft(
                        &id,
                        crate::work::ReadWorkDraft {
                            artifact_id: draft.artifact_id.clone(),
                            file: Some(0),
                            offset: 0,
                        },
                    )
                    .await
                    .unwrap();
                assert_eq!(review.files[0].path, "result.txt");
                assert_eq!(review.page.unwrap().content, "broken\n");
                assert_eq!(
                    transport.calls.lock().unwrap().len(),
                    calls_before,
                    "inspection cannot restart inference"
                );
                assert!(workspace
                    .read_work_draft(
                        &id,
                        crate::work::ReadWorkDraft {
                            artifact_id: "unrelated-artifact".into(),
                            file: None,
                            offset: 0,
                        }
                    )
                    .await
                    .is_err());
                assert_eq!(
                    std::fs::read_to_string(root.join("result.txt")).unwrap(),
                    "original\n"
                );
                assert!(
                    std::fs::read_dir(root.join(".lokai/artifacts"))
                        .unwrap()
                        .any(|e| e
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with("draft-")),
                    "rejected output remains available for review"
                );
                let artifact = draft.artifact_id.clone();
                drop(workspace);
                let reopened = LocalWorkspace::open_with_workspace(
                    dir.path().join("state.db"),
                    "offline".into(),
                    "http://127.0.0.1:1".into(),
                    Some(root.clone()),
                )
                .await
                .unwrap();
                let review = reopened
                    .read_work_draft(
                        &id,
                        crate::work::ReadWorkDraft {
                            artifact_id: artifact.clone(),
                            file: Some(0),
                            offset: 0,
                        },
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    review.page.unwrap().content,
                    "broken\n",
                    "draft inspection survives a new engine instance"
                );
                let meta = reopened
                    .services
                    .host
                    .app
                    .host
                    .runtime
                    .artifact_store()
                    .metadata(&tetonic_domain::ArtifactId::new(artifact.clone()))
                    .await
                    .unwrap();
                if let tetonic_domain::artifact::ArtifactLocation::LocalFile(path) =
                    meta.storage_location
                {
                    std::fs::write(path, b"tampered").unwrap();
                } else {
                    panic!("expected a local artifact");
                }
                assert!(
                    reopened
                        .read_work_draft(
                            &id,
                            crate::work::ReadWorkDraft {
                                artifact_id: artifact,
                                file: Some(0),
                                offset: 0,
                            }
                        )
                        .await
                        .is_err(),
                    "changed artifact bytes cannot masquerade as retained work"
                );
            }
        })
        .await;
}
