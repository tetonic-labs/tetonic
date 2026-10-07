use super::*;

pub(super) fn reply(scenario: u8, child: bool, request: &Value) -> (&'static str, Value) {
    let continuation = request["messages"].to_string().contains("Continuation of");
    let steps = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .count();
    if child {
        if scenario == 10
            && !continuation
            && request["messages"].to_string().contains("CHECK_CANARY")
        {
            return ("TEST_FAILURE", json!({}));
        }
        return (
            "finish",
            json!({"summary":if continuation {"CONTINUED_RESULT based on retained COMPARE_RESULT"} else {"COMPARE_RESULT evidence kept"}}),
        );
    }
    if continuation {
        if steps == 0 {
            return (
                DISPATCH,
                json!({"assignment_key":if scenario == 10 {"check"} else {"assemble-result"}}),
            );
        }
        return ("finish", json!({"summary":"RECOVERED_COMBINED_RESULT"}));
    }
    if scenario == 12 {
        return match steps {
            0 => (
                "ask_human",
                json!({"question":"Which audience?","why":"Confirm the intended audience"}),
            ),
            1 => (DISPATCH, json!({"assignment_key":"compare"})),
            _ => ("TEST_FAILURE", json!({})),
        };
    }
    match steps {
        0 => (DISPATCH, json!({"assignment_key":"compare"})),
        1 => (DISPATCH, json!({"assignment_key":"check"})),
        _ => ("TEST_FAILURE", json!({})),
    }
}

