//! Hosted coordination exercises the real managed dispatcher and tool hosts.
use super::*;
use crate::local_workspace::{
    plan_execution::CoordinationModel, AnswerPlanQuestion, PlanCommand, SaveWorkBrief, StartPlan,
};

struct TeamTransport {
    tool: String,
    calls: Mutex<Vec<(String, Value)>>,
    workers: tokio::sync::Barrier,
}

#[async_trait::async_trait]
impl HostedTransport for TeamTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let provider = if body.get("contents").is_some() {
            "google"
        } else if body.get("input").is_some() {
            "openai"
        } else {
            "anthropic"
        };
        let names = parity::names(&body, provider);
        let role = if names.contains(&"dispatch_assignment") {
            "coordinator"
        } else if names.contains(&"read_file") {
            "alpha"
        } else {
            "bravo"
        };
        assert!(!body.to_string().contains("PRIVATE_CANARY_MUST_NOT_LEAK"));
        let count = {
            let mut calls = self.calls.lock().unwrap();
            calls.push((role.into(), body.clone()));
            calls.iter().filter(|(r, _)| r == role).count()
        };
        if role != "coordinator" && count == 1 {
            // Both workers must reach inference before either can continue.
            tokio::time::timeout(std::time::Duration::from_secs(8), self.workers.wait())
                .await
                .expect("independent agents were serialized");
        }
        let (tool, args, id) = match (role, count) {
            ("coordinator", 1) => {
                assert!(names
                    .iter()
                    .all(|n| ["finish", "dispatch_assignment", "ask_human"].contains(n)));
                (
                    "dispatch_assignment",
                    json!({"assignment_keys":["alpha","bravo"]}),
                    "dispatch",
                )
            }
            ("coordinator", 2) => {
                let result = parity::output(&body, provider, "dispatch");
                for evidence in [
                    "TEAM_FILE_CANARY",
                    "Tuesday 10:00",
                    "ANSWER_CANARY Beginners",
                ] {
                    assert!(result.contains(evidence), "missing {evidence}: {result}");
                }
                (
                    "finish",
                    json!({"summary":format!("Combined fixture contributions: {result}")}),
                    "done",
                )
            }
            ("alpha", 1) => ("read_file", json!({"path":"brief.txt"}), "file"),
            ("alpha", 2) => {
                assert!(parity::output(&body, provider, "file").contains("TEAM_FILE_CANARY"));
                (
                    "ask_human",
                    json!({"question":"Which audience?","why":"This shapes the recommendation.","options":["Beginners","Experienced"]}),
                    "question",
                )
            }
            ("alpha", 3) => {
                assert!(
                    parity::output(&body, provider, "question").contains("ANSWER_CANARY Beginners")
                );
                (
                    "finish",
                    json!({"summary":"TEAM_FILE_CANARY for ANSWER_CANARY Beginners"}),
                    "done",
                )
            }
            ("bravo", 1) => (
                self.tool.as_str(),
                json!({"query":"Tuesday availability"}),
                "calendar",
            ),
            ("bravo", 2) => {
                let result = parity::output(&body, provider, "calendar");
                assert!(result.contains("Tuesday 10:00"));
                ("finish", json!({"summary":result}), "done")
            }
            _ => panic!("unexpected {role} call {count}"),
        };
        Ok(parity::completion(provider, tool, args, id))
    }
}

async fn set_guide(workspace: &LocalWorkspace, provider: &str, model: &str) -> LocalAgent {
    let guide = workspace.guide_for_coordination().await.unwrap();
    workspace
        .update_agent(UpdateLocalAgent {
            agent_key: guide.key,
            expected_definition_digest: guide.definition_digest,
            configuration: CreateLocalAgent {
                request_id: uuid::Uuid::new_v4().to_string(),
                name: guide.name,
                purpose: guide.purpose,
                provider: provider.into(),
                model: model.into(),
                harness: "general".into(),
                hosted_consent: true,
                hosted_tools_consent: false,
                expected_workspace_root: None,
                tools: Some(vec![]),
                max_steps: 6,
                max_seconds: 30,
                max_tokens: 4096,
            },
        })
        .await
        .unwrap()
}

