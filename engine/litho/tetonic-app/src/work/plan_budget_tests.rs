//! Exercise budget failure through the real registered execution + DAG scheduler.
use super::*;

pub(super) fn reply(child: bool, request: &Value) -> (&'static str, Value) {
    if child {
        let assignment = width::assignment(request).unwrap();
        return (
            "finish",
            json!({"summary":format!("BUDGET_RESULT_{}", assignment.key)}),
        );
    }
    if !request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool")
    {
        return (
            DISPATCH,
            json!({"assignment_keys":["compare", "check", "follow", "blocked"]}),
        );
    }
    let messages = request["messages"].to_string();
    assert!(messages.contains("BUDGET_RESULT_check"));
    assert!(messages.contains("BUDGET_RESULT_follow"));
    assert!(
        !messages.contains("BUDGET_RESULT_compare"),
        "over-budget output must not become accepted evidence"
    );
    (
        "ask_human",
        json!({"question":"One assignment exhausted its allowance. Shall we continue that unfinished branch?", "why":"Independent contributions are saved; its dependent work has not run."}),
    )
}

#[tokio::test]
async fn exhausted_assignment_preserves_parallel_results_and_coordinator_can_report_partial_work() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(19).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("budget.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed_options(&workspace, false, true).await;
            let mut content = workspace.plan_view(&source).await.unwrap().plans[0]
                .content
                .clone()
                .unwrap();
            let mut follow = content.assignments[1].clone();
            follow.key = "follow".into();
            follow.title = "Use the independent result".into();
            follow.depends_on = vec!["check".into()];
            let mut blocked = content.assignments[0].clone();
            blocked.key = "blocked".into();
            blocked.title = "Depends on the exhausted assignment".into();
            blocked.depends_on = vec!["compare".into()];
            content.assignments.extend([follow, blocked]);
            content.token_budget = 20_096;
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
            let view = recovery::wait(&workspace, &source, "waiting_human").await;
            let state = |key: &str| {
                let pin = view
                    .receipt
                    .assignments
                    .iter()
                    .find(|p| p.assignment_key == key)
                    .unwrap();
                view.assignments
                    .iter()
                    .find(|a| a.id == pin.work_id)
                    .unwrap()
                    .state
                    .as_str()
            };
            assert_eq!(state("compare"), "failed");
            assert_eq!(state("check"), "completed");
            assert_eq!(state("follow"), "completed");
            assert_eq!(state("blocked"), "not_started");
            {
                let calls = calls.lock().unwrap();
                assert_eq!(
                    calls
                        .iter()
                        .filter(|r| width::assignment(r).is_some())
                        .count(),
                    3
                );
                for call in calls.iter() {
                    assert!(call["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|m| m["role"] == "system"
                            && m["content"]
                                .as_str()
                                .unwrap_or_default()
                                .contains("Current execution allowance:")));
                    assert!(
                        call["options"]["num_predict"].as_u64().unwrap() < 8000,
                        "output must leave room for input"
                    );
                }
            }
            let usage = workspace
                .services
                .local
                .resources()
                .team_work_usage(&workspace.services.host.credential, ORG.into(), TEAM.into())
                .await
                .unwrap();
            assert_eq!(usage.iter().filter(|r| r.over_limit).count(), 1);
            assert_eq!(
                usage
                    .iter()
                    .map(|r| r.input_tokens + r.output_tokens)
                    .sum::<i64>(),
                4121
            );
            workspace
                .cancel(&started.receipt.root_work_id)
                .await
                .unwrap();
            assert_eq!(
                groups::terminal(&workspace, &source).await.state,
                "canceled"
            );
        })
        .await;
    server.abort();
}
