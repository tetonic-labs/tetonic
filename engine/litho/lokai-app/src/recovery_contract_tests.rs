//! H3-3 recovery matrix: kill-and-reopen the same `lokai.db`.

use crate::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct FakeEventSink;
impl events::ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: events::ApplicationEvent) {}
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lokai-h33-{}-{}",
        std::process::id(),
        lokai_memory::new_id("h33")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_app(dir: &Path) -> (Application, lokai_memory::SharedStore) {
    let db_path = dir.join("lokai.db");
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifacts = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            dir.join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifacts);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    (app, store)
}

async fn start_ws(app: &Application, ws: &Path) -> commands::StartSessionResultPayload {
    app.sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session")
}

async fn resume_ws(
    app: &Application,
    ws: &Path,
    session_id: Option<&str>,
) -> commands::StartSessionResultPayload {
    app.sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: ws.display().to_string(),
            resume: Some(true),
            session_id: session_id.map(str::to_string),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("resume")
}

#[tokio::test]
async fn conversation_is_restored_on_reopen() {
    let dir = temp_dir();
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let (app, store) = make_app(&dir);
    let started = start_ws(&app, &ws).await;
    {
        let _ = store.write_sync({
            let session_id = started.session_id.clone();
            move |db| {
                db.append_message(&session_id, "user", "a0", "hello from user", None)
                    .unwrap();
            }
        });
    }
    let sid = started.session_id.clone();
    drop(app);
    drop(store);

    let (app2, _) = make_app(&dir);
    assert!(!app2.sessions.has_live(&sid));
    let resumed = resume_ws(&app2, &ws, Some(&sid)).await;
    assert_eq!(resumed.session_id, sid);
    assert!(resumed
        .messages
        .iter()
        .any(|m| m.content.contains("hello from user")));
}

#[tokio::test]
async fn granted_approval_is_restored_on_reopen() {
    let dir = temp_dir();
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let (app, store) = make_app(&dir);
    let started = start_ws(&app, &ws).await;
    {
        let _ = store.write_sync({
            let session_id = started.session_id.clone();
            move |db| {
                db.record_approval(
                    "appr_1",
                    &session_id,
                    "run_shell",
                    "echo hi",
                    "granted",
                    true,
                )
                .unwrap();
                db.add_approval_rule("run_shell", "echo hi").unwrap();
            }
        });
    }
    drop(app);
    drop(store);

    let (_, store2) = make_app(&dir);
    let row = store2
        .read_sync(|db| {
            db.get_approval("appr_1")
                .unwrap()
                .expect("durable approval")
        })
        .unwrap();
    assert_eq!(row.decision, "granted");
    let matches = store2
        .read_sync(|db| db.approval_rule_matches("run_shell", "echo hi").unwrap())
        .unwrap();
    assert!(matches);
}

#[tokio::test]
async fn pending_live_approval_is_lost_and_resume_is_recovery_required() {
    let dir = temp_dir();
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let (app, store) = make_app(&dir);
    let started = start_ws(&app, &ws).await;
    let sid = started.session_id.clone();
    {
        let _ = store.write_sync({
            let sid = sid.clone();
            move |db| {
                db.upsert_turn_operation(
                    &sid,
                    "turn_1",
                    "awaiting_approval",
                    r#"{"tool":"run_shell"}"#,
                )
                .unwrap();
            }
        });
    }
    let live = app.sessions.live(&sid).expect("live");
    let _ = live;
    let _rx = app
        .approvals
        .register_request(commands::RegisterApprovalCommand {
            session_id: sid.clone(),
            approval_id: "stale_approval".into(),
            call_id: "c1".into(),
            kind: "run_shell".into(),
            detail: "echo hi".into(),
            tool: "run_shell".into(),
            args: serde_json::json!({}),
            missing_controls: vec![],
            user_approval_required: true,
            auto_grant_approvals: false,
            attempt_id: None,
        })
        .expect("park");
    drop(app);
    drop(store);

    let (app2, _) = make_app(&dir);
    assert!(!app2.sessions.has_live(&sid));
    let resumed = resume_ws(&app2, &ws, Some(&sid)).await;
    assert_eq!(resumed.resume_state, "recovery_required");
    let delivered = app2
        .approvals
        .respond(commands::ApprovalResponseCommand {
            session_id: sid,
            approval_id: "stale_approval".into(),
            approved: true,
            remember: false,
            kind: "run_shell".into(),
            detail: "echo hi".into(),
            channel_delivered: false,
            attempt_id: None,
        })
        .expect_err("unknown pre-restart approval must be rejected");
    assert!(delivered.to_string().contains("stale or unknown"));
}

#[tokio::test]
async fn in_flight_run_journal_is_restored() {
    let dir = temp_dir();
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let (app, store) = make_app(&dir);
    let started = start_ws(&app, &ws).await;
    let sid = started.session_id.clone();
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: sid.clone(),
            user_input: "do the work".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    let run_id = plan.run_id.clone();
    {
        let _ = store.write_sync({
            let sid = sid.clone();
            move |db| {
                db.upsert_turn_operation(&sid, "turn_1", "executing", "{}")
                    .unwrap();
            }
        });
    }
    drop(app);
    drop(store);

    let (app2, _) = make_app(&dir);
    let snap = app2
        .runs
        .inspect_run(crate::commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("restored journal");
    assert!(!snap.attempts.is_empty());
    let resumed = resume_ws(&app2, &ws, Some(&sid)).await;
    assert_eq!(resumed.resume_state, "recovery_required");
}

#[tokio::test]
async fn live_session_maps_do_not_survive_reopen() {
    let dir = temp_dir();
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let (app, store) = make_app(&dir);
    let started = start_ws(&app, &ws).await;
    let sid = started.session_id.clone();
    assert!(app.sessions.has_live(&sid));
    drop(app);
    drop(store);
    let (app2, _) = make_app(&dir);
    assert!(!app2.sessions.has_live(&sid));
    assert_eq!(app2.sessions.live_count(), 0);
}
