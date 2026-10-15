//! Deterministic scheduler qualification, not evidence of model planning quality.
use super::*;

pub(super) fn assignment(request: &Value) -> Option<tetonic_memory::PlanAssignment> {
    request["messages"].as_array()?.iter().find_map(|m| {
        let input = m["content"].as_str()?;
        let (_, tail) = input.split_once("Your agreed assignment:\n")?;
        serde_json::from_str(tail.split("\n\nShared brief").next()?).ok()
    })
}

pub(super) fn reply(child: bool, request: &Value) -> (&'static str, Value) {
    if child {
        let a = assignment(request).unwrap();
        let input = request["messages"].to_string();
        for dependency in &a.depends_on {
            assert!(
                input.contains(&format!("WIDTH_RESULT_{dependency}")),
                "missing committed dependency {dependency}"
            );
        }
        (
            "finish",
            json!({"summary":format!("WIDTH_RESULT_{}", a.key)}),
        )
    } else if !request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool")
    {
        (
            DISPATCH,
            json!({"assignment_keys":(0..20).map(|i| format!("task-{i}")).collect::<Vec<_>>()}),
        )
    } else {
        let input = request["messages"].to_string();
        for i in 0..20 {
            assert!(
                input.contains(&format!("WIDTH_RESULT_task-{i}")),
                "missing result {i}: {input}"
            );
        }
        (
            "finish",
            json!({"summary":"WIDTH_COMBINED_RESULT: all twenty contributions received"}),
        )
    }
}

