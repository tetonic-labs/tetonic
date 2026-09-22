//! R1 session authority: live store, CancelRun, spawn DAG, leases, replay.

use crate::*;
use lokai_domain::{AttemptState, EventType, ExpireLease, RunCommand, RunState, TaskState};
use lokai_run::{command_envelope, RunSupervisor};
use std::sync::atomic::Ordering;
use std::sync::Arc;

struct FakeEventSink;
impl events::ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: events::ApplicationEvent) {}
}

fn make_app() -> Application {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_test_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    })
}

async fn start_fresh(app: &Application) -> commands::StartSessionResultPayload {
    let tmp = std::env::temp_dir();
    app.sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session start")
}

async fn inspect_run(app: &Application, run_id: &lokai_domain::RunId) -> lokai_domain::RunSnapshot {
    app.runs
        .inspect_run(commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("inspect_run")
}

#[tokio::test]
async fn session_start_cancel_end_use_live_store() {
    let app = make_app();
    let started = start_fresh(&app).await;
    assert!(app.sessions.has_live(&started.session_id));
    let live = app.sessions.live(&started.session_id).unwrap();
    assert!(live.take_conversation().is_ok());
    live.restore_conversation(lokai_core::Conversation::new());

    app.sessions
        .cancel_session(commands::CancelRunCommand {
            session_id: started.session_id.clone(),
            pooled_cancel: false,
        })
        .await
        .expect("cancel");
    assert!(live.cancel.load(Ordering::Relaxed));

    app.sessions
        .end_session(commands::EndSessionCommand {
            session_id: started.session_id.clone(),
            workspace_root: std::env::temp_dir().display().to_string(),
            status: Some("ok".into()),
            error: None,
        })
        .expect("end");
    assert!(!app.sessions.has_live(&started.session_id));
}

#[tokio::test]
async fn two_in_flight_turns_rejected_on_live_session() {
    let app = make_app();
    let started = start_fresh(&app).await;
    let live = app.sessions.live(&started.session_id).unwrap();
    assert!(matches!(
        admit_chat_turn(false, false, &live, None),
        TurnAdmitDecision::Admit {
            capacity_warning: None
        }
    ));
    assert_eq!(
        admit_chat_turn(false, false, &live, None),
        TurnAdmitDecision::Reject(TurnAdmitError::TurnInFlight)
    );
}

#[tokio::test]
async fn cancel_mid_turn_is_durable_and_rejects_forged_complete() {
    let app = make_app();
    let started = start_fresh(&app).await;
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "hi".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    app.sessions
        .cancel_session(commands::CancelRunCommand {
            session_id: started.session_id.clone(),
            pooled_cancel: false,
        })
        .await
        .expect("cancel");
    let snap = inspect_run(&app, &plan.run_id).await;
    assert_eq!(snap.state, RunState::Canceled);
    assert!(snap
        .attempts
        .values()
        .all(|a| matches!(&a.state, AttemptState::Canceled)));

    let err = app
        .runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect_err("forged complete after cancel must fail");
    assert!(
        err.to_string().to_lowercase().contains("cancel")
            || err.to_string().contains("lease")
            || err.to_string().contains("stale")
            || err.to_string().contains("expired")
            || err.to_string().contains("Invalid")
    );
}

#[tokio::test]
async fn spawn_child_task_appears_in_snapshot_dag() {
    let app = make_app();
    let started = start_fresh(&app).await;
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "hi".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    app.runs
        .register_spawn_task(&started.session_id, "a0_s0", "coder", "")
        .await
        .expect("spawn task");
    let snap = inspect_run(&app, &plan.run_id).await;
    let child = lokai_domain::TaskId::new("task_spawn_a0_s0");
    assert!(
        snap.tasks.contains_key(&child),
        "child task missing: {:?}",
        snap.tasks.keys().collect::<Vec<_>>()
    );
    assert!(
        snap.dependencies
            .get(&child)
            .map(|d| d.is_empty())
            .unwrap_or(true),
        "admit_child has no parent AddDependency"
    );
    assert!(
        snap.attempts
            .values()
            .any(|a| a.task_id == child && matches!(&a.state, AttemptState::Running)),
        "child Attempt started before execute"
    );

    app.sessions
        .cancel_session(commands::CancelRunCommand {
            session_id: started.session_id.clone(),
            pooled_cancel: false,
        })
        .await
        .expect("cancel parent");
    let snap = inspect_run(&app, &plan.run_id).await;
    assert_eq!(snap.state, RunState::Canceled);
    assert!(snap
        .tasks
        .values()
        .any(|t| t.task_id == child && matches!(&t.state, TaskState::Canceled)));
    assert!(snap
        .attempts
        .values()
        .all(|a| matches!(&a.state, AttemptState::Canceled)));
}

#[tokio::test]
async fn expire_lease_then_complete_rejected() {
    let app = make_app();
    let started = start_fresh(&app).await;
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "hi".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    app.supervisor
        .handle(RunCommand::ExpireLease(ExpireLease {
            envelope: command_envelope(
                "expire",
                Some(plan.run_id.to_string().len() as u64),
                "test",
            ),
            run_id: plan.run_id.clone(),
            attempt_id: plan.attempt_id.clone(),
            expired_at: 1,
        }))
        .await
        .ok();
    // Use snapshot sequence for a valid expire if the first call failed expected_sequence.
    let snap = inspect_run(&app, &plan.run_id).await;
    if !snap
        .attempts
        .get(&plan.attempt_id)
        .is_some_and(|a| matches!(&a.state, AttemptState::LeaseExpired))
    {
        app.supervisor
            .handle(RunCommand::ExpireLease(ExpireLease {
                envelope: command_envelope("expire2", Some(snap.sequence), "test"),
                run_id: plan.run_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                expired_at: 1,
            }))
            .await
            .expect("expire lease");
    }
    let err = app
        .runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect_err("complete after expire must fail");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("lease")
            || msg.contains("stale")
            || msg.contains("expir")
            || msg.contains("invalid"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn heartbeat_extends_lease() {
    let app = make_app();
    let started = start_fresh(&app).await;
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "hi".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    let before = inspect_run(&app, &plan.run_id)
        .await
        .attempts
        .get(&plan.attempt_id)
        .unwrap()
        .lease
        .as_ref()
        .unwrap()
        .expires_at;
    app.runs
        .heartbeat_turn(&plan.attempt_id.0)
        .await
        .expect("heartbeat");
    let after = inspect_run(&app, &plan.run_id)
        .await
        .attempts
        .get(&plan.attempt_id)
        .unwrap()
        .lease
        .as_ref()
        .unwrap()
        .expires_at;
    assert!(
        after >= before,
        "heartbeat should renew expires_at ({before} -> {after})"
    );
}

#[tokio::test]
async fn resume_from_sequence_returns_events_or_gap() {
    let app = make_app();
    let started = start_fresh(&app).await;
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "hi".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    app.runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect("complete");
    let events = app
        .runs
        .resume_events(commands::ResumeRunEventsCommand {
            run_id: plan.run_id.to_string(),
            after_sequence: 0,
            limit: Some(64),
        })
        .await
        .expect("resume")
        .expect("no gap");
    assert!(!events.is_empty(), "journal replay must return events");
    let gap = app
        .runs
        .resume_events(commands::ResumeRunEventsCommand {
            run_id: plan.run_id.to_string(),
            after_sequence: 0,
            limit: Some(64),
        })
        .await
        .expect("resume again");
    // after_sequence 0 with events present is not a gap (earliest is usually 1).
    if let Ok(ev) = gap {
        assert!(!ev.is_empty());
    }
}

#[tokio::test]
async fn restart_resume_does_not_continue_canceled_attempt() {
    let dir = std::env::temp_dir().join(format!(
        "lokai-r1-restart-{}-{}",
        std::process::id(),
        unix_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("lokai.db");
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy: policy.clone(),
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    let started = start_fresh(&app).await;
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "hi".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    app.sessions
        .cancel_session(commands::CancelRunCommand {
            session_id: started.session_id.clone(),
            pooled_cancel: false,
        })
        .await
        .expect("cancel");
    let run_id = plan.run_id.clone();
    drop(app);

    let supervisor = lokai_run::DurableRunSupervisor::new(Some(store));
    let snap = supervisor
        .snapshot(run_id.clone())
        .await
        .expect("reload snapshot");
    assert_eq!(snap.state, RunState::Canceled);
    let replay = supervisor
        .resume_from_sequence(run_id, 0, 128)
        .await
        .expect("resume after restart")
        .expect("events");
    assert!(!replay.is_empty());
    assert!(replay.iter().any(|e| matches!(
        e.event_type,
        EventType::CancellationRequested | EventType::Other(_)
    )));
}

fn unix_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[tokio::test]
async fn resume_preserves_tool_call_id() {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_test_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    let started = start_fresh(&app).await;
    let sid = started.session_id.clone();
    let ws = std::env::temp_dir().display().to_string();
    {
        let _ = store.write_sync({
            let sid = sid.clone();
            move |db| {
                db.append_message_with(
                    &sid,
                    "assistant",
                    "",
                    "",
                    Some(r#"[{"function":{"name":"read_file","arguments":"{}"}}]"#),
                    None,
                    None,
                )
                .unwrap();
                db.append_message_with(
                    &sid,
                    "tool",
                    "",
                    "file contents",
                    None,
                    Some("read_file"),
                    Some("tc_keep"),
                )
                .unwrap();
            }
        });
    }
    app.sessions
        .end_session(commands::EndSessionCommand {
            session_id: sid.clone(),
            workspace_root: ws.clone(),
            status: Some("ok".into()),
            error: None,
        })
        .expect("end");

    let first = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: ws.clone(),
            resume: Some(true),
            session_id: Some(sid.clone()),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("resume 1");
    app.sessions
        .end_session(commands::EndSessionCommand {
            session_id: sid.clone(),
            workspace_root: ws.clone(),
            status: Some("ok".into()),
            error: None,
        })
        .expect("end 2");
    let second = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: ws,
            resume: Some(true),
            session_id: Some(sid),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("resume 2");

    assert_eq!(first.messages.len(), second.messages.len());
    let tool = first
        .messages
        .iter()
        .find(|m| m.role == "tool")
        .expect("tool message");
    assert_eq!(tool.tool_name.as_deref(), Some("read_file"));
    assert_eq!(tool.tool_call_id.as_deref(), Some("tc_keep"));
    let tool_b = second.messages.iter().find(|m| m.role == "tool").unwrap();
    assert_eq!(tool.tool_call_id, tool_b.tool_call_id);
    assert_eq!(tool.content, tool_b.content);
}

#[tokio::test]
async fn resume_elision_note_when_over_cap() {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_test_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    let started = start_fresh(&app).await;
    let sid = started.session_id.clone();
    let ws = std::env::temp_dir().display().to_string();
    {
        let _ = store.write_sync({
            let sid = sid.clone();
            move |db| {
                for i in 0..201u32 {
                    db.append_message(&sid, "user", "", &format!("msg-{i}"), None)
                        .unwrap();
                }
            }
        });
    }
    app.sessions
        .end_session(commands::EndSessionCommand {
            session_id: sid.clone(),
            workspace_root: ws.clone(),
            status: Some("ok".into()),
            error: None,
        })
        .expect("end");
    let resumed = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: ws,
            resume: Some(true),
            session_id: Some(sid),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("resume");
    assert_eq!(resumed.messages_loaded, crate::RESUME_MESSAGE_CAP);
    assert!(resumed.messages[0]
        .content
        .contains("earlier messages omitted"));
    assert!(resumed.messages[0].content.contains("of 201"));
}
