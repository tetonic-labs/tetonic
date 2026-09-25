use crate::daemon::{Daemon, Dispatch};
use serde_json::{json, Value};
use tetonic_app::{SharedStore, LOCAL_NODE_ID};
use tetonic_rpc::channel_pair;
use tetonic_rpc::protocol::*;

use super::harness::*;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[test]
fn rpc_handle_rejects_before_initialize() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        std::env::remove_var("LOKAI_RPC_INSECURE");
        let (notifier, mut rx) = channel_pair(512);
        let mut daemon = Daemon::new(notifier, None, None);
        daemon
            .handle(
                Incoming {
                    jsonrpc: Some("2.0".into()),
                    id: Some(json!(1)),
                    method: methods::SESSION_START.into(),
                    params: json!({}),
                },
                json!(1),
            )
            .await;
        let resp = rx.recv().await.unwrap();
        assert!(resp.contains("initialize"));
    }));
}

#[test]
fn rpc_initialize_rejects_wrong_token() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        std::env::remove_var("LOKAI_RPC_INSECURE");
        let (notifier, _rx) = channel_pair(512);
        let mut daemon = Daemon::new(notifier, None, None);
        let token = daemon.rpc_token().to_string();
        let err = daemon
            .initialize(json!({
                "protocol_version": 1,
                "workspace_root": std::env::temp_dir().join("tetonicd-rpc-auth-test").display().to_string(),
                "rpc_token": "wrong"
            }))
            .await
            .unwrap_err();
        assert_eq!(err.message, "invalid rpc_token");
        assert_ne!(token, "wrong");
    }));
}

#[test]
fn orchestration_auto_routes_to_specialist() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn("Planning.\n", vec![("list_dir", json!({}))]),
            turn(
                "",
                vec![("finish", json!({"summary":"step 1 then step 2 planned"}))],
            ),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);

        let sid = {
            let v = daemon
                .session_start(json!({ "orchestration": "auto", "critic": false }))
                .await
                .unwrap();
            v["session_id"].as_str().unwrap().to_string()
        };
        daemon
            .chat_send(json!({
                "session_id": sid,
                "text": "Plan the refactor for the auth module"
            }))
            .unwrap();

        let events = drain_until_run_status(&mut rx).await;
        let ids = agent_ids_of(&events);
        assert!(
            ids.iter().any(|id| id == "a0_s0"),
            "expected specialist agent_id a0_s0, got {ids:?}"
        );
        let logs = log_messages(&events);
        assert!(
            logs.iter()
                .any(|m| m.contains("router:") && m.contains("planner")),
            "router log missing: {logs:?}"
        );
        assert_eq!(events.last().unwrap()["params"]["status"], "ok");
    }));
}

#[test]
fn orchestration_in_loop_spawn_agent() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn(
                "",
                vec![(
                    "spawn_agent",
                    json!({"role": "planner", "task": "outline repo layout"}),
                )],
            ),
            turn("", vec![("finish", json!({"summary":"layout plan"}))]),
            turn(
                "",
                vec![("finish", json!({"summary":"delegated to planner"}))],
            ),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);

        let sid = {
            let v = daemon
                .session_start(json!({ "orchestration": "auto", "critic": false }))
                .await
                .unwrap();
            v["session_id"].as_str().unwrap().to_string()
        };
        daemon
            .chat_send(json!({
                "session_id": sid,
                "text": "Summarize this codebase structure"
            }))
            .unwrap();

        let events = drain_until_run_status(&mut rx).await;
        let ids = agent_ids_of(&events);
        assert!(
            ids.iter().any(|id| id == "a0_s0"),
            "expected spawned planner a0_s0, got {ids:?}"
        );
        assert_eq!(events.last().unwrap()["params"]["status"], "ok");
    }));
}

