use super::*;

#[tokio::test]
async fn engine_progress_rejects_foreign_scope_and_misattributed_completed_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.db");
    let (url, _, server) = scripted_server(5).await;
    tokio::task::LocalSet::new().run_until(async {
        let workspace = Rc::new(LocalWorkspace::open(path.clone(), "qwen3.5:latest".into(), url).await.unwrap());
        let source = seed(&workspace).await;
        workspace.start_plan(&source, StartPlan { request_id: uuid::Uuid::new_v4().to_string(), revision: 1, ..Default::default() }).await.unwrap();
        let result = terminal(&workspace, &source).await;
        assert_eq!(result.state, "completed");
        let reader = workspace.controller_reader();
        let progress = reader.read(&result.receipt).await.unwrap();
        assert!(progress.assignments.iter().all(|a| a.state == tetonic_memory::AssignmentState::Completed));
        // Exercise the real finalization boundary: generated output is not yet
            // accepted task evidence, and must neither fail nor release dependents.
            let _ = settled_usage(&workspace).await;
            let run_id = progress.run_id.clone().unwrap();
            let work = result.receipt.assignments[0].work_id.clone();
            let original = workspace.services.keys.store.write(move |db| {
                let original = db.load_run_snapshot(&run_id).unwrap().unwrap();
                let work = db.get_team_work_item(ORG, TEAM, &work).unwrap().unwrap();
                let mut finalizing = original.clone();
                finalizing.state = tetonic_domain::RunState::Active;
                let attempt = &finalizing.attempts[&tetonic_domain::AttemptId::new(work.attempt_id.unwrap())];
                let task = finalizing.tasks.get_mut(&attempt.task_id).unwrap();
                task.state = tetonic_domain::TaskState::Running;
                task.accepted_artifact = None;
                task.completed_version = None;
                db.persist_run_projection(&finalizing).unwrap();
                original
            }).await.unwrap();
            let finalizing = reader.read(&result.receipt).await.unwrap();
            assert_eq!(finalizing.assignments[0].state, tetonic_memory::AssignmentState::Executing);
            assert!(finalizing.assignments[0].holds_capacity, "finalization still owns admission capacity");
            workspace.services.keys.store.write(move |db| db.persist_run_projection(&original)).await.unwrap().unwrap();
            let mut outsider = reader.clone();
        outsider.actor = "unrelated-owner".into();
        assert!(outsider.read(&result.receipt).await.is_err());
        let mut wrong_team = reader.clone();
        wrong_team.team = "another-team".into();
        assert!(wrong_team.read(&result.receipt).await.is_err());
        // Same agent/revision, same successful run: the other assignment's
        // completed attempt still cannot stand in for this work item's evidence.
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("UPDATE team_work_items SET attempt_id=(SELECT attempt_id FROM team_work_items WHERE work_id=?2) WHERE work_id=?1",
            rusqlite::params![result.receipt.assignments[0].work_id,result.receipt.assignments[1].work_id]).unwrap();
        let changed = reader.read(&result.receipt).await.unwrap();
        assert_eq!(changed.assignments[0].state, tetonic_memory::AssignmentState::NeedsAttention);
        assert_eq!(changed.assignments[1].state, tetonic_memory::AssignmentState::Completed);
    }).await;
    server.abort();
}

#[tokio::test]
async fn parent_stop_cancels_both_parallel_workers() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(9).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("plan.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed_options(&workspace, false, true).await;
            let start = workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 1,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while calls.lock().unwrap().len() < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("both children must start before stop");
            workspace.cancel(&start.receipt.root_work_id).await.unwrap();
            let result = terminal(&workspace, &source).await;
            assert_eq!(result.state, "canceled");
            assert!(
                result.assignments.iter().all(|a| a.state == "canceled"),
                "{}",
                serde_json::to_string(&result).unwrap()
            );
            assert_eq!(calls.lock().unwrap().len(), 3);
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn independent_agents_reach_inference_together_and_keep_their_identities() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(8).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("plan.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed_roster_options(&workspace, false, true, true).await;
            let before = workspace.services.agents().await.unwrap();
            workspace
                .start_plan(
                    &source,
                    StartPlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 1,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let result = terminal(&workspace, &source).await;
            assert_eq!(
                result.state,
                "completed",
                "{}",
                serde_json::to_string(&result).unwrap()
            );
            assert_eq!(calls.lock().unwrap().len(), 4);
            assert_ne!(
                result.receipt.assignments[0].agent_key,
                result.receipt.assignments[1].agent_key
            );
            for pin in &result.receipt.assignments {
                let original = before.iter().find(|a| a.key == pin.agent_key).unwrap();
                let current = workspace
                    .services
                    .agents()
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|a| a.key == pin.agent_key)
                    .unwrap();
                assert_eq!(original.id, current.id);
                assert_eq!(original.definition_digest, pin.definition_digest);
                assert_eq!(
                    serde_json::to_value(original).unwrap(),
                    serde_json::to_value(current).unwrap()
                );
            }
            let usage = settled_usage(&workspace).await;
            assert!(usage.iter().all(|u| u.held_tokens == 0 && !u.over_limit));
        })
        .await;
    server.abort();
}

pub(super) fn reply(scenario: u8, count: usize) -> (&'static str, Value) {
    match (scenario, count) {
        (6, 1) => (DISPATCH, json!({"assignment_keys":["check","compare"]})),
        (6, 2) => (DISPATCH, json!({"assignment_keys":["compare","check"]})),
        (5 | 7 | 8 | 9, 1) => (DISPATCH, json!({"assignment_keys":["compare","check"]})),
        _ => ("finish", json!({"summary":"COMBINED_GROUP_RESULT"})),
    }
}

pub(super) async fn terminal(workspace: &LocalWorkspace, source: &str) -> PlanExecutionView {
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
                            ..Default::default()
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
                assert_eq!(roots.len(), if scenario == 5 { 2 } else { 3 });
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
                        ..Default::default()
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
