//! Operator limits must reach admission without rewriting saved agent budgets.
use super::*;
use crate::host::HostConfiguration;

fn configuration() -> HostConfiguration {
    HostConfiguration::from_json(include_bytes!(
        "../../../../../docs/architecture/examples/workspace-execution.json"
    ))
    .unwrap()
}

#[tokio::test]
async fn configured_limits_reach_catalog_and_admission_without_rewriting_agents() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("workspace.db");
    let (url, calls, server) = server(false).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_configuration(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                None,
                configuration(),
            )
            .await
            .unwrap();
            let catalog = workspace.agent_catalog().await.unwrap();
            assert_eq!(
                (catalog.max_steps, catalog.max_seconds, catalog.max_tokens),
                (32, 900, 32000)
            );
            assert!(catalog.tools.is_empty(), "raising ceilings grants no tools");
            let input = CreateLocalAgent {
                workspace_root: None,
                request_id: uuid::Uuid::new_v4().to_string(),
                name: "Longer work".into(),
                purpose: "Review the provided evidence.".into(),
                provider: "ollama".into(),
                model: "qwen3.5:latest".into(),
                harness: "general".into(),
                hosted_consent: false,
                hosted_tools_consent: false,
                expected_workspace_root: None,
                max_steps: 20,
                max_seconds: 600,
                max_tokens: 20000,
                tools: Some(vec![]),
            };
            for invalid in [
                CreateLocalAgent {
                    workspace_root: None,
                    max_steps: 33,
                    ..input.clone()
                },
                CreateLocalAgent {
                    workspace_root: None,
                    max_seconds: 901,
                    ..input.clone()
                },
                CreateLocalAgent {
                    workspace_root: None,
                    max_tokens: 32001,
                    ..input.clone()
                },
            ] {
                assert!(workspace.create_agent(invalid).await.is_err());
            }
            let agent = workspace.create_agent(input).await.unwrap();
            let settings = workspace
                .services
                .agent_execution_settings(&agent)
                .await
                .unwrap();
            assert_eq!(
                (
                    settings.max_elapsed_seconds,
                    settings.reported_token_ceiling
                ),
                (600, Some(20000))
            );
            let prepared = workspace
                .services
                .local
                .resources()
                .prepare_general_revision(
                    &workspace.services.host.credential,
                    ORG.into(),
                    agent.key.clone(),
                    agent.definition_digest.clone(),
                    "Review evidence".into(),
                    settings.limits,
                )
                .await
                .unwrap();
            assert_eq!(prepared.invocation().max_steps, 20);
            drop(workspace);

            let mut raised = configuration();
            raised.workspace_execution.max_steps = Some(64);
            raised.workspace_execution.max_seconds = 1800;
            raised.workspace_execution.max_tokens = 64000;
            let workspace = LocalWorkspace::open_with_configuration(
                database.clone(),
                "qwen3.5:latest".into(),
                url.clone(),
                None,
                raised,
            )
            .await
            .unwrap();
            let stored = workspace
                .services
                .registered_agent(&agent.key)
                .await
                .unwrap();
            let saved = workspace
                .services
                .agent_profile(agent.key.clone(), &stored)
                .unwrap();
            assert_eq!(saved.definition_digest, agent.definition_digest);
            assert_eq!(
                (saved.max_steps, saved.max_seconds, saved.max_tokens),
                (20, 600, 20000)
            );
            drop(workspace);

            let workspace = LocalWorkspace::open_with_configuration(
                database,
                "qwen3.5:latest".into(),
                url,
                None,
                HostConfiguration::default(),
            )
            .await
            .unwrap();
            assert!(workspace
                .submit_for_agent(
                    uuid::Uuid::new_v4().to_string(),
                    "Do not execute beyond host ceilings".into(),
                    agent.key.clone(),
                )
                .await
                .is_err());
            let stored = workspace
                .services
                .registered_agent(&agent.key)
                .await
                .unwrap();
            let saved = workspace
                .services
                .agent_profile(agent.key.clone(), &stored)
                .unwrap();
            assert_eq!(saved.definition_digest, agent.definition_digest);
            assert_eq!(
                (saved.max_steps, saved.max_seconds, saved.max_tokens),
                (20, 600, 20000)
            );
            assert!(
                calls.lock().unwrap().is_empty(),
                "rejected admission must not call inference"
            );
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn configured_coordination_budget_reaches_planning_and_real_managed_team_execution() {
    let directory = tempfile::tempdir().unwrap();
    let (url, _, server) = server(false).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open_with_configuration(
                    directory.path().join("plan.db"),
                    "qwen3.5:latest".into(),
                    url,
                    None,
                    configuration(),
                )
                .await
                .unwrap(),
            );
            let source = seed(&workspace).await;
            let view = workspace.plan_view(&source).await.unwrap();
            let mut content = view.plans[0].content.clone().unwrap();
            content.token_budget = 10000; // 2,000 workers + 8,000 coordination, above the old 4,096 cap.
            assert!(workspace
                .execution_readiness(&content)
                .await
                .unwrap()
                .is_empty());
            let mut excessive = content.clone();
            excessive.token_budget = 12001;
            assert!(workspace
                .execution_readiness(&excessive)
                .await
                .unwrap()
                .iter()
                .any(|r| r.contains("256–10000")));
            workspace
                .set_budget_settings(BudgetSettingsRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 0,
                    token_limit: Some(9000),
                })
                .await
                .unwrap();
            assert!(workspace
                .execution_readiness(&content)
                .await
                .unwrap()
                .iter()
                .any(|r| r.contains("workspace allowance")));
            workspace
                .set_budget_settings(BudgetSettingsRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 1,
                    token_limit: None,
                })
                .await
                .unwrap();
            let context = workspace
                .director_input(&source, "Plan the work".into())
                .await
                .unwrap();
            assert!(context.contains("\"maximum_tokens\":10000"));
            assert!(context.contains("\"maximum_steps\":24"));
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
            let started = workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 2,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let receipt = started.receipt;
            let coordinator = workspace.pinned_coordinator(&receipt).await.unwrap();
            assert_eq!(
                (
                    coordinator.max_steps,
                    coordinator.max_seconds,
                    coordinator.max_tokens
                ),
                (24, 2700, 8000)
            );
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    let view = workspace.execution_view(&source).await.unwrap().unwrap();
                    if view.state == "completed" {
                        assert_eq!(view.assignments.len(), 2);
                        assert!(view.assignments.iter().all(|a| a.state == "completed"));
                        break;
                    }
                    assert!(
                        !matches!(
                            view.state.as_str(),
                            "failed" | "canceled" | "recovery_required"
                        ),
                        "{}",
                        serde_json::to_string(&view).unwrap()
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                }
            })
            .await
            .unwrap();
            assert!(settled_usage(&workspace)
                .await
                .iter()
                .all(|u| u.held_tokens == 0));
        })
        .await;
    server.abort();
}
