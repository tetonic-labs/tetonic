use super::*;

#[test]
fn local_model_failure_is_actionable_without_exposing_provider_payloads() {
    let message = task_failure_message(Some(
        "provider: requested model residency unavailable; PRIVATE_PAYLOAD",
    ));
    assert!(message.contains("local inference server"));
    assert!(message.contains("Completed contributions are saved"));
    assert!(!message.contains("PRIVATE_PAYLOAD"));
}

#[tokio::test]
async fn work_usage_counts_real_managed_calls_and_preserves_allowances_across_retry_and_restart() {
    for report in [Some((10, 20)), None, Some((40, 20))] {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("usage.db");
        let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_usage(
            true,
            "finish",
            serde_json::json!({"summary":"Budgeted answer"}),
            false,
            report,
        )
        .await;
        tokio::task::LocalSet::new()
            .run_until(async {
                let workspace =
                    LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url.clone())
                        .await
                        .unwrap();
                workspace
                    .set_budget_settings(BudgetSettingsRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 0,
                        token_limit: Some(50),
                    })
                    .await
                    .unwrap();
                let id = uuid::Uuid::new_v4().to_string();
                workspace
                    .submit(id.clone(), "Answer briefly.".into())
                    .await
                    .unwrap();
                let expected = if report == Some((10, 20)) {
                    "completed"
                } else {
                    "failed"
                };
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    loop {
                        let task = workspace.task(&id).await.unwrap();
                        let usage = workspace.snapshot().await.unwrap().usage;
                        if task.state == expected
                            && usage.len() == 1
                            && (expected != "completed" || usage[0].released_tokens == 20)
                        {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await
                .expect("execution must settle or explicitly fail");
                workspace
                    .set_budget_settings(BudgetSettingsRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 1,
                        token_limit: Some(100),
                    })
                    .await
                    .unwrap();
                let replay = workspace
                    .submit(id.clone(), "Answer briefly.".into())
                    .await
                    .unwrap();
                assert_eq!(replay.state, expected);
                assert_eq!(calls.lock().unwrap().len(), 1, "replay must not bill again");
                assert_eq!(calls.lock().unwrap()[0]["options"]["num_predict"], 50);
                drop(workspace);
                let reopened = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
                let snapshot = reopened.snapshot().await.unwrap();
                assert_eq!(snapshot.budget_setting.token_limit, Some(100));
                let usage = &snapshot.usage[0];
                assert_eq!(
                    usage.budget.as_ref().unwrap().token_limit,
                    50,
                    "new default must not enlarge old work"
                );
                assert_eq!(usage.calls, 1);
                match report {
                    Some((10, 20)) => {
                        assert_eq!(
                            (
                                usage.input_tokens,
                                usage.output_tokens,
                                usage.held_tokens,
                                usage.released_tokens
                            ),
                            (10, 20, 0, 20)
                        );
                        assert_eq!(usage.budget.as_ref().unwrap().available_tokens, 20);
                    }
                    None => {
                        assert_eq!(
                            (
                                usage.unknown_calls,
                                usage.held_tokens,
                                usage.released_tokens
                            ),
                            (1, 50, 0)
                        );
                        assert_eq!(usage.budget.as_ref().unwrap().available_tokens, 0);
                    }
                    _ => {
                        assert!(usage.over_limit);
                        assert_eq!(usage.input_tokens + usage.output_tokens, 60);
                        assert_eq!(usage.released_tokens, 0);
                    }
                }
            })
            .await;
        server.abort();
    }
}

