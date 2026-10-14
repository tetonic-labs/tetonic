//! Offline regression: real registered workers, ledger, staged writes and
//! finalization; a scripted loopback provider charges realistic growing inputs.
//! This verifies engine mechanics, not frontier-model output quality.
use super::*;

pub(super) fn reply(request: &Value, calls: &[Value]) -> (&'static str, Value) {
    if request["messages"][0]["content"]
        .as_str()
        .unwrap_or_default()
        .starts_with("Maintain a concise factual working record")
    {
        return ("SUMMARY", json!({}));
    }
    if let Some(assignment) = width::assignment(request) {
        let count = calls
            .iter()
            .filter(|call| width::assignment(call).is_some_and(|a| a.key == assignment.key))
            .count();
        let path = format!("{}.txt", assignment.key);
        match count {
            1..=10 => (
                "read_file",
                json!({"path":format!("source-{}.txt", count - 1), "max_bytes":4096}),
            ),
            11 => (
                "write_file",
                json!({"path":path, "content":format!("{}: verified source facts\n", assignment.key)}),
            ),
            12 => ("read_file", json!({"path":path})),
            _ => {
                assert!(
                    request["messages"]
                        .to_string()
                        .contains("verified source facts"),
                    "must inspect the staged bytes before finishing"
                );
                (
                    "finish",
                    json!({"summary":format!("DELIVERED_{}: inspected {}", assignment.key, path)}),
                )
            }
        }
    } else if !request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool")
    {
        (DISPATCH, json!({"assignment_keys":["compare", "check"]}))
    } else {
        let text = request["messages"].to_string();
        if !text.contains("DELIVERED_compare") || !text.contains("DELIVERED_check") {
            return (
                "ask_human",
                json!({"question":"Inspect the failed test worker", "why":"Worker did not deliver"}),
            );
        }
        (
            "finish",
            json!({"summary":"Both workers delivered inspected files"}),
        )
    }
}

pub(super) fn response(request: &Value, tool: &str, args: Value) -> Value {
    let req = tetonic_inference::ChatRequest {
        model: "qwen3.5:latest".into(),
        messages: serde_json::from_value(request["messages"].clone()).unwrap(),
        tools: request
            .get("tools")
            .filter(|v| !v.is_null())
            .map(|v| serde_json::from_value(v.clone()).unwrap())
            .unwrap_or_default(),
        ..Default::default()
    };
    let message = if tool == "SUMMARY" {
        json!({"role":"assistant", "content":"Read the earlier numbered source files. Their factual marker is: verified source facts. Preserve the original assignment and output name. No output has yet been accepted. Inspect staged output before finishing."})
    } else {
        json!({"role":"assistant", "content":"", "tool_calls":[{"function":{"name":tool,"arguments":args}}]})
    };
    json!({"model":"qwen3.5:latest", "message":message, "done":true, "prompt_eval_count":req.estimated_input_tokens(), "eval_count":100})
}

#[tokio::test]
async fn growing_context_is_metered_compacted_and_workers_deliver_inspected_files() {
    run_delivery(false).await;
}

#[tokio::test]
async fn budget_stop_after_writing_retains_drafts_without_publishing_them() {
    run_delivery(true).await;
}

