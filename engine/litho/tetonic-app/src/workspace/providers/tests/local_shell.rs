//! Local inference uses the same selection, approval and process sink.
use super::*;

#[tokio::test]
async fn terminal_is_not_offered_when_control_storage_is_in_the_working_folder() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = LocalWorkspace::open_with_workspace(
        dir.path().join("state.db"),
        "offline".into(),
        "http://127.0.0.1:1".into(),
        Some(dir.path().to_path_buf()),
    )
    .await
    .unwrap();
    assert!(!workspace
        .services
        .host
        .settings
        .allowed_tools
        .contains("run_shell"));
    assert!(workspace
        .services
        .host
        .settings
        .allowed_tools
        .contains("read_file"));
}

#[tokio::test]
async fn local_shell_uses_owner_decisions_and_delivers_process_output() {
    for cancel_running in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        std::fs::create_dir(&root).unwrap();
        let command = if cancel_running {
            std::fs::write(root.join("slow.py"), "from pathlib import Path\nimport time\nPath('started').write_text('started')\ntime.sleep(2)\nPath('late').write_text('late')").unwrap();
            if cfg!(windows) {
                "python slow.py"
            } else {
                "python3 slow.py"
            }
        } else {
            "echo local-terminal-proof"
        };
        let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            "run_shell",
            json!({"command":command}),
            false,
        )
        .await;
        tokio::task::LocalSet::new()
            .run_until(async {
                let workspace = LocalWorkspace::open_with_workspace(
                    dir.path().join("state.db"),
                    "qwen3.5:latest".into(),
                    url,
                    Some(root.clone()),
                )
                .await
                .unwrap();
                let agent = workspace
                    .create_agent(CreateLocalAgent {
                        workspace_root: None,
                        provider: "ollama".into(),
                        hosted_consent: false,
                        hosted_tools_consent: false,
                        expected_workspace_root: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: "Local operator".into(),
                        purpose: "Use explicitly granted commands".into(),
                        model: "qwen3.5:latest".into(),
                        harness: "general".into(),
                        max_steps: 3,
                        max_seconds: 30,
                        max_tokens: 1024,
                        tools: Some(vec!["run_shell".into()]),
                    })
                    .await
                    .unwrap();
                let id = uuid::Uuid::new_v4().to_string();
                workspace
                    .submit_for_agent(id.clone(), "Echo the proof".into(), agent.key)
                    .await
                    .unwrap();
                let approval = tokio::time::timeout(std::time::Duration::from_secs(6), async {
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
                .unwrap();
                assert_eq!(calls.lock().unwrap().len(), 1);
                workspace
                    .resolve_approval(
                        &approval.approval_id,
                        ResolveApprovalRequest {
                            allow: true,
                            proposal_digest: approval.proposal_digest.clone(),
                        },
                    )
                    .await
                    .unwrap();
                if cancel_running {
                    tokio::time::timeout(std::time::Duration::from_secs(6), async {
                        while !root.join("started").exists() {
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        }
                    })
                    .await
                    .unwrap_or_else(|e| {
                        panic!(
                            "approved child process should start: {e:?}; {:?}",
                            calls.lock().unwrap()
                        )
                    });
                    let duplicate = workspace
                        .services
                        .keys
                        .store
                        .write(move |db| {
                            db.consume_shell_approval(
                                ORG,
                                TEAM,
                                &approval.approval_id,
                                &approval.proposal_digest,
                                chrono::Utc::now().timestamp(),
                            )
                        })
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        duplicate,
                        Some(false),
                        "already-running command approval is consumed"
                    );
                    tokio::time::timeout(std::time::Duration::from_secs(3), workspace.cancel(&id))
                        .await
                        .unwrap()
                        .unwrap();
                }
                let result = settled(&workspace, &id).await;
                if cancel_running {
                    assert_eq!(result.state, "canceled");
                    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
                    assert!(
                        !root.join("late").exists(),
                        "cancel must stop the shell's child process"
                    );
                    return;
                }
                assert_eq!(result.state, "completed", "{:?}", result.error);
                let requests = calls.lock().unwrap();
                let output = requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["role"] == "tool")
                    .unwrap();
                assert!(output["content"]
                    .as_str()
                    .unwrap()
                    .contains("local-terminal-proof"));
            })
            .await;
        server.abort();
    }
}
