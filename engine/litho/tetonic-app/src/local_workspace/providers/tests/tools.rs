use super::*;

struct FileTransport {
    calls: Mutex<Vec<Value>>,
    tool: String,
    path: String,
}
#[async_trait::async_trait]
impl HostedTransport for FileTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(body);
        let (name, args, id) = if calls.len() == 1 {
            (
                self.tool.as_str(),
                if self.tool == "write_file" {
                    json!({"path":self.path,"content":"must not write"})
                } else {
                    json!({"path":self.path})
                },
                "real-provider-id",
            )
        } else {
            let input = calls.last().unwrap()["input"].as_array().unwrap();
            let result = input
                .iter()
                .find(|item| item["type"] == "function_call_output")
                .expect("actual result must be returned");
            assert_eq!(result["call_id"], "real-provider-id");
            (
                "finish",
                json!({"summary":result["output"].as_str().unwrap()}),
                "finish-id",
            )
        };
        Ok(
            json!({"status":"completed","output":[{"type":"function_call","call_id":id,"name":name,"arguments":args.to_string()}],"usage":{"input_tokens":20,"output_tokens":20}}),
        )
    }
}

#[tokio::test]
async fn hosted_reads_use_selected_tools_exact_folder_and_actual_provider_result_ids() {
    for (tool, path, contents) in [
        ("read_file", "notes.txt", "The orchard has seventeen trees."),
        ("read_file", "../outside.txt", "OUTSIDE MUST NOT LEAK"),
        ("write_file", "forbidden.txt", ""),
        ("read_file", "notes.txt", "AKIAIOSFODNN7EXAMPLE"),
    ] {
        tokio::task::LocalSet::new()
            .run_until(async {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path().join("approved");
                std::fs::create_dir(&root).unwrap();
                std::fs::write(root.join("notes.txt"), contents).unwrap();
                std::fs::write(dir.path().join("outside.txt"), "OUTSIDE MUST NOT LEAK").unwrap();
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
                let transport = Arc::new(FileTransport {
                    calls: Mutex::default(),
                    tool: tool.into(),
                    path: path.into(),
                });
                workspace.hosted_transport = Some(transport.clone());
                workspace
                    .save_provider_key(SaveProviderKey {
                        provider: "openai".into(),
                        api_key: "disposable-provider-key".into(),
                    })
                    .await
                    .unwrap();
                let input = CreateLocalAgent {
                    provider: "openai".into(),
                    hosted_consent: true,
                    hosted_tools_consent: false,
                    expected_workspace_root: Some(
                        tetonic_tools::Workspace::new(&root)
                            .unwrap()
                            .root()
                            .to_str()
                            .unwrap()
                            .to_owned(),
                    ),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    name: "Reader".into(),
                    purpose: "Read the assigned document with selected tools.".into(),
                    model: "configured-model".into(),
                    harness: "general".into(),
                    max_steps: 3,
                    max_seconds: 30,
                    max_tokens: 1024,
                    tools: Some(vec!["read_file".into()]),
                };
                assert!(
                    workspace.create_agent(input.clone()).await.is_err(),
                    "file disclosure requires explicit consent"
                );
                for stale_root in [None, Some("a-different-folder".into())] {
                    assert!(
                        workspace
                            .create_agent(CreateLocalAgent {
                                hosted_tools_consent: true,
                                expected_workspace_root: stale_root,
                                ..input.clone()
                            })
                            .await
                            .is_err(),
                        "consent must name the folder actually shown to the owner"
                    );
                }
                let agent = workspace
                    .create_agent(CreateLocalAgent {
                        hosted_tools_consent: true,
                        ..input
                    })
                    .await
                    .unwrap();
                assert_eq!(
                    agent.hosted_workspace.as_deref(),
                    Some(
                        tetonic_tools::Workspace::new(&root)
                            .unwrap()
                            .root()
                            .to_str()
                            .unwrap()
                    )
                );
                let id = uuid::Uuid::new_v4().to_string();
                workspace
                    .submit_for_agent(id.clone(), "Read the document".into(), agent.key.clone())
                    .await
                    .unwrap();
                let result = settled(&workspace, &id).await;
                let calls = transport.calls.lock().unwrap().clone();
                let all = serde_json::to_string(&calls).unwrap();
                assert!(!all.contains("OUTSIDE MUST NOT LEAK"));
                assert!(!all.contains("AKIAIOSFODNN7EXAMPLE"));
                assert!(!root.join("forbidden.txt").exists());
                let names: Vec<_> = calls[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|tool| tool["name"].as_str().unwrap())
                    .collect();
                assert!(names.contains(&"read_file"));
                assert!(!names.contains(&"write_file"));
                assert!(!names.contains(&"recall"));
                if contents.starts_with("The orchard") {
                    assert_eq!(result.state, "completed");
                    assert_eq!(calls.len(), 2);
                    assert!(
                        all.contains(contents),
                        "real file contents must reach the second model call"
                    );
                    assert!(result
                        .messages
                        .iter()
                        .any(|message| message.content.contains(contents)));
                } else if path == "../outside.txt" || tool == "write_file" {
                    assert_eq!(calls.len(), 2, "denial must reach the model as a result");
                    let output = calls[1]["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|item| item["type"] == "function_call_output")
                        .unwrap();
                    let output = output["output"].as_str().unwrap();
                    assert!(!output.is_empty());
                    assert_ne!(output, "ok");
                } else {
                    assert_eq!(calls.len(), 1, "secret must block further inference");
                    assert_eq!(result.state, "failed");
                }
                // Changing the host folder cannot silently transfer this approval.
                workspace.host.settings.workspace_root = Some(dir.path().to_path_buf());
                assert!(workspace.hosted_binding(&agent).await.is_err());
            })
            .await;
    }
}
