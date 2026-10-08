use super::*;

/// Seeds explicit demonstration input only. The UI must start the agreed plan;
/// all subsequent questions and contributions come from the installed model.
#[tokio::test]
#[ignore = "requires a fresh local handoff proof directory"]
async fn seed_local_handoff_plan() {
    let directory = std::path::PathBuf::from(
        std::env::var("TETONIC_HANDOFF_PROOF_DIR").expect("set a fresh proof directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("workspace.db");
    assert!(!path.exists(), "proof must use a fresh database");
    let workspace = Rc::new(
        LocalWorkspace::open(
            path,
            "qwen3.5:latest".into(),
            "http://127.0.0.1:11434".into(),
        )
        .await
        .unwrap(),
    );
    let source = seed_variant(&workspace, true).await;
    let mut content = workspace.plan_view(&source).await.unwrap().plans[0]
        .content
        .clone()
        .unwrap();
    content.token_budget = 14596;
    content.summary="Compare workshop formats for the owner's audience, review facilitation risks independently, then prepare a recommendation.".into();
    for a in &mut content.assignments {
        a.token_budget = 3500;
    }
    content.assignments[0].instructions="Compare one 60-minute workshop with three 20-minute sessions. The audience has deliberately not been specified. Before comparing, use ask_human to ask whether the audience is beginners or experienced participants, explaining briefly why it matters. After the answer, call finish with a comparison under 80 words for that audience. Use reasoning only.".into();
    content.assignments[1].instructions="Independently identify two practical facilitation risks shared by a long workshop and several short sessions. You do not need to know the audience to name general risks. Call finish with under 60 words. Use reasoning only.".into();
    content.assignments[2].instructions="Use the comparison and independent risks to propose a practical next step. Call finish with under 80 words. State assumptions.".into();
    if std::env::var("TETONIC_HANDOFF_TWO_ASSIGNMENTS").is_ok() {
        content.assignments.truncate(2);
        content.token_budget = 11096;
        content.summary = "Compare workshop formats for the owner's audience and independently assess facilitation risks.".into();
    }
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
    std::fs::write(directory.join("shape-id.txt"), source).unwrap();
}

async fn wait_for(
    workspace: &LocalWorkspace,
    source: &str,
    predicate: impl Fn(&PlanExecutionView) -> bool,
) -> PlanExecutionView {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let view = workspace.execution_view(source).await.unwrap().unwrap();
            if predicate(&view) {
                return view;
            }
            assert!(
                !matches!(view.state.as_str(), "failed" | "recovery_required"),
                "{}",
                serde_json::to_string(&view).unwrap()
            );
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("plan did not reach the expected state")
}

#[tokio::test]
async fn human_wait_allows_independent_work_and_direction_preserves_attempts_usage_and_contributions(
) {
    human_wait_and_direction(2).await;
}

#[tokio::test]
async fn conversational_workers_complete_with_natural_answers_after_handoff() {
    human_wait_and_direction(4).await;
}

async fn human_wait_and_direction(scenario: u8) {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("human.db");
    let (url, calls, server) = scripted_server(scenario).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url.clone())
                    .await
                    .unwrap(),
            );
            let source = seed_variant(&workspace, true).await;
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
            let waiting = wait_for(&workspace, &source, |v| {
                v.assignments[0].state == "waiting_human" && v.assignments[1].state == "completed"
            })
            .await;
            let progress = workspace
                .controller_reader()
                .read(&waiting.receipt)
                .await
                .unwrap();
            assert_eq!(
                progress.assignments[0].state,
                tetonic_memory::AssignmentState::WaitingHuman
            );
            assert!(
                progress.assignments[0].holds_capacity,
                "live-only waits must not pretend their slot is released"
            );
            assert_eq!(
                progress.assignments[1].state,
                tetonic_memory::AssignmentState::Completed
            );
            assert_eq!(
                progress.assignments[2].state,
                tetonic_memory::AssignmentState::NotStarted
            );
            assert!(
                !crate::team_work_controller::ready_assignments(&progress).contains(&"wrap".into()),
                "unanswered producer is not completed evidence"
            );
            let question = waiting.assignments[0].human_questions[0].clone();
            assert_eq!(waiting.assignments[2].state, "not_started");
            // A reply must include a sibling which completed while another
            // assignment waited. It must not imply the waiting/dependent work
            // completed or leak private exploration into the coordinator.
            let undelivered = Arc::new(Mutex::new(HashSet::from([
                "compare".to_owned(),
                "check".to_owned(),
                "wrap".to_owned(),
            ])));
            let waiting_reply = || {
                tetonic_domain::ToolOutcome::ok(
                    "Waiting",
                    json!({"state":"waiting_human","assignment_key":"compare"}).to_string(),
                )
            };
            let reply = workspace
                .collect_plan_contributions(
                    &waiting.receipt,
                    "compare",
                    waiting_reply(),
                    &undelivered,
                )
                .await
                .unwrap();
            let body: Value = serde_json::from_str(&reply.content).unwrap();
            assert_eq!(body["also_completed"].as_array().unwrap().len(), 1);
            assert_eq!(body["also_completed"][0]["assignment_key"], "check");
            assert_eq!(
                body["also_completed"][0]["contribution"],
                contribution_text(&waiting.assignments[1]).unwrap()
            );
            assert_eq!(body["outstanding_assignments"], json!(["compare", "wrap"]));
            assert!(!reply.content.contains("PRIVATE_CANARY_MUST_NOT_LEAK"));
            let repeat = workspace
                .collect_plan_contributions(
                    &waiting.receipt,
                    "compare",
                    waiting_reply(),
                    &undelivered,
                )
                .await
                .unwrap();
            assert!(
                serde_json::from_str::<Value>(&repeat.content).unwrap()["also_completed"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            // All model calls settle, then answering consumes no inference while waiting.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let count = calls.lock().unwrap().len();
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            assert_eq!(calls.lock().unwrap().len(), count);
            assert_eq!(count, 5, "three coordinator calls and two children");
            let amendment_id = uuid::Uuid::new_v4().to_string();
            let amendment = || AmendPlanAssignment {
                request_id: amendment_id.clone(),
                expected_revision: 0,
                assignment_key: "wrap".into(),
                instructions: "WRAP_CANARY DIRECTION_CANARY Explain the answer for beginners"
                    .into(),
            };
            let direction = workspace
                .amend_plan_assignment(&source, amendment())
                .await
                .unwrap();
            assert_eq!(
                direction.affected_work_ids,
                vec![waiting.assignments[2].id.clone()]
            );
            assert!(direction
                .retained_work_ids
                .contains(&waiting.assignments[1].id));
            assert_eq!(
                workspace
                    .amend_plan_assignment(&source, amendment())
                    .await
                    .unwrap(),
                direction
            );
            for key in ["compare", "check", "wrap"] {
                assert!(workspace
                    .amend_plan_assignment(
                        &source,
                        AmendPlanAssignment {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            expected_revision: 0,
                            assignment_key: key.into(),
                            instructions: "stale or started".into()
                        }
                    )
                    .await
                    .is_err());
            }
            for key in ["compare", "check"] {
                assert!(workspace
                    .amend_plan_assignment(
                        &source,
                        AmendPlanAssignment {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            expected_revision: 1,
                            assignment_key: key.into(),
                            instructions: "Cannot rewrite started work".into()
                        }
                    )
                    .await
                    .is_err());
            }
            assert!(workspace
                .amend_plan_assignment(
                    &source,
                    AmendPlanAssignment {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: i64::MAX,
                        assignment_key: "wrap".into(),
                        instructions: "overflow rejected".into()
                    }
                )
                .await
                .is_err());
            let q = question.clone();
            workspace
                .keys
                .store
                .read(move |db| {
                    assert!(db
                        .work_human_questions("outsider", ORG, TEAM, &q.work_id)
                        .is_err());
                    assert!(db
                        .answer_work_human(tetonic_memory::AnswerWorkHuman {
                            actor: "outsider",
                            org: ORG,
                            team: TEAM,
                            work: &q.work_id,
                            id: &q.id,
                            request: "bad",
                            answer: "answer",
                            now: 0
                        })
                        .is_err());
                    assert!(db
                        .answer_work_human(tetonic_memory::AnswerWorkHuman {
                            actor: OWNER,
                            org: ORG,
                            team: TEAM,
                            work: &q.work_id,
                            id: &q.id,
                            request: "expired",
                            answer: "answer",
                            now: q.deadline + 1
                        })
                        .is_err());
                    assert!(db
                        .answer_work_human(tetonic_memory::AnswerWorkHuman {
                            actor: OWNER,
                            org: ORG,
                            team: "another-team",
                            work: &q.work_id,
                            id: &q.id,
                            request: "wrong-scope",
                            answer: "answer",
                            now: 0
                        })
                        .is_err());
                })
                .await
                .unwrap();
            let response_id = uuid::Uuid::new_v4().to_string();
            let answer = || AnswerPlanQuestion {
                request_id: response_id.clone(),
                question_id: question.id.clone(),
                answer: "ANSWER_CANARY Beginners".into(),
            };
            let saved = workspace
                .answer_plan_question(&question.work_id, answer())
                .await
                .unwrap();
            assert_eq!(saved.response_id.as_deref(), Some(response_id.as_str()));
            assert!(workspace
                .answer_plan_question(
                    &question.work_id,
                    AnswerPlanQuestion {
                        answer: "different answer".into(),
                        ..answer()
                    }
                )
                .await
                .is_err());
            let completed = wait_for(&workspace, &source, |v| v.state == "completed").await;
            assert_eq!(completed.receipt, started.receipt);
            assert!(completed
                .assignments
                .iter()
                .all(|a| a.state == "completed"
                    && a.run_id == completed.root.as_ref().unwrap().run_id));
            assert!(contribution_text(&completed.assignments[0])
                .unwrap()
                .contains("ANSWER_CANARY"));
            assert!(contribution_text(&completed.assignments[2])
                .unwrap()
                .contains("DIRECTION_CANARY"));
            assert_eq!(
                contribution_text(&completed.assignments[1]).unwrap(),
                contribution_text(&waiting.assignments[1]).unwrap()
            );
            assert_eq!(
                completed.assignments[0].human_questions[0].attempt_id,
                question.attempt_id
            );
            let requests = calls.lock().unwrap().clone();
            assert_eq!(
                requests.len(),
                9,
                "no duplicate contribution or inference while waiting"
            );
            assert!(!serde_json::to_string(&requests)
                .unwrap()
                .contains("PRIVATE_CANARY_MUST_NOT_LEAK"));
            let usage = settled_usage(&workspace).await;
            assert_eq!(
                usage
                    .iter()
                    .map(|r| r.input_tokens + r.output_tokens)
                    .sum::<i64>(),
                270
            );
            assert_eq!(usage.iter().map(|r| r.held_tokens).sum::<i64>(), 0);
            assert_eq!(
                workspace
                    .amend_plan_assignment(&source, amendment())
                    .await
                    .unwrap(),
                direction,
                "lost acknowledgement remains retryable after completion"
            );
            let reopened = LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let receipt = reopened
                .answer_plan_question(&question.work_id, answer())
                .await
                .unwrap();
            assert_eq!(receipt.response_id, Some(response_id.clone()));
            assert_eq!(
                reopened.plan_directions(&source).await.unwrap(),
                vec![direction]
            );
            assert_eq!(calls.lock().unwrap().len(), 9);
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn stopped_human_wait_rejects_answers_and_changes_without_reviving_work() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(2).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let path = dir.path().join("stop.db");
            let workspace = Rc::new(
                LocalWorkspace::open(path.clone(), "qwen3.5:latest".into(), url.clone())
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
            let waiting = wait_for(&workspace, &source, |v| {
                v.assignments[0].state == "waiting_human" && v.assignments[1].state == "completed"
            })
            .await;
            let question = waiting.assignments[0].human_questions[0].clone();
            workspace
                .cancel(&waiting.receipt.root_work_id)
                .await
                .unwrap();
            let stopped = wait_for(&workspace, &source, |v| v.state == "canceled").await;
            assert_eq!(stopped.assignments[0].state, "canceled");
            assert_eq!(stopped.assignments[1].state, "completed");
            assert_eq!(stopped.assignments[2].state, "not_started");
            let count = calls.lock().unwrap().len();
            assert!(workspace
                .answer_plan_question(
                    &question.work_id,
                    AnswerPlanQuestion {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        question_id: question.id.clone(),
                        answer: "Too late".into()
                    }
                )
                .await
                .is_err());
            assert!(workspace
                .amend_plan_assignment(
                    &source,
                    AmendPlanAssignment {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: 0,
                        assignment_key: "wrap".into(),
                        instructions: "Restart secretly".into()
                    }
                )
                .await
                .is_err());
            let reopened = LocalWorkspace::open(path, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            assert!(reopened
                .answer_plan_question(
                    &question.work_id,
                    AnswerPlanQuestion {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        question_id: question.id,
                        answer: "Restart secretly".into()
                    }
                )
                .await
                .is_err());
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            assert_eq!(calls.lock().unwrap().len(), count);
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn coordinator_answers_reach_workers_through_scoped_plan_context() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(3).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("clarify.db"), "qwen3.5:latest".into(), url)
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
            let waiting = wait_for(&workspace, &source, |v| v.state == "waiting_human").await;
            let question = waiting.root.as_ref().unwrap().human_questions[0].clone();
            assert!(waiting.assignments.iter().all(|a| a.state == "not_started"));
            let q = question.clone();
            workspace
                .keys
                .store
                .write(move |db| {
                    assert!(db
                        .ask_work_human(tetonic_memory::AskWorkHuman {
                            actor: OWNER,
                            org: ORG,
                            team: TEAM,
                            work: &q.work_id,
                            attempt: &q.attempt_id,
                            id: "second-simultaneous-question",
                            content: q.content.clone(),
                            now: chrono::Utc::now().timestamp() as u64
                        })
                        .is_err());
                })
                .await
                .unwrap();
            workspace
                .answer_plan_question(
                    &question.work_id,
                    AnswerPlanQuestion {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        question_id: question.id,
                        answer: "ROOT_ANSWER_CANARY Beginners".into(),
                    },
                )
                .await
                .unwrap();
            let completed = wait_for(&workspace, &source, |v| v.state == "completed").await;
            for task in completed.assignments {
                assert!(
                    task.human_questions.is_empty(),
                    "workers already have the audience"
                );
                assert!(task.input.contains("ROOT_ANSWER_CANARY"));
                assert!(!task.input.contains("PRIVATE_CANARY_MUST_NOT_LEAK"));
            }
            assert_eq!(calls.lock().unwrap().len(), 8);
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn unanswered_question_expires_without_a_false_starting_or_retry_state() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = scripted_server(2).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = Rc::new(
                LocalWorkspace::open(dir.path().join("expire.db"), "qwen3.5:latest".into(), url)
                    .await
                    .unwrap(),
            );
            let source = seed_variant(&workspace, true).await;
            let agent = workspace
                .create_agent(CreateLocalAgent {
                    provider: "ollama".into(),
                    hosted_consent: false,
                    hosted_tools_consent: false,
                    expected_workspace_root: None,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    name: "Short wait".into(),
                    purpose: "Ask when information is missing; finish when answered.".into(),
                    model: "qwen3.5:latest".into(),
                    harness: "general".into(),
                    max_steps: 4,
                    max_seconds: 10,
                    max_tokens: 4096,
                    tools: Some(vec![]),
                })
                .await
                .unwrap();
            let mut content = workspace.plan_view(&source).await.unwrap().plans[0]
                .content
                .clone()
                .unwrap();
            content.assignments[0].agent_key = agent.key;
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
            workspace
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
            let waiting = wait_for(&workspace, &source, |v| {
                v.assignments[0].state == "waiting_human"
            })
            .await;
            let q = waiting.assignments[0].human_questions[0].clone();
            let expired =
                wait_for(&workspace, &source, |v| v.assignments[0].state == "failed").await;
            assert!(expired.assignments[0]
                .error
                .as_ref()
                .unwrap()
                .contains("time limit"));
            assert_eq!(expired.assignments[1].state, "completed");
            assert!(workspace
                .answer_plan_question(
                    &q.work_id,
                    AnswerPlanQuestion {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        question_id: q.id,
                        answer: "Too late".into()
                    }
                )
                .await
                .is_err());
            let failed = wait_for(&workspace, &source, |v| v.state == "failed").await;
            assert_eq!(failed.assignments[2].state, "not_started");
            let run_id = failed.assignments[0].run_id.clone().unwrap();
            workspace
                .keys
                .store
                .read(move |db| {
                    let run = db.load_run_snapshot(&run_id).unwrap().unwrap();
                    let attempt = run
                        .attempts
                        .get(&tetonic_domain::AttemptId::new(q.attempt_id))
                        .unwrap();
                    assert_eq!(attempt.state, tetonic_domain::AttemptState::TimedOut);
                    let task = &run.tasks[&attempt.task_id];
                    assert_eq!(task.state, tetonic_domain::TaskState::Failed);
                    assert_eq!(task.binding.retry_policy.max_attempts, 1);
                })
                .await
                .unwrap();
            assert!(
                calls.lock().unwrap().len() <= 19,
                "model retries remain bounded"
            );
        })
        .await;
    server.abort();
}

#[tokio::test]
async fn restarted_wait_remains_inspectable_but_an_answer_cannot_revive_its_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("restart.db");
    let (url, calls, server) = scripted_server(2).await;
    let (source, question) = {
        let runtime = tokio::task::LocalSet::new();
        runtime
            .run_until(async {
                let workspace = Rc::new(
                    LocalWorkspace::open(path.clone(), "qwen3.5:latest".into(), url.clone())
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
                let view = wait_for(&workspace, &source, |v| {
                    v.assignments[0].state == "waiting_human"
                        && v.assignments[1].state == "completed"
                })
                .await;
                (source, view.assignments[0].human_questions[0].clone())
            })
            .await
        // Dropping this LocalSet drops live execution without issuing a user stop.
    };
    let count = calls.lock().unwrap().len();
    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open(path, "qwen3.5:latest".into(), url)
                .await
                .unwrap();
            let view = workspace.execution_view(&source).await.unwrap().unwrap();
            assert!(!matches!(
                view.state.as_str(),
                "running" | "waiting_human" | "completed"
            ));
            assert_eq!(view.assignments[0].human_questions[0].id, question.id);
            assert!(
                workspace
                    .continue_plan(
                        &source,
                        ContinuePlan {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            expected_root_work_id: view.receipt.root_work_id.clone(),
                        }
                    )
                    .await
                    .is_err(),
                "a crashed unresolved attempt is not a terminal retry"
            );
            assert!(workspace
                .answer_plan_question(
                    &question.work_id,
                    AnswerPlanQuestion {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        question_id: question.id,
                        answer: "Do not restart".into()
                    }
                )
                .await
                .is_err());
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            assert_eq!(calls.lock().unwrap().len(), count);
        })
        .await;
    server.abort();
}