#[test]
fn orchestration_in_loop_spawn_agent_exhausts_budget() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let _env_guard = ENV_LOCK.lock().await;
        std::env::set_var("LOKAI_MAX_SPAWN_PER_TURN", "1");
        struct ClearEnv(&'static str);
        impl Drop for ClearEnv {
            fn drop(&mut self) {
                std::env::remove_var(self.0);
            }
        }
        let _clear = ClearEnv("LOKAI_MAX_SPAWN_PER_TURN");
        let turns = vec![
            turn(
                "",
                vec![(
                    "spawn_agent",
                    json!({"role": "planner", "task": "first spawn"}),
                )],
            ),
            turn("", vec![("finish", json!({"summary":"plan a"}))]),
            turn(
                "",
                vec![(
                    "spawn_agent",
                    json!({"role": "planner", "task": "second spawn should fail"}),
                )],
            ),
            turn("", vec![("finish", json!({"summary":"done anyway"}))]),
        ];
        let (mut daemon, mut rx, dir) = daemon_with_mock(turns.clone());
        // Child jobs resolve the persisted parent identity before exercising
        // the spawn budget. A storeless mock fails before reaching that check.
        let sink = std::sync::Arc::new(crate::daemon::events::DaemonEventSink::new(
            daemon.notifier.clone(),
        ));
        daemon.services.as_mut().unwrap().app = tetonic_app::Application::bootstrap_mock_with_store(
            &dir,
            Some(SharedStore::open(":memory:", 1).unwrap()),
            sink,
            turns,
        );

        let sid = {
            let v = daemon
                .session_start(json!({ "orchestration": "auto", "critic": false }))
                .await
                .unwrap();
            v["session_id"].as_str().unwrap().to_string()
        };
        daemon
            .chat_send(json!({
                "session_id": sid,
                "text": "Run spawn workflow test"
            }))
            .unwrap();

        let events = drain_until_run_status(&mut rx).await;
        let tool_results: Vec<_> = events
            .iter()
            .filter(|e| e["method"] == "event/tool_result")
            .collect();
        assert!(
            tool_results.iter().any(|e| {
                e["params"]["ok"] == false
                    && e["params"]["summary"]
                        .as_str()
                        .unwrap_or("")
                        .contains("spawn budget")
            }),
            "expected spawn budget error in tool results: {tool_results:?}"
        );
    }));
}

#[test]
fn agent_spawn_runs_planner_turn() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn("Steps.\n", vec![("read_file", json!({"path":"a.txt"}))]),
            turn(
                "",
                vec![(
                    "finish",
                    json!({"summary":"Plan: read a.txt, then outline the migration in steps."}),
                )],
            ),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);

        let sid = daemon.session_start(json!({})).await.unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        let spawn = daemon
            .agent_spawn(json!({
                "session_id": sid,
                "role": "planner",
                "task": "Outline migration steps for a.txt"
            }))
            .unwrap();
        assert_eq!(spawn["accepted"], true);
        let spawned_id = spawn["agent_id"].as_str().unwrap().to_string();
        assert!(spawned_id.starts_with("a0_s"));

        let events = drain_until_run_status(&mut rx).await;
        let ids = agent_ids_of(&events);
        assert!(
            ids.iter().any(|id| id == &spawned_id),
            "spawned agent should stream events, got {ids:?}"
        );
        assert!(methods_of(&events).iter().any(|m| m == "event/tool_call"));
        assert_eq!(events.last().unwrap()["params"]["status"], "ok");
    }));
}

#[test]
fn agent_spawn_rejects_spoofed_parent_agent_id() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn("Planning.\n", vec![("list_dir", json!({}))]),
            turn("", vec![("finish", json!({"summary":"plan steps"}))]),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);

        let sid = {
            let v = daemon
                .session_start(json!({ "orchestration": "auto", "critic": false }))
                .await
                .unwrap();
            v["session_id"].as_str().unwrap().to_string()
        };
        daemon
            .chat_send(json!({
                "session_id": sid,
                "text": "Plan the auth refactor"
            }))
            .unwrap();
        let _events = drain_until_run_status(&mut rx).await;

        let err = daemon
            .agent_spawn(json!({
                "session_id": sid,
                "role": "planner",
                "task": "nested task",
                "parent_agent_id": "a0"
            }))
            .unwrap_err();
        assert!(
            err.message.contains("active spawn anchor"),
            "expected parent spoof rejection, got: {}",
            err.message
        );
    }));
}

