use super::*;

#[tokio::test]
async fn missing_approvals_are_visible_and_can_be_removed_independently() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    for path in [&first, &second] {
        std::fs::create_dir(path).unwrap();
    }
    let workspace = LocalWorkspace::open(
        dir.path().join("state.db"),
        "offline".into(),
        "http://127.0.0.1:1".into(),
    )
    .await
    .unwrap();
    let mut paths = vec![];
    for path in [&first, &second] {
        paths.push(
            workspace
                .inspect_folder(FolderInspection {
                    path: path.to_string_lossy().into_owned(),
                })
                .await
                .unwrap(),
        );
    }
    workspace
        .save_folders(SaveWorkspaceFolders {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: 0,
            paths: paths.clone(),
        })
        .await
        .unwrap();
    for path in [&first, &second] {
        std::fs::remove_dir(path).unwrap();
    }
    assert_eq!(
        workspace
            .folder_catalog()
            .await
            .unwrap()
            .unavailable_paths
            .len(),
        2
    );
    assert!(workspace.services.available_folders().unwrap().is_empty());
    let saved = workspace
        .save_folders(SaveWorkspaceFolders {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: 1,
            paths: vec![paths[1].clone()],
        })
        .await
        .unwrap();
    assert_eq!(saved.paths, vec![paths[1].clone()]);
    assert_eq!(saved.unavailable_paths, saved.paths);
    assert!(workspace.services.resolve_folder(Some(&paths[1])).is_err());
    assert!(
        workspace
            .save_folders(SaveWorkspaceFolders {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 2,
                paths,
            })
            .await
            .is_err(),
        "an unavailable removed folder cannot be granted again"
    );
}

#[tokio::test]
async fn owner_adds_folder_without_restart_and_existing_agents_keep_their_scope() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("research");
    for p in [
        &first,
        &second,
        &dir.path().join(".ssh"),
        &dir.path().join("logs"),
    ] {
        std::fs::create_dir(p).unwrap();
    }
    std::fs::write(second.join("notes.txt"), "NEW_FOLDER_EVIDENCE").unwrap();
    let database = dir.path().join("control.db");
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_tool(
        true,
        "read_file",
        serde_json::json!({"path":"notes.txt"}),
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let config = crate::host::HostConfiguration {
                storage: crate::host::StorageConfiguration {
                    artifact_directory: Some(dir.path().join("logs")),
                    ..Default::default()
                },
                ..Default::default()
            };
            let workspace = LocalWorkspace::open_with_configuration(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                None,
                config.clone(),
            )
            .await
            .unwrap();
            let existing = workspace.snapshot().await.unwrap().agents;
            let inspect = |p: &Path| FolderInspection {
                path: p.to_string_lossy().into_owned(),
            };
            let path = workspace.inspect_folder(inspect(&second)).await.unwrap();
            #[cfg(windows)]
            for denied in [
                r"\\server\share\folder",
                r"\\?\UNC\server\share\folder",
                r"\\.\C:\folder",
            ] {
                assert!(workspace
                    .inspect_folder(FolderInspection {
                        path: denied.into()
                    })
                    .await
                    .is_err());
            }
            assert!(!workspace
                .agent_catalog()
                .await
                .unwrap()
                .tools
                .contains(&"read_file".into()));
            assert!(!workspace
                .agent_catalog()
                .await
                .unwrap()
                .workspace_folders
                .contains(&path));
            for p in [
                dir.path().to_path_buf(),
                dir.path().join(".ssh"),
                dir.path().join("logs"),
                dir.path().join("missing"),
                PathBuf::from("relative"),
            ] {
                assert!(
                    workspace.inspect_folder(inspect(&p)).await.is_err(),
                    "rejected {p:?}"
                );
            }
            let request = SaveWorkspaceFolders {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 0,
                paths: vec![path.clone()],
            };
            let saved = workspace.save_folders(request.clone()).await.unwrap();
            assert_eq!(saved.revision, 1);
            assert_eq!(
                workspace
                    .save_folders(request.clone())
                    .await
                    .unwrap()
                    .revision,
                1
            );
            let mut stale = request.clone();
            stale.request_id = uuid::Uuid::new_v4().to_string();
            assert!(workspace.save_folders(stale).await.is_err());
            for agent in workspace.snapshot().await.unwrap().agents {
                let before = existing.iter().find(|a| a.key == agent.key).unwrap();
                assert_eq!(before.definition_digest, agent.definition_digest);
                assert_eq!(before.workspace_root, agent.workspace_root);
            }
            let mut input = CreateLocalAgent {
                tool_pack_selection: None,
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "Reader".into(),
                purpose: "Read notes".into(),
                provider: "anthropic".into(),
                model: "claude-test".into(),
                harness: "general".into(),
                hosted_consent: true,
                hosted_tools_consent: true,
                expected_workspace_root: Some(
                    first.canonicalize().unwrap().to_string_lossy().into_owned(),
                ),
                workspace_root: Some(path.clone()),
                max_steps: 8,
                max_seconds: 60,
                max_tokens: 2048,
                tools: Some(vec!["read_file".into()]),
            };
            assert!(
                workspace
                    .services
                    .agent_configuration(input.clone())
                    .is_err(),
                "registration never supplies hosted disclosure consent"
            );
            input.provider = "ollama".into();
            input.model = "qwen3.5:latest".into();
            input.hosted_consent = false;
            input.hosted_tools_consent = false;
            input.expected_workspace_root = None;
            let agent = workspace.create_agent(input).await.unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            workspace
                .submit_for_agent(id.clone(), "Read notes.txt".into(), agent.key.clone())
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                while workspace.task(&id).await.unwrap().state != "completed" {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert!(serde_json::to_string(&*calls.lock().unwrap())
                .unwrap()
                .contains("NEW_FOLDER_EVIDENCE"));
            drop(workspace);
            let reopened = LocalWorkspace::open_with_configuration(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                Some(first.clone()),
                config.clone(),
            )
            .await
            .unwrap();
            assert_eq!(
                reopened.folder_catalog().await.unwrap().paths,
                vec![path.clone()]
            );
            assert_eq!(
                reopened.services.resolve_folder(Some(&path)).unwrap(),
                Some(second.canonicalize().unwrap())
            );
            reopened
                .save_folders(SaveWorkspaceFolders {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 1,
                    paths: vec![],
                })
                .await
                .unwrap();
            assert!(reopened.services.resolve_folder(Some(&path)).is_err());
            let retried = reopened.save_folders(request.clone()).await.unwrap();
            assert_eq!(retried.revision, 2);
            assert!(
                retried.paths.is_empty(),
                "retry must not restore removed access"
            );
            reopened
                .save_folders(SaveWorkspaceFolders {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 2,
                    paths: vec![path.clone()],
                })
                .await
                .unwrap();
            drop(reopened);
            let mut locked_config = config;
            locked_config.lock_agent_folders = true;
            let locked = LocalWorkspace::open_with_configuration(
                database,
                "qwen3.5:latest".into(),
                url,
                Some(first),
                locked_config,
            )
            .await
            .unwrap();
            assert!(!locked.folder_catalog().await.unwrap().can_manage);
            assert!(locked.inspect_folder(inspect(&second)).await.is_err());
            assert!(locked.save_folders(request).await.is_err());
            assert!(locked.services.resolve_folder(Some(&path)).is_err());
        })
        .await;
    server.abort();
}

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
                tool_pack_selection: None,
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
