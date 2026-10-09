use super::*;

#[tokio::test]
async fn folder_edits_are_pinned_durable_and_checked_against_host_scope() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    let forbidden = dir.path().join(".ssh");
    let artifacts = dir.path().join("runtime-output");
    for path in [&first, &second, &forbidden, &artifacts] {
        std::fs::create_dir(path).unwrap();
    }
    std::fs::write(first.join("notes.txt"), "WRONG_FOLDER_CANARY").unwrap();
    std::fs::write(second.join("notes.txt"), "selected folder evidence").unwrap();
    let canonical = |p: &Path| p.canonicalize().unwrap().to_string_lossy().into_owned();
    let database = dir.path().join("control.db");
    let config = crate::host::HostConfiguration {
        agent_folders: vec![
            second.clone(),
            forbidden.clone(),
            dir.path().into(),
            artifacts.clone(),
        ],
        storage: crate::host::StorageConfiguration {
            artifact_directory: Some(artifacts.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_tool(
        true,
        "read_file",
        serde_json::json!({"path":"notes.txt"}),
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_configuration(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                Some(first.clone()),
                config.clone(),
            )
            .await
            .unwrap();
            let catalog = workspace.agent_catalog().await.unwrap();
            assert_eq!(
                catalog.workspace_folders.len(),
                2,
                "control and credential directories are not offered"
            );
            assert_eq!((catalog.default_steps, catalog.default_seconds), (16, 300));
            let mut input = CreateLocalAgent {
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "Reader".into(),
                purpose: "Read the supplied notes".into(),
                provider: "ollama".into(),
                model: "qwen3.5:latest".into(),
                harness: "general".into(),
                hosted_consent: false,
                hosted_tools_consent: false,
                expected_workspace_root: None,
                workspace_root: Some(canonical(&first)),
                max_steps: 16,
                max_seconds: 300,
                max_tokens: 2048,
                tools: Some(vec!["read_file".into()]),
            };
            let original = workspace.create_agent(input.clone()).await.unwrap();
            let before = workspace
                .services
                .agent_execution_settings(&original)
                .await
                .unwrap();
            for denied in [
                canonical(&forbidden),
                canonical(&artifacts),
                canonical(dir.path()),
                first.join("../second").to_string_lossy().into_owned(),
            ] {
                input.workspace_root = Some(denied);
                assert!(workspace
                    .services
                    .agent_configuration(input.clone())
                    .is_err());
            }
            input.request_id = uuid::Uuid::new_v4().to_string();
            input.workspace_root = Some(canonical(&second));
            let mut hosted = input.clone();
            hosted.provider = "anthropic".into();
            hosted.hosted_consent = true;
            hosted.hosted_tools_consent = true;
            hosted.expected_workspace_root = Some(canonical(&first));
            assert!(
                workspace
                    .services
                    .agent_configuration(hosted.clone())
                    .is_err(),
                "folder access is not consent to disclose that folder"
            );
            hosted.expected_workspace_root = Some(canonical(&second));
            let disclosure = workspace.services.agent_configuration(hosted).unwrap();
            assert_eq!(
                disclosure["preferences"]["tool_disclosure"]["workspace"],
                canonical(&second)
            );
            let update = UpdateLocalAgent {
                agent_key: original.key.clone(),
                expected_definition_digest: original.definition_digest.clone(),
                configuration: input.clone(),
            };
            let changed = workspace.update_agent(update.clone()).await.unwrap();
            assert_eq!(changed.id, original.id);
            assert_eq!(before.workspace_root, Some(first.canonicalize().unwrap()));
            assert_eq!(
                workspace
                    .services
                    .agent_execution_settings(&original)
                    .await
                    .unwrap()
                    .workspace_root,
                before.workspace_root
            );
            let mut stale = update;
            stale.configuration.request_id = uuid::Uuid::new_v4().to_string();
            assert!(workspace.update_agent(stale).await.is_err());
            let id = uuid::Uuid::new_v4().to_string();
            workspace
                .submit_for_agent(
                    id.clone(),
                    "Read notes.txt with the selected file tool".into(),
                    changed.key.clone(),
                )
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                while workspace.task(&id).await.unwrap().state != "completed" {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            let captured = calls.lock().unwrap().clone();
            assert!(captured.last().unwrap()["messages"]
                .to_string()
                .contains("selected folder evidence"));
            assert!(!serde_json::to_string(&captured)
                .unwrap()
                .contains("WRONG_FOLDER_CANARY"));
            drop(workspace);
            let reopened = LocalWorkspace::open_with_configuration(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                Some(first.clone()),
                config,
            )
            .await
            .unwrap();
            let saved = reopened
                .services
                .agents()
                .await
                .unwrap()
                .into_iter()
                .find(|a| a.key == changed.key)
                .unwrap();
            assert_eq!(saved.definition_digest, changed.definition_digest);
            assert_eq!(saved.workspace_root, Some(canonical(&second)));
            assert_eq!((saved.max_steps, saved.max_seconds), (16, 300));
            drop(reopened);
            // Removing an operator-approved folder does not substitute the default.
            let narrowed = LocalWorkspace::open_with_workspace(
                database,
                "qwen3.5:latest".into(),
                url,
                Some(first),
            )
            .await
            .unwrap();
            assert!(narrowed
                .services
                .agent_execution_settings(&saved)
                .await
                .is_err());
            assert_eq!(calls.lock().unwrap().len(), captured.len());
        })
        .await;
    server.abort();
}