#[test]
fn agent_spawn_coder_denies_run_shell() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn("", vec![("run_shell", json!({"command":"echo pwn"}))]),
            turn("", vec![("finish", json!({"summary":"done"}))]),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);

        let sid = daemon.session_start(json!({})).await.unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        daemon
            .agent_spawn(json!({
                "session_id": sid,
                "role": "coder",
                "task": "Try shell"
            }))
            .unwrap();

        let events = drain_until_run_status(&mut rx).await;
        let tool_results: Vec<_> = events
            .iter()
            .filter(|e| e["method"] == "event/tool_result")
            .collect();
        assert!(
            tool_results.iter().any(|e| {
                e["params"]["ok"] == false
                    && e["params"]["summary"]
                        .as_str()
                        .unwrap_or("")
                        .contains("run_shell")
            }),
            "spawned coder should deny run_shell: {tool_results:?}"
        );
    }));
}

#[test]
fn initialize_rejects_when_sessions_active() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let (mut daemon, _rx, dir) = daemon_with_mock(vec![]);
        daemon.session_start(json!({})).await.unwrap();
        let err = daemon
            .initialize(json!({
                "protocol_version": 1,
                "workspace_root": dir.display().to_string(),
                "rpc_token": daemon.rpc_token()
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("re-initialize"));
    }));
}

#[test]
fn agent_spawn_shared_budget_exhausted_on_second_rpc() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let _env_guard = ENV_LOCK.lock().await;
        std::env::set_var("LOKAI_MAX_SPAWN_PER_TURN", "1");
        struct ClearEnv(&'static str);
        impl Drop for ClearEnv {
            fn drop(&mut self) {
                std::env::remove_var(self.0);
            }
        }
        let _clear = ClearEnv("LOKAI_MAX_SPAWN_PER_TURN");
        let turns = vec![
            turn("ok.\n", vec![("list_dir", json!({}))]),
            turn("", vec![("finish", json!({"summary":"first"}))]),
            turn("", vec![("finish", json!({"summary":"second"}))]),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);
        let sid = daemon.session_start(json!({})).await.unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        daemon
            .agent_spawn(json!({
                "session_id": sid,
                "role": "planner",
                "task": "first spawn"
            }))
            .unwrap();
        drain_until_run_status(&mut rx).await;

        daemon
            .agent_spawn(json!({
                "session_id": sid,
                "role": "planner",
                "task": "second spawn should fail budget"
            }))
            .unwrap();
        let events = drain_until_run_status(&mut rx).await;
        assert!(
            events.iter().any(|e| {
                e["method"] == "event/run_status"
                    && e["params"]["status"] == "error"
                    && e["params"]["error"]
                        .as_str()
                        .unwrap_or("")
                        .contains("spawn budget")
            }),
            "expected spawn budget error: {events:?}"
        );
    }));
}

#[test]
fn session_start_coerces_data_class_floor() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let (mut daemon, _rx, dir) = daemon_with_mock(vec![]);
        std::fs::write(dir.join(".env"), "SECRET=x\n").unwrap();
        let v = daemon
            .session_start(json!({ "data_class": "personal" }))
            .await
            .unwrap();
        assert_eq!(v["data_class"], "secret");
    }));
}

#[test]
fn agent_spawn_rejects_unknown_role() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let (mut daemon, _rx, _dir) = daemon_with_mock(vec![]);
        let sid = daemon.session_start(json!({})).await.unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let err = daemon
            .agent_spawn(json!({
                "session_id": sid,
                "role": "janitor",
                "task": "clean up"
            }))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest.code());
        assert!(
            err.message.contains("unknown role"),
            "unexpected message: {}",
            err.message
        );
    }));
}