pub(super) async fn wait(
    workspace: &LocalWorkspace,
    source: &str,
    state: &str,
) -> PlanExecutionView {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let view = workspace.plan_view(source).await.unwrap();
            let execution = view.execution.unwrap();
            if execution.state == state
                && (matches!(state, "completed" | "waiting_human")
                    || view.recovery.is_some_and(|r| r.available))
            {
                return execution;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("plan must reach the expected terminal state")
}

async fn recorded_usage(workspace: &LocalWorkspace) -> Vec<tetonic_memory::WorkUsage> {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let rows = workspace
                .local
                .resources()
                .team_work_usage(&workspace.host.credential, ORG.into(), TEAM.into())
                .await
                .unwrap();
            // Provider failures can retain an unknown reservation. Recovery must
            // preserve it; it must never manufacture a zero-cost settlement.
            if !rows.is_empty() && rows.iter().all(|r| r.pending_calls == 0) {
                return rows;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn continuation_preserves_completed_evidence_and_usage_and_runs_only_unfinished_assignments()
{
    exercise(10).await;
}

#[tokio::test]
async fn continuation_after_synthesis_failure_proposes_only_assembling_completed_work() {
    exercise(11).await;
}

#[tokio::test]
async fn continuation_keeps_owner_answers_and_latest_assignment_directions() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(12).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("answers.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed_variant(&workspace, true).await;
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
            let waiting = wait(&workspace, &source, "waiting_human").await;
            for revision in 0..2 {
                workspace
                    .amend_plan_assignment(
                        &source,
                        AmendPlanAssignment {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            expected_revision: revision,
                            assignment_key: "wrap".into(),
                            instructions: format!("LATEST_DIRECTION_{revision}"),
                        },
                    )
                    .await
                    .unwrap();
            }
            let question = &waiting.root.as_ref().unwrap().human_questions[0];
            workspace
                .answer_plan_question(
                    &question.work_id,
                    AnswerPlanQuestion {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        question_id: question.id.clone(),
                        answer: "ANSWER_RETAINED Beginners".into(),
                    },
                )
                .await
                .unwrap();
            let failed = wait(&workspace, &source, "failed").await;
            let count = calls.lock().unwrap().len();
            let continuation = workspace
                .continue_plan(
                    &source,
                    ContinuePlan {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_root_work_id: failed.receipt.root_work_id,
                    },
                )
                .await
                .unwrap();
            let view = workspace
                .plan_view(&continuation.continuation_work_id)
                .await
                .unwrap();
            let assignments = &view.plans[0].content.as_ref().unwrap().assignments;
            assert_eq!(assignments.len(), 2);
            let wrap = assignments.iter().find(|a| a.key == "wrap").unwrap();
            assert_eq!(wrap.instructions, "LATEST_DIRECTION_1");
            assert_eq!(wrap.depends_on, vec!["check"]);
            let brief = workspace
                .work_briefs(&continuation.continuation_work_id)
                .await
                .unwrap();
            assert!(brief[0].body.contains("ANSWER_RETAINED Beginners"));
            assert!(brief[0].body.contains("Which audience?"));
            assert!(brief[0].body.contains("COMPARE_RESULT"));
            assert_eq!(calls.lock().unwrap().len(), count);
        })
        .await;
    server.abort();
}

async fn exercise(scenario: u8) {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("continuation.db");
    let folder = dir.path().join("files");
    std::fs::create_dir(&folder).unwrap();
    let (url, calls, server) = scripted_server(scenario).await;
    let (source, receipt, original, usage) = tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open_with_workspace(
                    database.clone(),
                    "qwen3.5:latest".into(),
                    url.clone(),
                    Some(folder.clone()),
                )
                .await
                .unwrap(),
            );
            let source = seed(&workspace).await;
            let worker = workspace.plan_view(&source).await.unwrap().plans[0]
                .content
                .as_ref()
                .unwrap()
                .assignments[1]
                .agent_key
                .clone();
            if scenario == 10 {
                set_tools(&workspace, &worker, vec!["write_file".into()]).await;
            }
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
            let original = wait(&workspace, &source, "failed").await;
            assert_eq!(original.assignments[0].state, "completed");
            let mut missing = workspace.execution_view(&source).await.unwrap().unwrap();
            missing.assignments[0].messages.clear();
            assert!(!workspace.recovery_view(&missing).await.unwrap().unwrap().available);
            // Later agent edits cannot hide the failed attempt's prior authority.
            if scenario == 10 {
                set_tools(&workspace, &worker, vec![]).await;
            }
            let usage = recorded_usage(&workspace).await;
            // Optional browser proof uses an isolated, honest failed-provider
            // fixture. Never seed demonstrations into the owner's live database.
            if scenario == 10 {
                if let Ok(directory) = std::env::var("TETONIC_RECOVERY_PROOF_DIR") {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    let target = directory.join("workspace.db");
                    assert!(!target.exists(), "use a fresh recovery proof directory");
                    let backup = tetonic_memory::pre_migration_backup(&database).unwrap();
                    std::fs::copy(backup, target).unwrap();
                    copy_fixture_files(&folder, &directory.join("files"));
                    std::fs::write(directory.join("fixture.json"), serde_json::to_vec_pretty(&json!({"kind":"controlled provider failure, not real model output","source":source,"execution":original})).unwrap()).unwrap();
                }
            }
            let count = calls.lock().unwrap().len();
            let prepare = |root: String| ContinuePlan {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_root_work_id: root,
            };
            assert!(workspace
                .continue_plan(&source, prepare(uuid::Uuid::new_v4().to_string()))
                .await
                .is_err());
            let invalid = original.receipt.clone();
            workspace
                .keys
                .store
                .write(move |db| {
                    let receipt = tetonic_memory::PlanContinuation {
                        source_work_id: invalid.source_work_id.clone(),
                        root_work_id: invalid.root_work_id.clone(),
                        continuation_work_id: "invalid-continuation".into(),
                        request_id: "invalid-continuation".into(),
                        retained: vec![],
                        review_before_repeat: vec![],
                        created_by: OWNER.into(),
                    };
                    let mut bad = invalid.content.clone();
                    bad.assignments[0].depends_on = vec!["missing".into()];
                    let command = |actor| tetonic_memory::CreatePlanContinuation {
                        actor,
                        org: ORG,
                        team: TEAM,
                        receipt: &receipt,
                        guide_key: shaping::GUIDE,
                        brief: "valid brief",
                        content: &bad,
                    };
                    assert!(db.create_plan_continuation(command("outsider")).is_err());
                    assert!(db.create_plan_continuation(command(OWNER)).is_err());
                    assert!(
                        db.get_team_work_item(ORG, TEAM, &receipt.continuation_work_id)
                            .unwrap()
                            .is_none(),
                        "a rejected proposal must roll back its source and brief"
                    );
                    assert!(db
                        .plan_continuation_links(OWNER, ORG, TEAM, &receipt.source_work_id)
                        .unwrap()
                        .is_empty());
                    assert!(db
                        .plan_continuation_links("outsider", ORG, TEAM, &receipt.source_work_id)
                        .is_err());
                })
                .await
                .unwrap();
            let receipt = workspace
                .continue_plan(&source, prepare(started.receipt.root_work_id.clone()))
                .await
                .unwrap();
            assert_eq!(
                workspace
                    .continue_plan(&source, prepare(started.receipt.root_work_id))
                    .await
                    .unwrap(),
                receipt,
                "a second tab/request must reopen the same proposal"
            );
            let view = workspace
                .plan_view(&receipt.continuation_work_id)
                .await
                .unwrap();
            assert_eq!(view.continuation_from, Some(receipt.clone()));
            assert!(view.execution.is_none());
            assert!(view.generation.is_none());
            assert_eq!(view.plans[0].status, "draft");
            let content = view.plans[0].content.as_ref().unwrap();
            assert_eq!(content.assignments.len(), 1);
            assert_eq!(
                content.assignments[0].key,
                if scenario == 10 {
                    "check"
                } else {
                    "assemble-result"
                }
            );
            assert!(content.assignments[0].depends_on.is_empty());
            assert_eq!(content.token_budget, 2000);
            assert_eq!(receipt.retained.len(), if scenario == 10 { 1 } else { 2 });
            assert_eq!(
                receipt.review_before_repeat.len(),
                if scenario == 10 { 1 } else { 0 }
            );
            let brief = workspace
                .work_briefs(&receipt.continuation_work_id)
                .await
                .unwrap();
            assert!(brief[0].body.contains("COMPARE_RESULT"));
            assert!(!brief[0].body.contains("PRIVATE_CANARY"));
            let discussion = workspace.conversation_input(&uuid::Uuid::new_v4().to_string(),shaping::GUIDE,Some(&receipt.continuation_work_id),"Help me adjust the remaining work").await.unwrap();
            assert!(discussion.contains("COMPARE_RESULT"));
            assert!(!discussion.contains("PRIVATE_CANARY"));
            assert!(workspace.conversation_input(&uuid::Uuid::new_v4().to_string(),shaping::GUIDE,Some(&source),"An ordinary unstarted source is still blocked").await.is_err());
            assert_eq!(
                calls.lock().unwrap().len(),
                count,
                "preparing/reading/retrying must not use inference"
            );
            assert_eq!(
                saved_work(&workspace.execution_view(&source).await.unwrap().unwrap()),
                saved_work(&original)
            );
            assert_eq!(
                serde_json::to_value(
                    recorded_usage(&workspace)
                        .await
                        .into_iter()
                        .filter(|r| r.work_id != receipt.continuation_work_id)
                        .collect::<Vec<_>>()
                )
                .unwrap(),
                serde_json::to_value(&usage).unwrap()
            );
            (source, receipt, original, usage)
        })
        .await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open_with_workspace(
                    database,
                    "qwen3.5:latest".into(),
                    url,
                    Some(folder),
                )
                .await
                .unwrap(),
            );
            assert_eq!(
                workspace.plan_view(&source).await.unwrap().continuation_to,
                Some(receipt.clone())
            );
            workspace
                .update_plan(
                    &receipt.continuation_work_id,
                    PlanCommand::Agree {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        revision: 1,
                    },
                )
                .await
                .unwrap();
            let start = || StartPlan {
                request_id: receipt.continuation_work_id.clone(),
                revision: 1,
                reviewed_previous_actions: scenario == 10,
                ..Default::default()
            };
            if scenario == 10 {
                let mut unreviewed = start();
                unreviewed.reviewed_previous_actions = false;
                assert!(workspace
                    .start_plan(&receipt.continuation_work_id, unreviewed)
                    .await
                    .err()
                    .unwrap()
                    .employee_message()
                    .contains("Review the earlier assignment actions"));
                assert!(workspace
                    .execution_receipt(&receipt.continuation_work_id)
                    .await
                    .unwrap()
                    .is_none());
            }
            let first = workspace
                .start_plan(&receipt.continuation_work_id, start())
                .await
                .unwrap();
            assert_eq!(
                workspace
                    .start_plan(&receipt.continuation_work_id, start())
                    .await
                    .unwrap()
                    .receipt,
                first.receipt
            );
            let complete = wait(&workspace, &receipt.continuation_work_id, "completed").await;
            assert_ne!(complete.receipt.root_work_id, original.receipt.root_work_id);
            assert_eq!(complete.assignments.len(), 1);
            assert!(complete.assignments[0].input.contains("COMPARE_RESULT"));
            assert!(contribution_text(complete.root.as_ref().unwrap())
                .unwrap()
                .contains("RECOVERED_COMBINED_RESULT"));
            assert_eq!(
                saved_work(&workspace.execution_view(&source).await.unwrap().unwrap()),
                saved_work(&original)
            );
            let new_usage = recorded_usage(&workspace).await;
            for row in &usage {
                assert_eq!(
                    serde_json::to_value(
                        new_usage.iter().find(|r| r.work_id == row.work_id).unwrap()
                    )
                    .unwrap(),
                    serde_json::to_value(row).unwrap()
                );
            }
            assert!(
                workspace
                    .continue_plan(
                        &receipt.continuation_work_id,
                        ContinuePlan {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            expected_root_work_id: complete.receipt.root_work_id
                        }
                    )
                    .await
                    .is_err(),
                "successful work must not offer recovery"
            );
            let requests = calls.lock().unwrap();
            let new_children: Vec<_> = requests
                .iter()
                .filter(|r| {
                    r["messages"].to_string().contains("Continuation of")
                        && !r["tools"].to_string().contains(DISPATCH)
                })
                .collect();
            assert_eq!(new_children.len(), 1, "completed workers never run again");
        })
        .await;
    server.abort();
}