#[cfg(test)]
#[tokio::test]
async fn tool_grants_are_explicit_and_unsupported_selections_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let workspace_dir = tempfile::tempdir().unwrap();
    let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({"summary":"Done"}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("grants.db"),
                "qwen3.5:latest".into(),
                url,
                Some(workspace_dir.path().to_path_buf()),
            )
            .await
            .unwrap();
            let catalog = workspace.agent_catalog().await.unwrap();
            assert!(catalog.tools.contains(&"read_file".to_string()));
            assert!(!catalog.tools.contains(&"run_shell".to_string()));
            assert_eq!(catalog.runtime_profiles.len(), 3);
            for profile in &catalog.runtime_profiles {
                assert_eq!(profile.harness, "general");
                if profile.provider == "ollama" {
                    assert_eq!(profile.tools, catalog.tools);
                } else if profile.provider == "openai" {
                    assert!(profile.tools.contains(&"read_file".into()));
                    assert!(!profile.tools.contains(&"write_file".into()));
                    assert!(profile.requires_tool_consent);
                } else {
                    assert!(profile.tools.is_empty());
                    assert!(profile.tool_restriction.is_some());
                }
            }
            let input = CreateLocalAgent {
                provider: "ollama".into(),
                hosted_consent: false,
                hosted_tools_consent: false,
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "No tools".into(),
                purpose: "Answer from the supplied prompt".into(),
                model: "qwen3.5:latest".into(),
                harness: "general".into(),
                max_steps: 2,
                max_seconds: 60,
                max_tokens: 1024,
                tools: None,
            };
            for tools in [None, Some(vec![])] {
                let agent = workspace
                    .create_agent(CreateLocalAgent {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        tools,
                        ..input.clone()
                    })
                    .await
                    .unwrap();
                assert!(agent.tools.is_empty());
                let stored = workspace.registered_agent(&agent.key).await.unwrap();
                let definition: serde_json::Value =
                    serde_json::from_str(&stored.definition_json).unwrap();
                assert_eq!(
                    definition["configuration"]["requested_tools"],
                    serde_json::json!([])
                );
            }
            for tools in [
                vec!["run_shell".into()],
                vec!["read_file".into(), "unknown_tool".into()],
            ] {
                assert!(workspace
                    .create_agent(CreateLocalAgent {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        tools: Some(tools),
                        ..input.clone()
                    })
                    .await
                    .is_err());
            }
            assert!(workspace
                .create_agent(CreateLocalAgent {
                    provider: "openai".into(),
                    hosted_consent: true,
                    hosted_tools_consent: false,
                    tools: Some(vec!["read_file".into()]),
                    ..input.clone()
                })
                .await
                .is_err());
            let reader = workspace
                .create_agent(CreateLocalAgent {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    tools: Some(vec!["read_file".into(), "read_file".into()]),
                    ..input
                })
                .await
                .unwrap();
            assert_eq!(reader.tools, vec!["read_file"]);
            let mut stored = workspace.registered_agent(&reader.key).await.unwrap();
            let original: serde_json::Value =
                serde_json::from_str(&stored.definition_json).unwrap();
            for (pointer, invalid) in [
                ("/configuration/preferences/model", serde_json::json!(123)),
                (
                    "/configuration/requested_tools",
                    serde_json::json!("read_file"),
                ),
                ("/harness", serde_json::json!("not-installed")),
                ("/schema_version", serde_json::json!(99)),
            ] {
                let mut definition = original.clone();
                *definition.pointer_mut(pointer).unwrap() = invalid;
                stored.definition_json = serde_json::to_string(&definition).unwrap();
                assert!(
                    workspace
                        .agent_profile(reader.key.clone(), &stored)
                        .is_err(),
                    "invalid {pointer} must not silently switch execution profiles"
                );
            }
        })
        .await;
    server.abort();
}

#[cfg(test)]
#[cfg(test)]
#[tokio::test]
async fn long_requests_survive_reopen_and_changed_suffixes_cannot_reuse_ids() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("input.db");
    let (url, calls, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({"summary":"Read your full request."}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url.clone())
                    .await
                    .unwrap();
            let original = format!(
                "{}\nThe final instruction is copper-lantern.",
                "Long context. ".repeat(25)
            );
            let id = uuid::Uuid::new_v4().to_string();
            let task = workspace
                .submit(id.clone(), original.clone())
                .await
                .unwrap();
            assert_eq!(task.input, original);
            assert!(workspace
                .submit(
                    id.clone(),
                    original.replace("copper-lantern", "blue-compass")
                )
                .await
                .is_err());
            tokio::time::timeout(std::time::Duration::from_secs(20), async {
                while workspace.task(&id).await.unwrap().state != "completed" {
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                }
            })
            .await
            .unwrap();
            drop(workspace);
            let reopened = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            assert_eq!(reopened.task(&id).await.unwrap().input, original);
            assert_eq!(
                reopened
                    .submit(id.clone(), original.clone())
                    .await
                    .unwrap()
                    .state,
                "completed"
            );
            assert_eq!(calls.lock().unwrap().len(), 1);
            let context = reopened
                .conversation_input(
                    &uuid::Uuid::new_v4().to_string(),
                    AGENT,
                    Some(&id),
                    "Explain the last instruction.",
                )
                .await
                .unwrap();
            assert!(context.contains("copper-lantern"));
            assert!(context.contains(&"Long context. ".repeat(25)));
        })
        .await;
    server.abort();
}