#[test]
fn orchestration_critic_runs_after_verify_recovery() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn(
                "",
                vec![(
                    "edit_file",
                    json!({
                        "path": "a.txt",
                        "old_string": "hello",
                        "new_string": "hello!"
                    }),
                )],
            ),
            turn("", vec![("finish", json!({"summary":"first finish"}))]),
            turn("", vec![("finish", json!({"summary":"second finish"}))]),
            turn("", vec![("finish", json!({"summary":"APPROVE"}))]),
        ];
        let (mut daemon, mut rx, dir) = daemon_with_mock(turns);
        write_verify_once_py(&dir);
        let sid = {
            let v = daemon
                .session_start(json!({
                    "orchestration": "auto",
                    "critic": true,
                    "verify_cmd": "python verify_once.py"
                }))
                .await
                .unwrap();
            v["session_id"].as_str().unwrap().to_string()
        };
        daemon
            .chat_send(json!({
                "session_id": sid,
                "text": "Implement a friendlier greeting in a.txt"
            }))
            .unwrap();
        let events = drain_until_run_status(&mut rx).await;
        let ids = agent_ids_of(&events);
        assert!(ids.iter().any(|id| id == "a0_s0"), "coder: {ids:?}");
        assert!(
            !ids.iter().any(|id| id == "a0_s1"),
            "in-loop verify no longer feeds leftover critic: {ids:?}"
        );
    }));
}

#[test]
fn orchestration_critic_skipped_when_verify_passes() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn(
                "",
                vec![(
                    "edit_file",
                    json!({
                        "path": "a.txt",
                        "old_string": "hello",
                        "new_string": "hello!"
                    }),
                )],
            ),
            turn("", vec![("finish", json!({"summary":"updated greeting"}))]),
        ];
        let (mut daemon, mut rx, dir) = daemon_with_mock(turns);
        write_verify_pass_py(&dir);

        let sid = {
            let v = daemon
                .session_start(json!({
                    "orchestration": "auto",
                    "critic": true,
                    "verify_cmd": "python verify_pass.py"
                }))
                .await
                .unwrap();
            v["session_id"].as_str().unwrap().to_string()
        };
        daemon
            .chat_send(json!({
                "session_id": sid,
                "text": "Implement a friendlier greeting in a.txt"
            }))
            .unwrap();

        let events = drain_until_run_status(&mut rx).await;
        let ids = agent_ids_of(&events);
        assert!(
            ids.iter().any(|id| id == "a0_s0"),
            "coder specialist: {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id == "a0_s1"),
            "critic should skip: {ids:?}"
        );
        let text = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert_eq!(text, "hello!");
    }));
}

#[test]
fn orchestration_critic_revise_triggers_coder_revision() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn(
                "",
                vec![(
                    "edit_file",
                    json!({
                        "path": "a.txt",
                        "old_string": "hello",
                        "new_string": "hello!"
                    }),
                )],
            ),
            turn("", vec![("finish", json!({"summary":"first pass"}))]),
            turn("", vec![("finish", json!({"summary":"second pass"}))]),
            turn(
                "",
                vec![(
                    "finish",
                    json!({"summary":"REVISE: add exclamation consistency check"}),
                )],
            ),
            turn("", vec![("finish", json!({"summary":"revision done"}))]),
        ];
        let (mut daemon, mut rx, dir) = daemon_with_mock(turns);
        write_verify_once_py(&dir);

        let sid = daemon
            .session_start(json!({
                "orchestration": "auto",
                "critic": true,
                "verify_cmd": "python verify_once.py"
            }))
            .await
            .unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        daemon
            .chat_send(json!({
                "session_id": sid,
                "text": "Implement greeting tweak in a.txt"
            }))
            .unwrap();

        let events = drain_until_run_status(&mut rx).await;
        let ids = agent_ids_of(&events);
        assert!(
            ids.iter().any(|id| id == "a0_s0"),
            "coder specialist: {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id == "a0_s2"),
            "in-loop verify/critic revision ended with 016: {ids:?}"
        );
    }));
}

#[test]
fn fabric_status_returns_snapshot() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let (daemon, _rx, _dir) = daemon_with_mock(vec![]);
        let v = daemon.fabric_status().await.unwrap();
        assert_eq!(v["effective_concurrency"], 1);
        assert_eq!(v["nodes"][0]["id"], LOCAL_NODE_ID);
        assert_eq!(v["nodes"][0]["healthy"], true);
    }));
}