pub(super) async fn wait_for_peers(request: &Value, calls: &Arc<Mutex<Vec<Value>>>, width: usize) {
    let a = assignment(request).unwrap();
    let index = a
        .key
        .strip_prefix("task-")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    if index >= 6 {
        return;
    }
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let ready = {
                let calls = calls.lock().unwrap();
                let keys: HashSet<_> = calls.iter().filter_map(assignment).map(|a| a.key).collect();
                // No first response returns until all six workers reach inference.
                // The slow branch additionally waits for another branch's successor:
                // a wave/barrier scheduler would deadlock here.
                (0..6)
                    .filter(|i| keys.contains(&format!("task-{i}")))
                    .count()
                    >= width
                    && (index != 0 || keys.contains("task-11"))
            };
            if ready {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("six workers and independent successor must progress concurrently");
}

#[tokio::test]
async fn six_agents_complete_twenty_tasks_with_branch_joins_and_reused_capacity() {
    run_width(6, 17).await;
}

#[tokio::test]
async fn twenty_tasks_queue_within_three_worker_slots_without_stalling_branches() {
    run_width(3, 18).await;
}

async fn run_width(width: u32, scenario: u8) {
    let directory = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(scenario).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut config = crate::host::HostConfiguration::default();
            config.workspace_execution.max_tokens = 120_000;
            config.workspace_execution.coordination_max_tokens = 20_000;
            let workspace = Rc::new(
                LocalWorkspace::open_with_configuration(
                    directory.path().join("width.db"),
                    "qwen3.5:latest".into(),
                    url,
                    None,
                    config,
                )
                .await
                .unwrap(),
            );
            let source = seed(&workspace).await;
            let resources = workspace.services.local.resources();
            let secret = &workspace.services.host.credential;
            let limits = resources
                .get_team_execution_limits(secret, ORG.into(), TEAM.into())
                .await
                .unwrap();
            // A coordinator occupies one slot. Configure the existing admission
            // policy explicitly instead of disabling it or changing product defaults.
            resources
                .set_team_execution_limits(
                    secret,
                    ORG.into(),
                    TEAM.into(),
                    limits.revision,
                    width + 1,
                )
                .await
                .unwrap();
            let base = workspace.plan_view(&source).await.unwrap();
            let mut content = base.plans[0].content.clone().unwrap();
            let mut agents: Vec<_> = content
                .assignments
                .iter()
                .map(|a| a.agent_key.clone())
                .collect();
            for n in 2..6 {
                let agent = workspace
                    .create_agent(CreateLocalAgent {
                        tool_pack_selection: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: format!("Worker {n}"),
                        purpose: "Complete the assigned contribution.".into(),
                        provider: "ollama".into(),
                        model: "qwen3.5:latest".into(),
                        harness: "general".into(),
                        max_steps: 4,
                        max_seconds: 120,
                        max_tokens: 4096,
                        tools: Some(vec![]),
                        workspace_root: None,
                        expected_workspace_root: None,
                        hosted_consent: false,
                        hosted_tools_consent: false,
                    })
                    .await
                    .unwrap();
                agents.push(agent.key);
            }
            let before = workspace.services.agents().await.unwrap();
            let template = content.assignments[0].clone();
            content.assignments = (0..20)
                .map(|i| {
                    let mut a = template.clone();
                    a.key = format!("task-{i}");
                    a.title = format!("Contribution {i}");
                    a.agent_key = agents[i % 6].clone();
                    a.instructions =
                        format!("Complete contribution {i} using its prerequisite evidence.");
                    a.deliverable = format!("Reviewed contribution {i}");
                    a.token_budget = 4000;
                    a.depends_on = match i {
                        0..=5 => vec![],
                        6..=17 => vec![format!("task-{}", i - 6)],
                        18 => (12..15).map(|n| format!("task-{n}")).collect(),
                        _ => (15..19).map(|n| format!("task-{n}")).collect(),
                    };
                    a
                })
                .collect();
            content.token_budget = 100_000;
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
            let reviewed = workspace.plan_view(&source).await.unwrap();
            assert_eq!(reviewed.max_assignments, PlanContent::MAX_ASSIGNMENTS);
            assert!(reviewed.execution_available, "{:?}", reviewed.readiness);
            let request = uuid::Uuid::new_v4().to_string();
            let started = workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: request.clone(),
                        revision: 2,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let complete = tokio::time::timeout(std::time::Duration::from_secs(45), async {
                loop {
                    let view = workspace.execution_view(&source).await.unwrap().unwrap();
                    if matches!(view.state.as_str(), "completed" | "failed" | "canceled") {
                        break view;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                }
            })
            .await
            .expect("wide plan did not settle");
            if complete.state != "completed" {
                let run = complete.root.as_ref().unwrap().run_id.clone().unwrap();
                let reasons = workspace
                    .services
                    .keys
                    .store
                    .read(move |db| {
                        db.load_run_snapshot(&run)
                            .unwrap()
                            .unwrap()
                            .attempts
                            .values()
                            .filter_map(|a| a.failure_reason.clone())
                            .collect::<Vec<_>>()
                    })
                    .await
                    .unwrap();
                panic!("Wide execution failures: {reasons:?}");
            }
            assert_eq!(
                complete.state,
                "completed",
                "{}",
                serde_json::to_string(&complete).unwrap()
            );
            let root = complete.root.as_ref().unwrap();
            assert!(contribution_text(root)
                .unwrap()
                .contains("WIDTH_COMBINED_RESULT"));
            assert_eq!(complete.assignments.len(), 20);
            assert_eq!(
                complete
                    .assignments
                    .iter()
                    .map(|t| &t.agent_key)
                    .collect::<HashSet<_>>()
                    .len(),
                6
            );
            for task in &complete.assignments {
                assert_eq!(task.state, "completed");
                assert_eq!(task.run_id, root.run_id);
                let pin = task.plan.as_ref().unwrap();
                assert_eq!(
                    contribution_text(task).unwrap(),
                    format!("WIDTH_RESULT_{}", pin.assignment_key.as_ref().unwrap())
                );
            }
            let retry = workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: request,
                        revision: 2,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(retry.receipt, started.receipt);
            assert_eq!(
                calls.lock().unwrap().len(),
                22,
                "one dispatch, twenty workers, one synthesis; retry creates no work"
            );
            let usage = settled_usage(&workspace).await;
            assert_eq!(
                usage
                    .iter()
                    .map(|u| u.input_tokens + u.output_tokens)
                    .sum::<i64>(),
                660
            );
            assert!(usage.iter().all(|u| u.held_tokens == 0 && !u.over_limit));
            let after = workspace.services.agents().await.unwrap();
            for key in agents {
                let saved = before.iter().find(|a| a.key == key).unwrap();
                let current = after.iter().find(|a| a.key == key).unwrap();
                assert_eq!(
                    serde_json::to_value(saved).unwrap(),
                    serde_json::to_value(current).unwrap()
                );
            }
        })
        .await;
    server.abort();
}
