//! M10 inspect / cancel / subscribe door tests (S-1).

use crate::*;
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
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = tetonic_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    })
}

/// S-1(e): create a Run on SharedStore without start_session; persist NULL.
#[tokio::test]
async fn create_run_without_session_persists_null_session_id() {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_m8_create_run_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = tetonic_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy: policy.clone(),
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });

    let run_id = app
        .runs
        .create_run(commands::CreateRunCommand {
            session_id: None,
            root_task_id: None,
            ..Default::default()
        })
        .await
        .expect("create_run");
    let snap = app
        .runs
        .inspect_run(commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("snapshot");
    assert!(
        snap.session_id.is_none(),
        "create_run must not mint a dummy SessionId"
    );
    let spec = snap
        .job_spec
        .as_ref()
        .expect("inspect create_run must name a JobSpec");
    assert_eq!(
        spec.identity_id.0,
        crate::definition::CODING_IDENTITY_ID,
        "inspect door names the coding identity, not a minted session"
    );
    assert_eq!(spec.input_digest, tetonic_run::job_input_digest(""));
    let col = store
        .read({
            let id = run_id.0.clone();
            move |db| db.load_run_projection_session_id(&id)
        })
        .await
        .expect("read")
        .expect("projection row");
    assert!(col.is_none(), "persisted session_id must be SQL NULL");
}

/// S-1(b): cancel_run does not require a live Session.
#[tokio::test]
async fn cancel_run_without_session_cancels() {
    let app = make_app();
    let run_id = app
        .runs
        .create_run(commands::CreateRunCommand {
            session_id: None,
            root_task_id: None,
            ..Default::default()
        })
        .await
        .expect("create_run");
    app.runs
        .cancel_run(commands::CancelByRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("cancel_run");
    let snap = app
        .runs
        .inspect_run(commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("inspect");
    assert_eq!(snap.state, tetonic_domain::RunState::Canceled);
    assert!(snap.session_id.is_none());
}

/// S-1(c): Application::event_sink send is observed by the bootstrap sink.
#[test]
fn event_sink_send_is_observed() {
    let (recorder, recorded) = events::RecordingEventSink::new();
    let db_path = std::env::temp_dir().join(format!(
        "lokai_test_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = tetonic_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy: policy.clone(),
        event_sink: recorder,
        index_db: None,
        fabric_hint: None,
    });
    app.event_sink()
        .send(events::ApplicationEvent::InspectorClear);
    let kinds = recorded.lock().unwrap();
    assert!(
        kinds
            .iter()
            .any(|e| matches!(e, events::ApplicationEvent::InspectorClear)),
        "event_sink send must be observed by the bootstrap sink"
    );
}