#[test]
fn chat_send_streams_tokens_tools_and_ok() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn("Looking.\n", vec![("list_dir", json!({}))]),
            turn("", vec![("finish", json!({"summary":"done"}))]),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);

        let sid = {
            let v = daemon.session_start(json!({})).await.unwrap();
            v["session_id"].as_str().unwrap().to_string()
        };
        let acc = daemon
            .chat_send(json!({ "session_id": sid, "text": "list files" }))
            .unwrap();
        assert_eq!(acc["accepted"], true);

        let events = drain_until_run_status(&mut rx).await;
        let methods = methods_of(&events);

        // Every notification carries agent_id = root.
        for e in &events {
            assert_eq!(e["params"]["agent_id"], "a0", "missing agent_id on {e}");
        }
        assert!(methods.iter().any(|m| m == "event/token"), "{methods:?}");
        assert!(
            methods.iter().any(|m| m == "event/tool_call"),
            "{methods:?}"
        );
        assert!(
            methods.iter().any(|m| m == "event/tool_result"),
            "{methods:?}"
        );
        // A finish tool_call appears among tool_calls.
        assert!(events
            .iter()
            .any(|e| e["method"] == "event/tool_call" && e["params"]["tool"] == "list_dir"));
        // Terminal status is ok.
        let last = events.last().unwrap();
        assert_eq!(last["method"], "event/run_status");
        assert_eq!(last["params"]["status"], "ok");
    }));
}

#[test]
fn run_shell_routes_through_approval_and_denies() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let turns = vec![
            turn("", vec![("run_shell", json!({"command":"echo hi"}))]),
            turn("", vec![("finish", json!({"summary":"done"}))]),
        ];
        let (mut daemon, mut rx, _dir) = daemon_with_mock(turns);
        let sid = daemon
            .session_start(json!({ "allow_shell": true }))
            .await
            .unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        daemon
            .chat_send(json!({ "session_id": sid, "text": "run it" }))
            .unwrap();

        // First, the approval request must arrive; capture its id.
        let mut approval_id = None;
        let mut saw_denied = false;
        let mut terminal = false;
        while let Some(s) = rx.recv().await {
            let v: Value = serde_json::from_str(&s).unwrap();
            match v["method"].as_str().unwrap_or("") {
                "event/approval_request" => {
                    approval_id = v["params"]["approval_id"].as_str().map(String::from);
                    // Respond with deny.
                    let id = approval_id.clone().unwrap();
                    let ack = daemon
                        .approval_respond(
                            json!({ "session_id": sid, "approval_id": id, "decision": "deny" }),
                        )
                        .unwrap();
                    assert_eq!(ack["ok"], true);
                }
                "event/tool_result" => {
                    if v["params"]["tool_call_id"].as_str().is_some()
                        && v["params"]["error_kind"] == "denied"
                    {
                        saw_denied = true;
                    }
                }
                "event/run_status" if v["params"]["status"] != "started" => {
                    terminal = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(approval_id.is_some(), "no approval request emitted");
        assert!(saw_denied, "denied tool_result not observed");
        assert!(terminal);
    }));
}

#[test]
fn session_end_persists_and_allows_fresh_start() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let store = SharedStore::open(":memory:", 1).unwrap();
        let dir = std::env::temp_dir().join(format!("tetonicd-end-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().to_string();
        let (mut daemon, _rx) = daemon_with_store_and_mock(store.clone(), root.clone(), vec![]);
        let sid = daemon.session_start(json!({})).await.unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        daemon
            .session_end(json!({ "session_id": sid, "status": "ok" }))
            .unwrap();
        assert!(!daemon.services().unwrap().app.sessions.has_live(&sid));
        assert_eq!(
            store
                .read_sync(|db| db.session_status(&sid))
                .unwrap()
                .unwrap()
                .as_deref(),
            Some("ok")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }));
}

