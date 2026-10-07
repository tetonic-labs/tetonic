use super::*;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "plan_human_tests.rs"]
mod human;

#[path = "plan_document_tests.rs"]
mod documents;

#[path = "plan_group_tests.rs"]
mod groups;

async fn settled_usage(workspace: &LocalWorkspace) -> Vec<tetonic_memory::WorkUsage> {
    // The run journal publishes the result before the registered executor's
    // completion watcher settles usage. Observe that separate durable boundary;
    // a missing settlement must still fail, rather than racing a single read.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let usage = workspace
                .local
                .resources()
                .team_work_usage(&workspace.host.credential, ORG.into(), TEAM.into())
                .await
                .unwrap();
            if !usage.is_empty() && usage.iter().all(|row| row.held_tokens == 0) {
                return usage;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("completed plan must release known unused reservations")
}

async fn server(hang_child: bool) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    scripted_server(if hang_child { 1 } else { 0 }).await
}

pub(in crate::local_workspace) async fn scripted_server(
    scenario: u8,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(Mutex::new(vec![]));
    let captured = calls.clone();
    let handle = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let captured = captured.clone();
            tokio::spawn(async move {
                let mut bytes = vec![];
                let (end, len) = loop {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        let len = String::from_utf8_lossy(&bytes[..end])
                            .lines()
                            .find_map(|line| {
                                let (k, v) = line.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (end + 4, len);
                    }
                };
                while bytes.len() < end + len {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                }
                let header = String::from_utf8_lossy(&bytes[..end]);
                let reply = if header.starts_with("POST /api/chat ") {
                    let request: Value = serde_json::from_slice(&bytes[end..end + len]).unwrap();
                    let child = request["messages"].as_array().unwrap().iter().any(|m| {
                        m["role"] == "user"
                            && m["content"]
                                .as_str()
                                .unwrap_or("")
                                .contains("Your agreed assignment:")
                    });
                    let (tool, args) = {
                        let mut calls = captured.lock().unwrap();
                        calls.push(request.clone());
                        if matches!(scenario, 2..=4) {
                            if child {
                                let input = request["messages"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .find(|m| m["role"] == "user")
                                    .unwrap()["content"]
                                    .as_str()
                                    .unwrap();
                                if !input.contains("ROOT_ANSWER_CANARY")
                                    && input.contains("COMPARE_CANARY")
                                    && !input.contains("WRAP_CANARY")
                                    && !request["messages"]
                                        .as_array()
                                        .unwrap()
                                        .iter()
                                        .any(|m| m["role"] == "tool")
                                {
                                    (
                                        "ask_human",
                                        json!({"question":"Which audience matters most?","why":"The recommendation depends on who attends.","options":["Beginners","Experienced participants"]}),
                                    )
                                } else if input.contains("WRAP_CANARY") {
                                    (
                                        "finish",
                                        json!({"summary":format!("WRAP_RESULT: {}",if input.contains("DIRECTION_CANARY") {"DIRECTION_CANARY applied"} else {"original direction"})}),
                                    )
                                } else if input.contains("CHECK_CANARY") {
                                    (
                                        "finish",
                                        json!({"summary":"CHECK_RESULT independent review retained"}),
                                    )
                                } else {
                                    (
                                        "finish",
                                        json!({"summary":format!("COMPARE_RESULT: {}",if request["messages"].to_string().contains("ANSWER_CANARY") {"ANSWER_CANARY received"} else {"missing answer"})}),
                                    )
                                }
                            } else {
                                let count = calls
                                    .iter()
                                    .filter(|r| r["tools"].to_string().contains(DISPATCH))
                                    .count();
                                if scenario == 3 {
                                    match count {
                                        1 => (
                                            "ask_human",
                                            json!({"question":"Which audience matters most?","why":"Choose a plan-wide audience."}),
                                        ),
                                        2 => (DISPATCH, json!({"assignment_key":"compare"})),
                                        3 => (DISPATCH, json!({"assignment_key":"check"})),
                                        4 => (DISPATCH, json!({"assignment_key":"wrap"})),
                                        _ => (
                                            "finish",
                                            json!({"summary":"COMBINED_RESULT with owner clarification"}),
                                        ),
                                    }
                                } else {
                                    match count {
                                        1 | 3 => (DISPATCH, json!({"assignment_key":"compare"})),
                                        2 => (DISPATCH, json!({"assignment_key":"check"})),
                                        4 => (DISPATCH, json!({"assignment_key":"wrap"})),
                                        _ => (
                                            "finish",
                                            json!({"summary":"COMBINED_RESULT with human direction"}),
                                        ),
                                    }
                                }
                            }
                        } else if child {
                            (
                                "finish",
                                json!({"summary":if request["messages"].to_string().contains("CHECK_CANARY") {"CHECK_RESULT: the comparison relies on an unstated attendance assumption."} else {"COMPARE_RESULT: shorter workshops offer more scheduling flexibility."}}),
                            )
                        } else {
                            let count = calls
                                .iter()
                                .filter(|r| r["tools"].to_string().contains(DISPATCH))
                                .count();
                            if matches!(scenario, 5..=9) {
                                groups::reply(scenario, count)
                            } else {
                                match count {
                                    1 => (
                                        "finish",
                                        json!({"summary":"Premature completion must be rejected"}),
                                    ),
                                    2 => (DISPATCH, json!({"assignment_key":"check"})),
                                    3 | 4 => (DISPATCH, json!({"assignment_key":"compare"})),
                                    5 => (DISPATCH, json!({"assignment_key":"check"})),
                                    _ => (
                                        "finish",
                                        json!({"summary":"COMBINED_RESULT: shorter workshops are more flexible; verify attendance before committing."}),
                                    ),
                                }
                            }
                        }
                    };
                    if child && matches!(scenario, 8 | 9) {
                        // Neither worker returns until both HTTP requests have arrived.
                        // This fails if either dispatch or model admission serializes them.
                        tokio::time::timeout(std::time::Duration::from_secs(10), async {
                            loop {
                                let count = captured
                                    .lock()
                                    .unwrap()
                                    .iter()
                                    .filter(|r| !r["tools"].to_string().contains(DISPATCH))
                                    .count();
                                if count >= 2 {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            }
                        })
                        .await
                        .expect("independent agents must overlap at inference");
                    }
                    if child && matches!(scenario, 1 | 7 | 9) {
                        let _ = stream.read(&mut [0; 1]).await;
                        return;
                    }
                    if scenario == 4 && child && tool == "finish" {
                        json!({"model":"qwen3.5:latest","message":{"role":"assistant","content":args["summary"]},"done":true,"prompt_eval_count":10,"eval_count":20})
                    } else {
                        json!({"model":"qwen3.5:latest","message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":tool,"arguments":args}}]},"done":true,"prompt_eval_count":10,"eval_count":20})
                    }
                } else if header.starts_with("POST /api/generate ") {
                    json!({"model":"qwen3.5:latest","done":true,"response":""})
                } else {
                    json!({"models":[{"name":"qwen3.5:latest","model":"qwen3.5:latest","size":1,"size_vram":1}],"capabilities":["completion","tools"]})
                };
                let body = format!("{reply}\n");
                let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (url, calls, handle)
}

async fn seed(workspace: &LocalWorkspace) -> String {
    seed_variant(workspace, false).await
}

async fn seed_variant(workspace: &LocalWorkspace, handoff: bool) -> String {
    seed_options(workspace, handoff, false).await
}

pub(in crate::local_workspace) async fn seed_options(
    workspace: &LocalWorkspace,
    handoff: bool,
    parallel: bool,
) -> String {
    let source = uuid::Uuid::new_v4().to_string();
    let resources = workspace.local.resources();
    let secret = &workspace.host.credential;
    resources
        .create_team_work_item_for_purpose(
            secret,
            crate::resources::CreateTeamWorkItem {
                org: ORG.into(),
                team: TEAM.into(),
                work_id: source.clone(),
                title: "Explore workshops".into(),
                request_id: format!("{source}@{}", shaping::GUIDE),
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
                body: "Compare one long workshop with several short sessions using reasoning only."
                    .into(),
            },
        )
        .await
        .unwrap();
    let worker = workspace
        .create_agent(CreateLocalAgent {
            provider: "ollama".into(),
            hosted_consent: false,
            hosted_tools_consent: false,
            expected_workspace_root: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            name: "Reviewer".into(),
            purpose: "Review the explicit supplied evidence; call finish with the result.".into(),
            model: "qwen3.5:latest".into(),
            harness: "general".into(),
            max_steps: 4,
            max_seconds: 120,
            max_tokens: 4096,
            tools: Some(vec![]),
        })
        .await
        .unwrap();
    let mut content:PlanContent=serde_json::from_value(json!({"title":"Workshop options","summary":"Compare options, then check assumptions","token_budget":3000,"open_questions":[],"assignments":[
        {"key":"compare","title":"Compare formats","instructions":"COMPARE_CANARY Compare formats","agent_key":AGENT,"depends_on":[],"tools":[],"deliverable":"Comparison","token_budget":1000},
        {"key":"check","title":"Check assumptions","instructions":"CHECK_CANARY Review the comparison","agent_key":worker.key,"depends_on":["compare"],"tools":[],"deliverable":"Risks","token_budget":1000}
    ]})).unwrap();
    if parallel {
        content.assignments[1].depends_on.clear();
    }
    if handoff {
        content.token_budget = 8000;
        content.assignments[0].token_budget = 2000;
        content.assignments[1].depends_on.clear();
        content.assignments[1].token_budget = 2000;
        let mut wrap = content.assignments[1].clone();
        wrap.key = "wrap".into();
        wrap.title = "Prepare recommendation".into();
        wrap.instructions = "WRAP_CANARY Use both contributions".into();
        wrap.depends_on = vec!["compare".into(), "check".into()];
        content.assignments.push(wrap);
    }
    resources
        .mutate_huddle_plan(
            secret,
            ORG.into(),
            TEAM.into(),
            source.clone(),
            crate::resources::PlanMutation::Save {
                request: uuid::Uuid::new_v4().to_string(),
                expected: 0,
                brief_revision: 1,
                generation_id: uuid::Uuid::new_v4().to_string(),
                generation_input: "Test-generated plan".into(),
                content: Some(content),
            },
        )
        .await
        .unwrap();
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
async fn agreed_plan_dispatches_two_agents_once_on_one_runtime_and_retains_scoped_contributions() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("plan.db");
    let (url, calls, server) = server(false).await;
    let work = async {
        let workspace = Rc::new(
            LocalWorkspace::open(database.clone(), "qwen3.5:latest".into(), url.clone())
                .await
                .unwrap(),
        );
        let source = seed(&workspace).await;
        assert!(
            workspace
                .plan_view(&source)
                .await
                .unwrap()
                .execution_available
        );
        let request = uuid::Uuid::new_v4().to_string();
        let first = workspace
            .start_plan(
                &source,
                StartPlan {
                    request_id: request.clone(),
                    revision: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let retry = workspace
            .start_plan(
                &source,
                StartPlan {
                    request_id: request.clone(),
                    revision: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(first.receipt, retry.receipt);
        assert!(workspace
            .start_plan(
                &source,
                StartPlan {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    revision: 1,
                    ..Default::default()
                }
            )
            .await
            .is_err());
        let complete = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let view = workspace.execution_view(&source).await.unwrap().unwrap();
                if view.state == "completed" {
                    break view;
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
        let root = complete.root.unwrap();
        assert_eq!(
            root.plan.as_ref().unwrap().depends_on,
            complete
                .assignments
                .iter()
                .map(|task| task.id.clone())
                .collect::<Vec<_>>()
        );
        assert!(contribution_text(&root)
            .unwrap()
            .contains("COMBINED_RESULT"));
        assert_eq!(complete.assignments.len(), 2);
        assert!(complete.assignments[1]
            .input
            .contains("Dependency contributions"));
        assert!(complete.assignments[1].input.contains("COMPARE_RESULT"));
        for task in &complete.assignments {
            assert_eq!(task.state, "completed");
            assert_eq!(task.run_id, root.run_id);
            assert!(!contribution_text(task).unwrap().contains("COMBINED_RESULT"));
        }
        let usage = settled_usage(&workspace).await;
        assert_eq!(
            usage
                .iter()
                .map(|r| r.input_tokens + r.output_tokens)
                .sum::<i64>(),
            240
        );
        assert_eq!(usage.iter().map(|r| r.held_tokens).sum::<i64>(), 0);
        assert_ne!(
            root.plan.as_ref().unwrap().information_context_id,
            workspace.context
        );
        assert!(complete.assignments.iter().all(|task| task
            .plan
            .as_ref()
            .unwrap()
            .information_context_id
            == root.plan.as_ref().unwrap().information_context_id));
        assert_ne!(
            complete.assignments[0].agent_key,
            complete.assignments[1].agent_key
        );
        assert_eq!(
            complete.assignments[1].plan.as_ref().unwrap().depends_on,
            vec![complete.assignments[0].id.clone()]
        );
        let requests = calls.lock().unwrap().clone();
        assert_eq!(requests.len(),8,"premature finish + dependency denial + two child calls + duplicate receipt + final synthesis");
        assert!(!serde_json::to_string(&requests)
            .unwrap()
            .contains("PRIVATE_CANARY_MUST_NOT_LEAK"));
        let child_requests: Vec<_> = requests
            .iter()
            .filter(|r| {
                r["messages"].as_array().unwrap().iter().any(|m| {
                    m["role"] == "user"
                        && m["content"]
                            .as_str()
                            .unwrap_or("")
                            .contains("Your agreed assignment:")
                })
            })
            .collect();
        assert_eq!(child_requests.len(), 2);
        assert!(child_requests[1]["messages"]
            .to_string()
            .contains("COMPARE_RESULT"));
        assert!(!child_requests[0]["messages"]
            .to_string()
            .contains("CHECK_RESULT"));
        let reopened = Rc::new(
            LocalWorkspace::open(database, "qwen3.5:latest".into(), url)
                .await
                .unwrap(),
        );
        let replay = reopened
            .start_plan(
                &source,
                StartPlan {
                    request_id: request,
                    revision: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(replay.state, "completed");
        assert_eq!(calls.lock().unwrap().len(), 8);
    };
    tokio::task::LocalSet::new().run_until(work).await;
    server.abort();
}

#[tokio::test]
async fn stale_brief_and_no_coordination_budget_block_start_without_inference() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = server(false).await;
    let work = async {
        let workspace = Rc::new(
            LocalWorkspace::open(dir.path().join("plan.db"), "qwen3.5:latest".into(), url)
                .await
                .unwrap(),
        );
        let source = seed(&workspace).await;
        let mut content = workspace.plan_view(&source).await.unwrap().plans[0]
            .content
            .clone()
            .unwrap();
        content.token_budget = 2000;
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
        assert!(workspace
            .start_plan(
                &source,
                StartPlan {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    revision: 2,
                    ..Default::default()
                }
            )
            .await
            .is_err());
        workspace
            .save_work_brief(
                &source,
                SaveWorkBrief {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: 1,
                    body: "Changed direction".into(),
                },
            )
            .await
            .unwrap();
        assert!(workspace
            .start_plan(
                &source,
                StartPlan {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    revision: 2,
                    ..Default::default()
                }
            )
            .await
            .is_err());
        assert!(calls.lock().unwrap().is_empty());
        assert!(workspace
            .execution_receipt(&source)
            .await
            .unwrap()
            .is_none());
    };
    tokio::task::LocalSet::new().run_until(work).await;
    server.abort();
}

#[tokio::test]
async fn stopping_a_plan_cancels_child_wait_without_starting_the_next_assignment() {
    let dir = tempfile::tempdir().unwrap();
    let (url, calls, server) = server(true).await;
    let work = async {
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
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while calls.lock().unwrap().len() < 4 {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        workspace
            .cancel(&started.receipt.root_work_id)
            .await
            .unwrap();
        let stopped = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let view = workspace.execution_view(&source).await.unwrap().unwrap();
                if view.state == "canceled" {
                    break view;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(stopped.assignments[0].state, "canceled");
        assert_eq!(stopped.assignments[1].state, "not_started");
        assert_eq!(calls.lock().unwrap().len(), 4);
    };
    tokio::task::LocalSet::new().run_until(work).await;
    server.abort();
}

/// Opt-in proof with the installed local model. Creates a fresh database only;
/// leaves its real records available to the isolated UI after completion/failure.
#[tokio::test]
#[ignore = "requires the installed local Ollama model and a fresh proof directory"]
async fn local_model_plan_journey() {
    let directory = std::path::PathBuf::from(
        std::env::var("TETONIC_PLAN_PROOF_DIR").expect("set a fresh proof directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("workspace.db");
    assert!(!database.exists(), "use a fresh database");
    let work = async {
        let workspace = Rc::new(
            LocalWorkspace::open(
                database,
                "qwen3.5:latest".into(),
                "http://127.0.0.1:11434".into(),
            )
            .await
            .unwrap(),
        );
        let source = seed(&workspace).await;
        let mut content = workspace.plan_view(&source).await.unwrap().plans[0]
            .content
            .clone()
            .unwrap();
        content.token_budget = 11096;
        content.assignments[0].token_budget = 3500;
        content.assignments[1].token_budget = 3500;
        content.assignments[0].instructions="Compare one 60-minute workshop with three 20-minute sessions. Use reasoning only; state assumptions. Keep your contribution under 120 words.".into();
        content.assignments[1].instructions="Review the supplied comparison for missing assumptions and tradeoffs. Name the strongest reason to choose each format. Keep your contribution under 120 words.".into();
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
        std::fs::write(directory.join("shape-id.txt"), &source).unwrap();
        std::fs::write(directory.join("root-id.txt"), &started.receipt.root_work_id).unwrap();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(390), async {
            loop {
                let view = workspace.execution_view(&source).await.unwrap().unwrap();
                if !matches!(view.state.as_str(), "starting" | "running" | "canceling") {
                    break view;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        })
        .await
        .unwrap();
        std::fs::write(
            directory.join("result.json"),
            serde_json::to_vec_pretty(&outcome).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("snapshot.json"),
            serde_json::to_vec_pretty(&workspace.snapshot().await.unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            outcome.state, "completed",
            "The real outcome is retained in result.json; do not call a failed run successful."
        );
    };
    tokio::task::LocalSet::new().run_until(work).await;
}