#[cfg(test)]
#[cfg(test)]
#[tokio::test]
async fn local_agents_persist_validate_and_execute_the_selected_definition() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("agents.db");
    let (url, requests, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({"summary":"Selected agent answered."}),
        false,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url.clone())
                    .await
                    .unwrap();
            let input = CreateLocalAgent {
                provider: "ollama".into(),
                hosted_consent: false,
                hosted_tools_consent: false,
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "Analyst".into(),
                purpose: "Use the amber compass method. Call finish with your answer.".into(),
                model: "qwen3.5:latest".into(),
                harness: "general".into(),
                max_steps: 2,
                max_seconds: 60,
                max_tokens: 1024,
                tools: None,
            };
            let catalog = workspace.agent_catalog().await.unwrap();
            assert_eq!(catalog.models, vec!["qwen3.5:latest"]);
            for invalid in [
                CreateLocalAgent {
                    max_steps: 5,
                    ..input.clone()
                },
                CreateLocalAgent {
                    max_seconds: 121,
                    ..input.clone()
                },
                CreateLocalAgent {
                    max_tokens: 4097,
                    ..input.clone()
                },
                CreateLocalAgent {
                    harness: "coding".into(),
                    ..input.clone()
                },
                CreateLocalAgent {
                    model: "not-installed".into(),
                    ..input.clone()
                },
            ] {
                assert!(workspace.create_agent(invalid).await.is_err());
            }
            assert_eq!(workspace.snapshot().await.unwrap().agents.len(), 2);
            let agent = workspace.create_agent(input.clone()).await.unwrap();
            assert_eq!(
                workspace.create_agent(input.clone()).await.unwrap().id,
                agent.id
            );
            assert!(workspace
                .create_agent(CreateLocalAgent {
                    name: "Changed".into(),
                    ..input.clone()
                })
                .await
                .is_err());
            assert_eq!(workspace.snapshot().await.unwrap().agents.len(), 3);
            let stored = workspace.registered_agent(&agent.key).await.unwrap();
            let prepared = workspace
                .local
                .resources()
                .prepare_general_revision(
                    &workspace.host.credential,
                    ORG.into(),
                    agent.key.clone(),
                    stored.identity.bound_definition_digest,
                    "Check selection".into(),
                    workspace.host.settings.limits.clone(),
                )
                .await
                .unwrap();
            assert_eq!(prepared.invocation().max_steps, 2);
            assert!(prepared.requested_tools().is_empty());
            let id = uuid::Uuid::new_v4().to_string();
            let task = workspace
                .submit_for_agent(id.clone(), "Check selection".into(), agent.key.clone())
                .await
                .unwrap();
            assert_eq!(task.agent_name, "Analyst");
            assert!(workspace
                .submit(id.clone(), task.input.clone())
                .await
                .is_err());
            let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                loop {
                    let task = workspace.task(&id).await.unwrap();
                    if matches!(task.state.as_str(), "completed" | "failed") {
                        break task;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(result.state, "completed");
            assert_eq!(result.agent_key, agent.key);
            let calls = requests.lock().unwrap().clone();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0]["model"], input.model);
            assert!(calls[0]["messages"].to_string().contains("amber compass"));
            let reply_id = uuid::Uuid::new_v4().to_string();
            let reply = workspace
                .submit_in_conversation(
                    reply_id.clone(),
                    "Explain that answer.".into(),
                    agent.key.clone(),
                    Some(id.clone()),
                )
                .await
                .unwrap();
            assert_eq!(reply.parent_id.as_deref(), Some(id.as_str()));
            tokio::time::timeout(std::time::Duration::from_secs(20), async {
                loop {
                    let result = workspace.task(&reply_id).await.unwrap();
                    if matches!(result.state.as_str(), "completed" | "failed") {
                        assert_eq!(result.state, "completed");
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                }
            })
            .await
            .unwrap();
            let calls = requests.lock().unwrap().clone();
            assert_eq!(calls.len(), 2);
            let context = calls[1]["messages"].to_string();
            assert!(context.contains("Check selection"));
            assert!(context.contains("Selected agent answered."));
            assert!(context.contains("Explain that answer."));
            assert!(workspace
                .submit_in_conversation(
                    uuid::Uuid::new_v4().to_string(),
                    "Stale reply".into(),
                    agent.key.clone(),
                    Some(id.clone())
                )
                .await
                .is_err());
            assert!(workspace
                .submit_in_conversation(
                    uuid::Uuid::new_v4().to_string(),
                    "Wrong agent".into(),
                    AGENT.into(),
                    Some(reply_id.clone())
                )
                .await
                .is_err());
            assert_eq!(
                workspace
                    .submit_in_conversation(
                        reply_id.clone(),
                        reply.input.clone(),
                        agent.key.clone(),
                        Some(id.clone())
                    )
                    .await
                    .unwrap()
                    .run_id,
                reply.run_id
            );
            drop(workspace);
            let reopened = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let snapshot = reopened.snapshot().await.unwrap();
            assert_eq!(
                reopened.task(&reply_id).await.unwrap().parent_id,
                Some(id.clone())
            );
            assert_eq!(snapshot.agents.len(), 3);
            assert_eq!(snapshot.tasks[0].agent_key, agent.key);
            server.abort();
            // Offline retries still return the durable registration/result.
            assert_eq!(reopened.create_agent(input).await.unwrap().id, agent.id);
            assert_eq!(
                reopened
                    .submit_for_agent(id, task.input, agent.key)
                    .await
                    .unwrap()
                    .run_id,
                task.run_id
            );
        })
        .await;
}