#[test]
fn session_resume_recovery_required_when_mid_execution() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let store = SharedStore::open(":memory:", 1).unwrap();
        let dir = std::env::temp_dir().join(format!("tetonicd-recovery-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().to_string();
        let sid = store
            .write_sync({
                let root = root.clone();
                move |db| db.start_session(&root, "single-agent", "mock")
            })
            .unwrap()
            .unwrap();
        let sid_clone = sid.clone();
        store
            .write_sync(move |db| db.append_message(&sid_clone, "user", "", "prior turn", None))
            .unwrap()
            .unwrap();
        let sid_clone = sid.clone();
        store
            .write_sync(move |db| {
                db.upsert_turn_operation(
                    &sid_clone,
                    "turn_crash",
                    "executing",
                    r#"{"tool":"run_shell"}"#,
                )
            })
            .unwrap()
            .unwrap();

        let (mut daemon, _rx) = daemon_with_store_and_mock(store, root, vec![]);
        let v = daemon
            .session_start(json!({ "resume": true }))
            .await
            .unwrap();
        assert_eq!(v["resume_state"], "recovery_required");
        assert_eq!(v["session_id"], sid);
        let _ = std::fs::remove_dir_all(&dir);
    }));
}

#[test]
fn session_resume_loads_audit_messages() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let store = SharedStore::open(":memory:", 1).unwrap();
        let dir = std::env::temp_dir().join(format!("tetonicd-resume-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().to_string();
        let sid = {
            let root = root.clone();
            store
                .write_sync(move |db| {
                    let sid = db.start_session(&root, "single-agent", "mock").unwrap();
                    db.append_message(&sid, "system", "", "you are lokai", None)
                        .unwrap();
                    db.append_message(&sid, "user", "", "prior turn", None)
                        .unwrap();
                    db.end_session(&sid, "ok", None).unwrap();
                    sid
                })
                .unwrap()
        };

        let (mut daemon, _rx) = daemon_with_store_and_mock(store, root, vec![]);
        let v = daemon
            .session_start(json!({ "resume": true }))
            .await
            .unwrap();
        assert_eq!(v["resumed"], true);
        assert_eq!(v["messages_loaded"], 2);
        assert_eq!(v["session_id"], sid);
    }));
}

#[test]
fn egress_policy_set_rejected_by_default() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        std::env::remove_var("LOKAI_ALLOW_RPC_EGRESS");
        std::env::remove_var("LOKAI_STRICT_RPC");
        let (mut daemon, _rx, _dir) = daemon_with_mock(vec![]);
        let err = daemon
            .egress_policy_set(json!({
                "add": [{ "label": "dev", "ip": "127.0.0.1", "port": 8080 }],
                "remove": []
            }))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest.code());
    });
}

#[test]
fn strict_rpc_blocks_policy_set() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        std::env::set_var("LOKAI_STRICT_RPC", "1");
        let (mut daemon, _rx, _dir) = daemon_with_mock(vec![]);
        let err = daemon
            .policy_set(json!({ "mode": "estate_stub" }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest.code());
        std::env::remove_var("LOKAI_STRICT_RPC");
    });
}

#[test]
fn run_snapshot_and_resume_rpc() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let (mut daemon, _rx, dir) = daemon_with_mock(vec![]);
        let sid = daemon.session_start(json!({})).await.unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let app = daemon.services().unwrap().app.clone();
        let plan = app
            .runs
            .plan_turn(&tetonic_app::commands::RunTurnCommand {
                session_id: sid,
                user_input: "hi".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .await
            .unwrap();
        let snap = daemon
            .run_snapshot(json!({ "run_id": plan.run_id.to_string() }))
            .await
            .unwrap();
        assert_eq!(snap["run_id"], plan.run_id.to_string());
        assert!(snap["sequence"].as_u64().unwrap() > 0);
        let resume = daemon
            .run_resume(json!({
                "run_id": plan.run_id.to_string(),
                "after_sequence": 0,
                "limit": 32
            }))
            .await
            .unwrap();
        assert!(!resume["events"].as_array().unwrap().is_empty());
        let _ = dir;
    }));
}

#[test]
fn run_cancel_without_session_rpc() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        let (mut daemon, _rx, dir) = daemon_with_mock(vec![]);
        let _sid = daemon.session_start(json!({})).await.unwrap();
        let app = daemon.services().unwrap().app.clone();
        let run_id = app
            .runs
            .create_run(tetonic_app::commands::CreateRunCommand {
                session_id: None,
                root_task_id: None,
                ..Default::default()
            })
            .await
            .unwrap();
        let canceled = daemon
            .run_cancel(json!({ "run_id": run_id.to_string() }))
            .await
            .unwrap();
        assert_eq!(canceled["canceled"], true);
        let snap = daemon
            .run_snapshot(json!({ "run_id": run_id.to_string() }))
            .await
            .unwrap();
        assert!(snap["state"].as_str().unwrap().contains("cancel"));
        let _ = dir;
    }));
}