async fn run_delivery(exhaust_after_write: bool) {
    let dir = tempfile::tempdir().unwrap();
    let files = dir.path().join("files");
    std::fs::create_dir(&files).unwrap();
    let (url, calls, server) = scripted_server(if exhaust_after_write { 21 } else { 20 }).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut config = crate::host::HostConfiguration::default();
            config.workspace_execution.max_tokens = 180_000;
            config.workspace_execution.coordination_max_tokens = 20_000;
            config.workspace_execution.context_tokens = 128_000;
            let workspace = Rc::new(
                LocalWorkspace::open_with_configuration(
                    dir.path().join("context.db"),
                    "qwen3.5:latest".into(),
                    url,
                    Some(files.clone()),
                    config,
                )
                .await
                .unwrap(),
            );
            let root = workspace
                .services
                .host
                .settings
                .workspace_root
                .clone()
                .unwrap();
            let source_text = format!(
                "verified source facts\n{}",
                "Scoped evidence about the assignment.\n".repeat(105)
            );
            for n in 0..10 {
                std::fs::write(root.join(format!("source-{n}.txt")), &source_text).unwrap();
            }
            let source = seed_options(&workspace, false, true).await;
            let mut content = workspace.plan_view(&source).await.unwrap().plans[0]
                .content
                .clone()
                .unwrap();
            for assignment in &mut content.assignments {
                let agent = workspace
                    .create_agent(CreateLocalAgent {
                        workspace_root: None,
                        expected_workspace_root: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        name: assignment.key.clone(),
                        purpose:
                            "Read the sources, write the assigned file, inspect it and finish."
                                .into(),
                        provider: "ollama".into(),
                        model: "qwen3.5:latest".into(),
                        harness: "general".into(),
                        tools: Some(vec!["read_file".into(), "write_file".into()]),
                        max_steps: 16,
                        max_seconds: 120,
                        max_tokens: 80_000,
                        hosted_consent: false,
                        hosted_tools_consent: false,
                    })
                    .await
                    .unwrap();
                assignment.agent_key = agent.key;
                assignment.tools = vec!["read_file".into(), "write_file".into()];
                assignment.token_budget = 80_000;
                assignment.deliverable =
                    format!("{}.txt, inspected before completion", assignment.key);
            }
            content.token_budget = 180_000;
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
            let complete = tokio::time::timeout(std::time::Duration::from_secs(45), async {
                loop {
                    let view = workspace.execution_view(&source).await.unwrap().unwrap();
                    if matches!(
                        view.state.as_str(),
                        "completed" | "failed" | "waiting_human" | "canceled"
                    ) {
                        break view;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                }
            })
            .await
            .unwrap();
            if exhaust_after_write {
                assert_eq!(complete.state, "waiting_human");
                assert!(complete.assignments.iter().all(|a| a.state != "completed"));
                assert!(!root.join("compare.txt").exists());
                assert!(!root.join("check.txt").exists());
                let drafts: Vec<_> = std::fs::read_dir(root.join(".lokai/artifacts"))
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with("draft-"))
                    .collect();
                assert_eq!(drafts.len(), 2);
                for draft in drafts {
                    let value: Value =
                        serde_json::from_slice(&std::fs::read(draft.path()).unwrap()).unwrap();
                    assert_eq!(value["commit_succeeded"], false);
                    assert!(!value["task_id"].is_null());
                    assert!(!value["attempt_id"].is_null());
                    let bytes: Vec<u8> =
                        serde_json::from_value(value["contents"][0]["bytes"].clone()).unwrap();
                    assert!(String::from_utf8(bytes)
                        .unwrap()
                        .contains("verified source facts"));
                }
                let root_task = complete.root.as_ref().unwrap();
                let receipt: Value = root_task
                    .messages
                    .iter()
                    .filter(|m| m.role == "tool")
                    .find_map(|m| serde_json::from_str::<Value>(&m.content).ok())
                    .unwrap();
                let results = receipt["results"].as_array().unwrap();
                assert_eq!(results.len(), 2);
                assert!(results.iter().all(|result| {
                    result["ok"] == false
                        && result["result"]["contribution"].is_null()
                        && result["result"]["failure"]["reason"]
                            .as_str()
                            .unwrap()
                            .contains("too little allowance")
                }));
                workspace.cancel(&root_task.id).await.unwrap();
                assert_eq!(
                    groups::terminal(&workspace, &source).await.state,
                    "canceled"
                );
                let usage = settled_usage(&workspace).await;
                assert!(usage
                    .iter()
                    .all(|r| !r.over_limit && r.unknown_calls == 0 && r.held_tokens == 0));
                return;
            }
            assert_eq!(
                complete.state,
                "completed",
                "{:?}",
                complete
                    .assignments
                    .iter()
                    .map(|a| (&a.state, &a.error))
                    .collect::<Vec<_>>()
            );
            for key in ["compare", "check"] {
                assert_eq!(
                    std::fs::read_to_string(root.join(format!("{key}.txt"))).unwrap(),
                    format!("{key}: verified source facts\n")
                );
            }
            for n in 0..10 {
                assert_eq!(
                    std::fs::read_to_string(root.join(format!("source-{n}.txt"))).unwrap(),
                    source_text
                );
            }
            // Unrelated contracts no longer inflate every worker request.
            assert!(!complete.assignments[0].input.contains("check.txt"));
            let (call_count, expected_usage) = {
                let calls = calls.lock().unwrap();
                let summaries: Vec<_> = calls
                    .iter()
                    .filter(|r| {
                        r["messages"][0]["content"]
                            .as_str()
                            .unwrap_or_default()
                            .starts_with("Maintain a concise factual working record")
                    })
                    .collect();
                assert!(
                    summaries.len() >= 2,
                    "each worker must compact under cumulative budget pressure"
                );
                assert!(summaries
                    .iter()
                    .all(|r| r["options"]["num_predict"].as_u64().unwrap() <= 1024));
                let call_count = calls.len();
                let expected_usage: u64 = calls
                    .iter()
                    .map(|r| {
                        response(r, "SUMMARY", json!({}))["prompt_eval_count"]
                            .as_u64()
                            .unwrap()
                            + 100
                    })
                    .sum();
                (call_count, expected_usage)
            };
            let usage = settled_usage(&workspace).await;
            assert_eq!(
                usage
                    .iter()
                    .map(|r| r.input_tokens + r.output_tokens)
                    .sum::<i64>() as u64,
                expected_usage,
                "all compaction calls must be metered in the existing ledger"
            );
            assert_eq!(
                usage.iter().map(|r| r.calls).sum::<i64>() as usize,
                call_count
            );
            assert!(usage.iter().all(|r| !r.over_limit && r.held_tokens == 0));
            assert!(expected_usage < 180_000);
        })
        .await;
    server.abort();
}
