use super::*;

struct FileTransport {
    provider: &'static str,
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
                    json!({"path":self.path,"content":"managed hosted write"})
                } else {
                    json!({"path":self.path})
                },
                "real-provider-id",
            )
        } else {
            let output =
                super::parity::output(calls.last().unwrap(), self.provider, "real-provider-id");
            ("finish", json!({"summary":output}), "finish-id")
        };
        Ok(super::parity::completion(self.provider, name, args, id))
    }
}

#[tokio::test]
async fn hosted_reads_use_selected_tools_exact_folder_and_actual_provider_result_ids() {
    for provider in ["openai", "anthropic", "google"] {
        for (tool, path, contents) in [
            ("read_file", "notes.txt", "The orchard has seventeen trees."),
            ("read_file", "../outside.txt", "OUTSIDE MUST NOT LEAK"),
            ("write_file", "forbidden.txt", ""),
            ("write_file", "result.txt", ""),
            ("read_file", "notes.txt", "AKIAIOSFODNN7EXAMPLE"),
        ] {
            tokio::task::LocalSet::new()
                .run_until(async {
                    let writable = path == "result.txt";
                    let dir = tempfile::tempdir().unwrap();
                    let root = dir.path().join("approved");
                    std::fs::create_dir(&root).unwrap();
                    std::fs::write(root.join("notes.txt"), contents).unwrap();
                    std::fs::write(dir.path().join("outside.txt"), "OUTSIDE MUST NOT LEAK")
                        .unwrap();
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
                    let transport = Arc::new(FileTransport {
                        calls: Mutex::default(),
                        provider,
                        tool: tool.into(),
                        path: path.into(),
                    });
                    workspace.services.hosted_transport = Some(transport.clone());
                    workspace
                        .save_provider_key(SaveProviderKey {
                            provider: provider.into(),
                            api_key: "disposable-provider-key".into(),
                        })
                        .await
                        .unwrap();
                    let input = CreateLocalAgent {
                        provider: provider.into(),
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
                        tools: Some(vec![
                            if writable { "write_file" } else { "read_file" }.into()
                        ]),
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
                    let mut legacy = agent.clone();
                    legacy.tool_disclosure = None;
                    assert_eq!(
                        workspace.services.hosted_binding(&legacy).await.is_ok(),
                        provider == "openai" && !writable,
                        "legacy consent must not gain new provider or write authority"
                    );
                    let mut moved = agent.clone();
                    moved.tool_disclosure.as_mut().unwrap().endpoint =
                        "https://elsewhere.invalid".into();
                    assert!(workspace.services.hosted_binding(&moved).await.is_err());
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
                    let names = super::parity::names(&calls[0], provider);
                    assert_eq!(names.contains(&"read_file"), !writable);
                    assert_eq!(names.contains(&"write_file"), writable);
                    assert!(!names.contains(&"recall"));
                    if writable {
                        assert_eq!(result.state, "completed", "{provider}: {:?}", result.error);
                        assert_eq!(
                            std::fs::read_to_string(root.join("result.txt")).unwrap(),
                            "managed hosted write"
                        );
                        assert_eq!(calls.len(), 2);
                    } else if contents.starts_with("The orchard") {
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
                        let output = super::parity::output(&calls[1], provider, "real-provider-id");
                        assert!(!output.is_empty());
                        assert_ne!(output, "ok");
                    } else {
                        assert_eq!(calls.len(), 1, "secret must block further inference");
                        assert_eq!(result.state, "failed");
                    }
                    // Changing the host folder cannot silently transfer this approval.
                    workspace.services.host.settings.workspace_root =
                        Some(dir.path().to_path_buf());
                    assert!(workspace.services.hosted_binding(&agent).await.is_err());
                })
                .await;
        }
    }
}
