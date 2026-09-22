//! AUD-01 app-door characterization (public APIs only).

use std::sync::Arc;

use tetonic_app::coding_pack::CodingPack;
use tetonic_app::commands::{
    CompleteTurnCommand, InspectRunCommand, RunTurnCommand, StartSessionCommand,
};
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_artifact::LocalArtifactStore;
use tetonic_domain::{AttemptState, RunState};
use tetonic_orchestrator::{RoleId, SpecialistPack};

struct FakeEventSink;
impl ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: ApplicationEvent) {}
}

fn make_app_with_store(dir: &std::path::Path) -> (Application, tetonic_memory::SharedStore) {
    std::fs::create_dir_all(dir).unwrap();
    let db_path = dir.join("lokai.db");
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifacts = Arc::new(
        LocalArtifactStore::new(
            dir.join("artifacts"),
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = tetonic_runtime::EngineRuntime::new(policy.clone(), None, artifacts);
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

fn turn_src() -> &'static str {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/turn_execution.rs"
    ))
}

#[test]
fn aud01_bh_resp_explain_turn_app_pack() {
    let pack = CodingPack;
    assert!(pack.explain_turn(&RoleId::new("planner"), false));
    assert!(pack.explain_turn(&RoleId::new("reviewer"), false));
    assert!(pack.explain_turn(&RoleId::new("critic"), false));
    assert!(!pack.explain_turn(&RoleId::new("coder"), false));
}

#[test]
fn aud01_bh_invest_app_pack_read_subset() {
    let tools = CodingPack
        .allowed_tools(&RoleId::new("planner"))
        .expect("read subset");
    assert!(tools.contains(&"search_code".into()));
    assert!(tools.contains(&"read_file".into()));
    assert!(!tools.contains(&"edit_file".into()));
}

#[test]
fn aud01_bh_plan_app_pack_no_edit() {
    let overlay = CodingPack.overlay(&RoleId::new("planner"));
    assert!(overlay.contains("Do NOT edit"));
    let tools = CodingPack.allowed_tools(&RoleId::new("planner")).unwrap();
    assert!(!tools.contains(&"run_shell".into()));
    assert!(!tools.contains(&"write_file".into()));
}

#[test]
fn aud01_bh_edit_mode_exists_without_commit_contract() {
    // PRESERVE mode only. Commit-before-winner is BH-FIN-COMMIT DEFECT, not this row.
    assert!(CodingPack.allowed_tools(&RoleId::new("coder")).is_none());
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/definition.rs"));
    assert!(src.contains("edit_file"));
    assert!(!src.contains("commit_staged_if_any"));
}

#[test]
fn aud01_bh_debug_execute_spawn_door() {
    // AUD-PATH-004. Distinct from in-loop orchestration_in_loop_spawn_agent.
    let src = turn_src();
    assert!(src.contains("async fn execute_spawn"));
    assert!(src.contains("register_spawn_task"));
    assert!(src.contains("run_spawned_specialist"));
    assert!(!src.contains("orchestration_in_loop_spawn_agent"));
    assert!(CodingPack.parse("debugger").is_some());
}

#[test]
fn aud01_bh_orch_spawn_execute_spawn_door() {
    // PRESERVE workflow; executor owner DEFECT (WORK-02).
    let src = turn_src();
    assert!(src.contains("async fn execute_spawn"));
}

#[test]
fn aud01_bh_stream_model_token_shape() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/events.rs"));
    assert!(src.contains("ModelToken {"));
    assert!(src.contains("session_id: String"));
    assert!(src.contains("agent_id: String"));
}

#[tokio::test]
async fn aud01_bh_recover_no_future_resume() {
    let dir = std::env::temp_dir().join(tetonic_memory::new_id("aud01_rec"));
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let (app, store) = make_app_with_store(&dir);
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session");
    {
        let session_id = started.session_id.clone();
        let _ = store.write_sync(move |db| {
            db.append_message(&session_id, "user", "a0", "hello from user", None)
                .unwrap();
        });
    }
    let sid = started.session_id.clone();
    drop(app);
    drop(store);

    let (app2, _) = make_app_with_store(&dir);
    assert!(
        !app2.sessions.has_live(&sid),
        "in-flight loop/future must not resume; journals reload only"
    );
    let resumed = app2
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            resume: Some(true),
            session_id: Some(sid.clone()),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("resume");
    assert_eq!(resumed.session_id, sid);
    assert!(resumed
        .messages
        .iter()
        .any(|m| m.content.contains("hello from user")));
}

#[tokio::test]
async fn aud01_defect_bh_fin_err_current() {
    // baseline_defect: not desired; WORK-FIN-02 inverts. Passing this does not
    // establish INV-V4-FIN-*.
    let dir = std::env::temp_dir().join(tetonic_memory::new_id("aud01_finerr"));
    let (app, mem) = make_app_with_store(&dir);
    let tmp = std::env::temp_dir();
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session");
    let plan = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "do the work".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    {
        let session_id = started.session_id.clone();
        let _ = mem.write_sync(move |db| {
            db.append_message(&session_id, "assistant", "", "boom path", None)
                .unwrap();
        });
    }
    app.runs
        .complete_turn(
            &CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: tmp.display().to_string(),
                canceled: false,
                error: Some("boom".into()),
            },
            None,
            None,
        )
        .await
        .expect("complete with error");
    let snap = app
        .runs
        .inspect_run(InspectRunCommand {
            run_id: plan.run_id.to_string(),
        })
        .await
        .expect("inspect");
    assert_eq!(snap.state, RunState::Failed);
    assert!(
        snap.attempts
            .values()
            .any(|a| matches!(a.state, AttemptState::Failed)),
        "pre-effect Failed is FailAttempt then FinishRun(Failed): {snap:?}"
    );
}

#[test]
fn aud01_defect_bh_appr_loss_waiter_is_process_local() {
    // DEFECT pin of process-local waiter (oneshot). Fail-closed expires PORTAL-01 VERIFY.
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/approval.rs"));
    assert!(src.contains("struct ParkedWait"));
    assert!(src.contains("oneshot::Sender<bool>"));
    assert!(src.contains("parked: Mutex<HashMap<String, ParkedWait>>"));
}