#[cfg(test)]
#[cfg(test)]
#[tokio::test]
async fn local_ui_runs_once_and_restores_the_real_result() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("local.db");
    let (url, requests, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({"summary":"A real engine result."}),
        false,
    )
    .await;
    let id = uuid::Uuid::new_v4().to_string();
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url.clone())
                    .await
                    .unwrap();
            assert!(workspace.snapshot().await.unwrap().tasks.is_empty());
            let first = workspace
                .submit(id.clone(), "Help me compare two approaches.".into())
                .await
                .unwrap();
            let duplicate = workspace
                .submit(id.clone(), first.input.clone())
                .await
                .unwrap();
            assert_eq!(first.run_id, duplicate.run_id);
            assert!(workspace
                .submit(id.clone(), "Different input".into())
                .await
                .is_err());
            assert!(workspace
                .submit(
                    uuid::Uuid::new_v4().to_string(),
                    "x".repeat(INPUT_LIMIT + 1)
                )
                .await
                .is_err());
            assert!(workspace.task("unknown").await.is_err());
            let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                loop {
                    let task = workspace.task(&id).await.unwrap();
                    if matches!(task.state.as_str(), "completed" | "failed" | "canceled") {
                        break task;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await;
            assert!(
                result.is_ok(),
                "run did not finish: {}",
                serde_json::to_string(&workspace.task(&id).await.unwrap()).unwrap()
            );
            let result = result.unwrap();
            assert_eq!(
                result.state,
                "completed",
                "{}",
                serde_json::to_string(&result).unwrap()
            );
            assert!(
                result
                    .messages
                    .iter()
                    .any(|m| m.content.contains("A real engine result.")),
                "{}",
                serde_json::to_string(&result).unwrap()
            );
            assert_eq!(requests.lock().unwrap().len(), 1);
            drop(workspace);
            let restored = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let snapshot = restored.snapshot().await.unwrap();
            assert_eq!(snapshot.tasks.len(), 1);
            assert_eq!(snapshot.tasks[0].state, "completed");
            assert_eq!(snapshot.tasks[0].run_id, first.run_id);
            let replay = restored.submit(id.clone(), first.input).await.unwrap();
            assert_eq!(replay.run_id, first.run_id);
            assert_eq!(requests.lock().unwrap().len(), 1);
        })
        .await;
    server.abort();
}