async fn seed(workspace: &LocalWorkspace, agents: &[LocalAgent]) -> String {
    let source = uuid::Uuid::new_v4().to_string();
    let resources = workspace.services.local.resources();
    resources
        .create_team_work_item_for_purpose(
            &workspace.services.host.credential,
            crate::resources::CreateTeamWorkItem {
                org: ORG.into(),
                team: TEAM.into(),
                work_id: source.clone(),
                title: "Plan a workshop".into(),
                request_id: format!("{source}@{}", GUIDE),
                goal_id: None,
            },
            Some("PRIVATE_CANARY_MUST_NOT_LEAK".into()),
            WorkPurpose::Explore,
        )
        .await
        .unwrap();
    workspace
        .save_work_brief(
            &source,
            SaveWorkBrief {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 0,
                body: "Review the supplied workshop brief and calendar availability.".into(),
            },
        )
        .await
        .unwrap();
    let assignments: Vec<_> = agents
        .iter()
        .zip(["alpha", "bravo"])
        .map(|(agent, key)| {
            json!({
                "key":key,"title":key,"instructions":"Use only the granted tools for the workshop.",
                "agent_key":agent.key,"depends_on":[],"tools":agent.tools,
                "deliverable":"A source-backed contribution","token_budget":1500
            })
        })
        .collect();
    resources.mutate_huddle_plan(&workspace.services.host.credential, ORG.into(), TEAM.into(), source.clone(),
        crate::resources::PlanMutation::Save {
            request: uuid::Uuid::new_v4().to_string(), expected: 0, brief_revision: 1,
            generation_id: uuid::Uuid::new_v4().to_string(), generation_input: "Fixture proposal".into(),
            content: Some(serde_json::from_value(json!({"title":"Workshop preparation",
                "summary":"Read the brief and check availability independently.","token_budget":6000,
                "open_questions":[],"assignments":assignments})).unwrap()),
        },
    ).await.unwrap();
    workspace
        .update_plan(
            &source,
            PlanCommand::Agree {
                request_id: uuid::Uuid::new_v4().to_string(),
                revision: 1,
            },
        )
        .await
        .unwrap();
    source
}

