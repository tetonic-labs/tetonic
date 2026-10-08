use super::*;
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
    let deps = ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy: policy.clone(),
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
    };
    Application::new(deps)
        .with_execution_policy(Arc::new(crate::definition::validate_coding_execution))
}

/// Slice 1 — policy/get returns coherent defaults.
#[tokio::test]
async fn test_policy_get_default() {
    let app = make_app();
    let result = app
        .policies
        .get_policy(commands::GetPolicyCommand {
            workspace_root: std::env::temp_dir().display().to_string(),
        })
        .await
        .expect("policy get");
    // Default engine is in full mode
    assert!(!result.mode.is_empty());
    assert!(!result.default_data_class.is_empty());
}

/// Slice 5 — policy/set roundtrip persists and reads back.
#[tokio::test]
async fn test_policy_set_roundtrip() {
    let app = make_app();
    app.policies
        .set_policy(commands::SetPolicyCommand {
            mode: Some("estate_stub".into()),
            verify_allowed: None,
            mutations_allowed: None,
            allow_sensitive_to_owner_estate: None,
            allow_repository_to_admin_managed: None,
        })
        .await
        .expect("policy set");
    let result = app
        .policies
        .get_policy(commands::GetPolicyCommand {
            workspace_root: std::env::temp_dir().display().to_string(),
        })
        .await
        .expect("policy get after set");
    assert_eq!(result.mode, "estate_stub");
}

/// Slice 5 — policy/set with no fields returns an application error (not a panic).
#[tokio::test]
async fn test_policy_set_no_fields_errors() {
    let app = make_app();
    let err = app
        .policies
        .set_policy(commands::SetPolicyCommand {
            mode: None,
            verify_allowed: None,
            mutations_allowed: None,
            allow_sensitive_to_owner_estate: None,
            allow_repository_to_admin_managed: None,
        })
        .await
        .expect_err("must fail with no fields");
    assert!(matches!(err, errors::AppError::InvalidRequest(_)));
}

/// Slice 6 — capacity begin_optimize rejects busy sessions.
#[test]
fn test_capacity_begin_optimize_busy() {
    let app = make_app();
    let err = app
        .capacity
        .begin_optimize(commands::BeginOptimizeCommand {
            sessions_busy: true,
            capacity_busy: false,
            depth: "quick".into(),
            auto_apply: false,
        })
        .expect_err("must reject busy sessions");
    assert!(matches!(err, errors::AppError::InvalidRequest(_)));
}

#[test]
fn test_capacity_cancel_optimize() {
    let app = make_app();
    assert!(!app
        .capacity
        .cancel_optimize(commands::CancelOptimizeCommand {
            requested_job_id: None,
            active_job_id: None,
            capacity_busy: false,
        })
        .expect("cancel when idle"));
    assert!(!app
        .capacity
        .cancel_optimize(commands::CancelOptimizeCommand {
            requested_job_id: Some("job_a".into()),
            active_job_id: Some("job_b".into()),
            capacity_busy: true,
        })
        .expect("cancel mismatch"));
}