#[cfg(test)]
#[cfg(test)]
#[tokio::test]
async fn local_ui_cancels_an_owned_managed_run() {
    let dir = tempfile::tempdir().unwrap();
    let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({}),
        true,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace =
                LocalWorkspace::open(dir.path().join("local.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            workspace
                .submit(id.clone(), "Wait for cancellation".into())
                .await
                .unwrap();
            workspace.cancel(&id).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                loop {
                    if workspace.task(&id).await.unwrap().state == "canceled" {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
    server.abort();
}

#[cfg(test)]
#[cfg(test)]
#[tokio::test]
async fn open_with_workspace_enables_canonical_tools() {
    let dir = tempfile::tempdir().unwrap();
    let workspace_dir = tempfile::tempdir().unwrap();
    let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({}),
        true,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("local.db"),
                "qwen3.5:latest".into(),
                url,
                Some(workspace_dir.path().to_path_buf()),
            )
            .await
            .unwrap();
            assert!(workspace.host.settings.workspace_root.is_some());
            assert!(workspace.host.settings.allowed_tools.contains("read_file"));
            assert!(workspace.host.settings.allowed_tools.contains("list_dir"));
            assert!(workspace.host.settings.allowed_tools.contains("grep"));
            assert!(workspace.host.settings.allowed_tools.contains("glob"));
            assert!(workspace.host.settings.allowed_tools.contains("edit_file"));
            assert!(workspace.host.settings.allowed_tools.contains("write_file"));

            let registered = workspace.registered_agent("Local assistant").await.unwrap();
            let def: serde_json::Value = serde_json::from_str(&registered.definition_json).unwrap();
            let req_tools: Vec<String> =
                serde_json::from_value(def["configuration"]["requested_tools"].clone()).unwrap();
            assert!(req_tools.contains(&"read_file".to_string()));
            assert!(req_tools.contains(&"list_dir".to_string()));
            assert!(req_tools.contains(&"edit_file".to_string()));
        })
        .await;
    server.abort();
}

#[cfg(test)]
#[cfg(test)]
#[tokio::test]
async fn sprint2_autonomy_and_review_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let workspace_dir = tempfile::tempdir().unwrap();
    let (url, _, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "finish",
        serde_json::json!({}),
        true,
    )
    .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("local.db"),
                "qwen3.5:latest".into(),
                url,
                Some(workspace_dir.path().to_path_buf()),
            )
            .await
            .unwrap();

            // 1. Tool scoping: create agent with scoped tools
            let agent_input = CreateLocalAgent {
                provider: "ollama".into(),
                hosted_consent: false,
                hosted_tools_consent: false,
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "Scoper".into(),
                purpose: "Scoped agent. Call finish.".into(),
                model: "qwen3.5:latest".into(),
                harness: "general".into(),
                max_steps: 2,
                max_seconds: 60,
                max_tokens: 1024,
                tools: Some(vec!["read_file".into(), "grep".into()]),
            };
            let agent = workspace.create_agent(agent_input).await.unwrap();
            assert_eq!(
                agent.tools,
                vec!["read_file".to_string(), "grep".to_string()]
            );

            // 2. Create work item with lead_id and agent_ids
            let work_id = uuid::Uuid::new_v4().to_string();
            let work = workspace
                .create_work_item(CreateWorkItemRequest {
                    id: work_id.clone(),
                    title: "Test multi-agent dispatch".into(),
                    goal_id: None,
                    agent_key: None,
                    lead_id: Some(agent.key.clone()),
                    agent_ids: Some(vec![agent.key.clone(), "Local assistant".into()]),
                })
                .await
                .unwrap();
            assert_eq!(work.lead_id.as_deref(), Some(agent.key.as_str()));
            assert_eq!(
                work.agent_ids.as_deref(),
                Some(&[agent.key.clone(), "Local assistant".into()][..])
            );

            // 3. Status transition to review and done
            workspace
                .set_local_work_data(
                    &work.id,
                    Some(vec!["Finding report: all tasks complete".into()]),
                    Some("review".into()),
                    None,
                    None,
                )
                .await
                .unwrap();

            let items = workspace.work_items().await.unwrap();
            let found = items.iter().find(|i| i.id == work.id).unwrap();
            assert_eq!(found.status, "review");
            assert_eq!(found.notes, vec!["Finding report: all tasks complete"]);

            // Transition to done upon user approval
            workspace
                .set_local_work_data(&work.id, None, Some("done".into()), None, None)
                .await
                .unwrap();
            let items2 = workspace.work_items().await.unwrap();
            let found2 = items2.iter().find(|i| i.id == work.id).unwrap();
            assert_eq!(found2.status, "done");
        })
        .await;
    server.abort();
}