#[test]
fn main_rs_does_not_special_case_shutdown_before_handle() {
    let src = include_str!("../../main.rs");
    assert!(
        !src.contains("if msg.method == methods::SHUTDOWN"),
        "production main.rs must not answer SHUTDOWN before handle"
    );
    assert!(
        !src.contains("LOKAI_RPC_TOKEN="),
        "production main.rs must not eprintln the token prefix"
    );
}

#[test]
fn chat_send_calls_run_turn_not_execute_turn() {
    let src = include_str!("../handlers/chat.rs");
    assert!(
        src.contains("submit_chat_turn"),
        "chat_send must invoke Application::submit_chat_turn"
    );
    assert!(
        !src.contains("turn_execution::execute_turn"),
        "chat_send must not skip to turn_execution::execute_turn"
    );
}

#[test]
fn handle_shutdown_before_initialize_is_not_ready() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        std::env::remove_var("LOKAI_RPC_INSECURE");
        let (notifier, mut rx) = channel_pair(512);
        let mut daemon = Daemon::new(notifier, None, None);
        let dispatch = daemon
            .handle(
                Incoming {
                    jsonrpc: Some("2.0".into()),
                    id: Some(json!(1)),
                    method: methods::SHUTDOWN.into(),
                    params: json!({}),
                },
                json!(1),
            )
            .await;
        assert_eq!(dispatch, Dispatch::Continue);
        let resp = rx.recv().await.unwrap();
        assert!(
            resp.contains("initialize") || resp.contains("NotReady") || resp.contains("-32002"),
            "unauth shutdown must NotReady, got {resp}"
        );
    }));
}

#[test]
fn empty_rpc_token_env_generates_and_does_not_mean_no_auth() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        std::env::remove_var("LOKAI_RPC_INSECURE");
        std::env::set_var("LOKAI_RPC_TOKEN", "   ");
        let (notifier, _rx) = channel_pair(512);
        let mut daemon = Daemon::new(notifier, None, None);
        let token = daemon.rpc_token().to_string();
        assert!(!token.trim().is_empty());
        let err = daemon
            .initialize(json!({
                "protocol_version": 1,
                "workspace_root": std::env::temp_dir().join("tetonicd-empty-token").display().to_string(),
                "rpc_token": ""
            }))
            .await
            .unwrap_err();
        assert_eq!(err.message, "invalid rpc_token");
        std::env::remove_var("LOKAI_RPC_TOKEN");
    }));
}

#[test]
fn initialize_result_rpc_token_is_none() {
    let src = include_str!("../handlers/initialize.rs");
    assert!(
        src.contains("rpc_token: None"),
        "initialize must not echo the live token"
    );
}

#[test]
fn strict_rpc_default_blocks_policy_set_and_fingerprint_allow() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(local.run_until(async {
        std::env::remove_var("LOKAI_STRICT_RPC");
        std::env::set_var("LOKAI_RPC_INSECURE", "1");
        let (mut daemon, mut rx, _dir) = daemon_with_mock(vec![]);
        let err = daemon
            .policy_set(json!({ "mode": "estate_stub" }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest.code());
        daemon
            .handle(
                Incoming {
                    jsonrpc: Some("2.0".into()),
                    id: Some(json!(2)),
                    method: methods::SECRET_FINGERPRINT_ALLOW.into(),
                    params: json!({ "fingerprint": "abc" }),
                },
                json!(2),
            )
            .await;
        let resp = rx.recv().await.unwrap();
        assert!(
            resp.contains("STRICT")
                || resp.contains("disabled")
                || resp.contains("InvalidRequest")
                || resp.contains("-32600"),
            "fingerprint allow must refuse when STRICT default on, got {resp}"
        );
        std::env::remove_var("LOKAI_RPC_INSECURE");
    }));
}