#[tokio::test]
async fn hosted_coordinators_dispatch_parallel_tool_using_teams_without_ollama() {
    for provider in ["openai", "anthropic", "google"] {
        tokio::task::LocalSet::new()
            .run_until(async {
                let dir = tempfile::tempdir().unwrap();
                let database = dir.path().join("team.db");
                let root = dir.path().join("approved");
                std::fs::create_dir(&root).unwrap();
                std::fs::write(root.join("brief.txt"), "TEAM_FILE_CANARY").unwrap();
                let fixture = crate::mcp::tests::fixture::Fixture::new().await;
                let vault = Arc::new(Vault::default());
                let mut workspace = LocalWorkspace::open_with_workspace(
                    database.clone(),
                    "offline".into(),
                    "http://127.0.0.1:1".into(),
                    Some(root.clone()),
                )
                .await
                .unwrap()
                .with_mcp_config(&fixture.config)
                .unwrap();
                workspace.services.keys = Arc::new(ProviderKeys {
                    store: workspace.services.keys.store.clone(),
                    vault: vault.clone(),
                });
                for p in ["openai", "anthropic", "google"] {
                    workspace
                        .save_provider_key(SaveProviderKey {
                            provider: p.into(),
                            api_key: "fixture-only-key".into(),
                        })
                        .await
                        .unwrap();
                }
                workspace.discover_mcp("calendar").await.unwrap();
                let tool = workspace.mcp_connections()[0]
                    .tools
                    .iter()
                    .find(|t| t.name == "search")
                    .unwrap()
                    .id
                    .clone();
                let transport = Arc::new(TeamTransport {
                    tool: tool.clone(),
                    calls: Mutex::default(),
                    workers: tokio::sync::Barrier::new(2),
                });
                workspace.services.hosted_transport = Some(transport.clone());
                let guide = set_guide(&workspace, provider, "coordination-fixture").await;
                let model = CoordinationModel::from(&guide);
                let mut agents = vec![];
                for (p, name, tools) in [
                    ("openai", "Brief researcher", vec!["read_file".into()]),
                    ("anthropic", "Calendar researcher", vec![tool.clone()]),
                ] {
                    agents.push(
                        workspace
                            .create_agent(CreateLocalAgent {
                                request_id: uuid::Uuid::new_v4().to_string(),
                                name: name.into(),
                                purpose: name.into(),
                                provider: p.into(),
                                model: format!("{p}-worker-fixture"),
                                harness: "general".into(),
                                hosted_consent: true,
                                hosted_tools_consent: true,
                                expected_workspace_root: Some(
                                    tetonic_tools::Workspace::new(&root)
                                        .unwrap()
                                        .root()
                                        .to_string_lossy()
                                        .into_owned(),
                                ),
                                tools: Some(tools),
                                max_steps: 4,
                                max_seconds: 30,
                                max_tokens: 1500,
                            })
                            .await
                            .unwrap(),
                    );
                }
                let workspace = std::rc::Rc::new(workspace);
                let source = seed(&workspace, &agents).await;
                let request_id = uuid::Uuid::new_v4().to_string();
                let request = || StartPlan {
                    request_id: request_id.clone(),
                    revision: 1,
                    coordinator: Some(model.clone()),
                    hosted_coordination_consent: true,
                    ..Default::default()
                };
                let view = workspace.plan_view(&source).await.unwrap();
                assert!(view.readiness.is_empty(), "{:?}", view.readiness);
                assert_eq!(view.coordinator, Some(model.clone()));
                let mut missing = request();
                missing.hosted_coordination_consent = false;
                assert!(workspace.start_plan(&source, missing).await.is_err());
                let mut stale = request();
                stale.coordinator.as_mut().unwrap().model = "another-model".into();
                assert!(workspace.start_plan(&source, stale).await.is_err());
                assert!(workspace
                    .execution_receipt(&source)
                    .await
                    .unwrap()
                    .is_none());
                assert!(transport.calls.lock().unwrap().is_empty());

                let started = workspace.start_plan(&source, request()).await.unwrap();
                let waiting = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    loop {
                        let view = workspace.execution_view(&source).await.unwrap().unwrap();
                        assert!(
                            !matches!(view.state.as_str(), "failed" | "completed" | "canceled"),
                            "{}",
                            serde_json::to_string(&view).unwrap()
                        );
                        if view.assignments.iter().any(|a| a.state == "waiting_human")
                            && view.assignments.iter().any(|a| a.state == "completed")
                        {
                            break view;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await
                .unwrap();
                set_guide(&workspace, provider, "next-plan-model").await;
                assert_eq!(
                    workspace.plan_view(&source).await.unwrap().coordinator,
                    Some(model.clone())
                );
                let question = waiting
                    .assignments
                    .iter()
                    .flat_map(|a| &a.human_questions)
                    .find(|q| q.answer.is_none())
                    .unwrap();
                workspace
                    .answer_plan_question(
                        &question.work_id,
                        AnswerPlanQuestion {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            question_id: question.id.clone(),
                            answer: "ANSWER_CANARY Beginners".into(),
                        },
                    )
                    .await
                    .unwrap();
                let root = settled(&workspace, &started.receipt.root_work_id).await;
                assert_eq!(
                    root.state,
                    "completed",
                    "{}",
                    serde_json::to_string(&root).unwrap()
                );
                let outcome = workspace.execution_view(&source).await.unwrap().unwrap();
                assert_eq!(outcome.state, "completed");
                assert!(outcome.assignments.iter().all(|a| a.run_id == root.run_id));
                for agent in &agents {
                    assert!(outcome
                        .receipt
                        .assignments
                        .iter()
                        .any(|a| a.agent_key == agent.key
                            && a.definition_digest == agent.definition_digest));
                }
                assert_eq!(
                    workspace
                        .start_plan(&source, request())
                        .await
                        .unwrap()
                        .receipt,
                    started.receipt
                );
                let mut changed = request();
                changed.coordinator.as_mut().unwrap().model = "next-plan-model".into();
                assert!(workspace.start_plan(&source, changed).await.is_err());
                {
                    let calls = transport.calls.lock().unwrap();
                    assert_eq!(calls.len(), 7);
                    for (role, body) in calls.iter() {
                        let wire = if body.get("contents").is_some() {
                            "google"
                        } else if body.get("input").is_some() {
                            "openai"
                        } else {
                            "anthropic"
                        };
                        let names = parity::names(body, wire);
                        if role == "coordinator" && wire != "google" {
                            assert_eq!(body["model"], "coordination-fixture");
                        }
                        if role != "coordinator" {
                            assert!(!names.contains(&"dispatch_assignment"));
                        }
                        assert!(!names.contains(&"write_file") && !names.contains(&"run_shell"));
                    }
                }
                assert_eq!(
                    fixture
                        .calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|r| r["method"] == "tools/call")
                        .count(),
                    1
                );

                // Missing worker credentials are actionable before another plan creates work.
                workspace
                    .remove_provider_key(RemoveProviderKey {
                        provider: "anthropic".into(),
                    })
                    .await
                    .unwrap();
                let blocked = seed(&workspace, &agents).await;
                let view = workspace.plan_view(&blocked).await.unwrap();
                assert!(view
                    .setup_issues
                    .iter()
                    .any(|i| i.agent_key == agents[1].key));
                assert!(!view.execution_available);
                assert!(workspace
                    .start_plan(
                        &blocked,
                        StartPlan {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            revision: 1,
                            coordinator: view.coordinator,
                            hosted_coordination_consent: true,
                            ..Default::default()
                        }
                    )
                    .await
                    .is_err());
                assert!(workspace
                    .execution_receipt(&blocked)
                    .await
                    .unwrap()
                    .is_none());
                // Historical execution reads do not depend on live keys or the current Guide.
                drop(workspace);
                let reopened = std::rc::Rc::new(
                    LocalWorkspace::open(
                        database,
                        "different-host-default".into(),
                        "http://127.0.0.1:1".into(),
                    )
                    .await
                    .unwrap(),
                );
                let saved = reopened.start_plan(&source, request()).await.unwrap();
                assert_eq!(saved.coordinator, Some(model));
                assert_eq!(saved.receipt, started.receipt);
                assert_eq!(transport.calls.lock().unwrap().len(), 7);
            })
            .await;
    }
}