async fn set_tools(workspace: &LocalWorkspace, key: &str, tools: Vec<String>) {
    let stored = workspace.registered_agent(key).await.unwrap();
    let agent = workspace.agent_profile(key.into(), &stored).unwrap();
    workspace
        .update_agent(UpdateLocalAgent {
            agent_key: key.into(),
            expected_definition_digest: agent.definition_digest,
            configuration: CreateLocalAgent {
                request_id: uuid::Uuid::new_v4().to_string(),
                provider: agent.provider,
                name: agent.name,
                purpose: agent.purpose,
                model: agent.model,
                harness: agent.harness,
                max_steps: agent.max_steps,
                max_seconds: agent.max_seconds,
                max_tokens: agent.max_tokens,
                tools: Some(tools),
                hosted_consent: false,
                hosted_tools_consent: false,
                expected_workspace_root: None,
            },
        })
        .await
        .unwrap();
}

fn saved_work(view: &PlanExecutionView) -> Value {
    let mut value = serde_json::to_value(view).unwrap();
    // Projection cursors can advance as the existing run's completion watcher
    // journals finalization. Compare saved outcomes and pins, not that cursor.
    if let Some(root) = value["root"].as_object_mut() {
        root.remove("sequence");
    }
    for assignment in value["assignments"].as_array_mut().unwrap() {
        assignment.as_object_mut().unwrap().remove("sequence");
    }
    value
}

fn copy_fixture_files(source: &std::path::Path, target: &std::path::Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture_files(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}
