use super::*;

pub(super) fn reply(scenario: u8, count: usize) -> (&'static str, Value) {
    match (scenario, count) {
        (6, 1) => (DISPATCH, json!({"assignment_keys":["check","compare"]})),
        (6, 2) => ("finish", json!({"summary":"PREMATURE_GROUP_RESULT"})),
        (6, 3 | 4) | (5 | 7, 1) => (DISPATCH, json!({"assignment_keys":["compare","check"]})),
        _ => ("finish", json!({"summary":"COMBINED_GROUP_RESULT"})),
    }
}

async fn terminal(workspace: &LocalWorkspace, source: &str) -> PlanExecutionView {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let view = workspace.execution_view(source).await.unwrap().unwrap();
            if matches!(view.state.as_str(), "completed" | "failed" | "canceled") {
                return view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn grouped_dispatch_preserves_dependencies_receipts_and_idempotent_retries() {
    for scenario in [5, 6] {
        let dir = tempfile::tempdir().unwrap();
        let (url, calls, server) = scripted_server(scenario).await;
        tokio::task::LocalSet::new()
            .run_until(async {
                let workspace = Rc::new(
                    LocalWorkspace::open(dir.path().join("plan.db"), "qwen3.5:latest".into(), url)
                        .await
                        .unwrap(),
                );
                let source = seed(&workspace).await;
                workspace
                    .start_plan(
                        &source,
                        StartPlan {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            revision: 1,
                        },
                    )
                    .await
                    .unwrap();
                let outcome = terminal(&workspace, &source).await;
                assert_eq!(
                    outcome.state,
                    "completed",
                    "{}",
                    serde_json::to_string(&outcome).unwrap()
                );
                let root = outcome.root.unwrap();
                assert_eq!(contribution_text(&root).unwrap(), "COMBINED_GROUP_RESULT");
                assert!(outcome.assignments[1].input.contains("COMPARE_RESULT"));
                assert!(outcome
                    .assignments
                    .iter()
                    .all(|task| task.run_id == root.run_id
                        && task.state == "completed"
                        && !task.input.contains("PRIVATE_CANARY")));
                let usage = settled_usage(&workspace).await;
                assert!(usage
                    .iter()
                    .all(|row| row.held_tokens == 0 && !row.over_limit));
                let calls = calls.lock().unwrap();
                let roots: Vec<_> = calls
                    .iter()
                    .filter(|r| r["tools"].to_string().contains(DISPATCH))
                    .collect();
                assert_eq!(roots.len(), if scenario == 5 { 2 } else { 5 });
                let dispatch_schema = roots[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|tool| tool["function"]["name"] == DISPATCH)
                    .unwrap();
                let parameters = &dispatch_schema["function"]["parameters"];
                assert_eq!(parameters["required"], json!(["assignment_keys"]));
                assert!(parameters["properties"].get("assignment_key").is_none());
                assert_eq!(parameters["additionalProperties"], false);
                assert_eq!(
                    calls.len() - roots.len(),
                    2,
                    "retries must not duplicate children"
                );
                assert!(
                    roots.last().unwrap()["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|message| {
                            let text = message["content"].as_str().unwrap_or("");
                            message["role"] == "tool"
                                && text.contains("COMPARE_RESULT")
                                && text.contains("CHECK_RESULT")
                                && text.contains("\"outstanding_assignments\":[]")
                        }),
                    "both source contributions must reach synthesis in one receipt"
                );
                if scenario == 6 {
                    assert!(
                        roots[1]["messages"].to_string().contains("error:"),
                        "invalid dependency order is reported before any worker runs"
                    );
                    assert!(
                        roots[2]["messages"]
                            .to_string()
                            .contains("still needs contributions"),
                        "grouping must not bypass completion guard"
                    );
                }
            })
            .await;
        server.abort();
    }
}

#[tokio::test]
async fn stopping_a_group_cancels_the_child_and_never_starts_the_next_key() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(7).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("plan.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed(&workspace).await;
            let started = workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 1,
                    },
                )
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                while calls.lock().unwrap().len() < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            workspace
                .cancel(&started.receipt.root_work_id)
                .await
                .unwrap();
            let outcome = terminal(&workspace, &source).await;
            assert_eq!(outcome.state, "canceled");
            assert_eq!(outcome.assignments[1].state, "not_started");
            assert_eq!(calls.lock().unwrap().len(), 2);
        })
        .await;
    server.abort();
}
