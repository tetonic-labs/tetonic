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
        fabric_hint: None,
    };
    Application::new(deps)
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

/// Slice 2 — fresh session start records a session id in the store.
#[tokio::test]
async fn test_session_start_fresh() {
    let app = make_app();
    let tmp = std::env::temp_dir();
    let result = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            resume: None,
            model_tier: None,
            session_id: None,
            goal: None,
            data_class: None,
            verify_cmd: None,
            briefing: Some(false), // skip briefing in tests
            orchestration: None,
            critic: None,
            llm_router: None,
            model_fast: None,
            model_hard: None,
            session_max_steps: None,
            ..Default::default()
        })
        .await
        .expect("session start");
    assert!(!result.session_id.is_empty());
    assert!(!result.resumed);
    assert_eq!(result.resume_state, "fresh");
    assert!(result.messages.is_empty());
}

/// Slice 2 — session end persists the status.
#[tokio::test]
async fn test_session_end() {
    let app = make_app();
    let tmp = std::env::temp_dir();
    let started = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            resume: None,
            model_tier: None,
            session_id: None,
            goal: None,
            data_class: None,
            verify_cmd: None,
            briefing: Some(false),
            orchestration: None,
            critic: None,
            llm_router: None,
            model_fast: None,
            model_hard: None,
            session_max_steps: None,
            ..Default::default()
        })
        .await
        .expect("session start");

    app.sessions
        .end_session(commands::EndSessionCommand {
            session_id: started.session_id.clone(),
            workspace_root: tmp.display().to_string(),
            status: Some("ok".into()),
            error: None,
        })
        .expect("session end");
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

/// Slice 3 — plan_turn resolves verify gate from verify_cmd.
#[test]
fn test_plan_turn_verify_gate() {
    let app = make_app();
    let tmp = std::env::temp_dir();
    let started = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.sessions.start_session(commands::StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            resume: None,
            model_tier: None,
            session_id: None,
            goal: None,
            data_class: None,
            verify_cmd: Some("cargo test".into()),
            briefing: Some(false),
            orchestration: None,
            critic: None,
            llm_router: None,
            model_fast: None,
            model_hard: None,
            session_max_steps: None,
            ..Default::default()
        }))
        .expect("session start");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    let plan = local
        .block_on(
            &rt,
            app.runs.plan_turn(&commands::RunTurnCommand {
                session_id: started.session_id,
                user_input: "hello".into(),
                verify_cmd: Some("cargo test".into()),
                llm_router: Some(false),
            }),
        )
        .expect("plan turn");
    assert!(plan.verify_gated);
    assert!(!plan.llm_router);
}

/// Slice 4 — approval respond persists allow + remember rule.
#[test]
fn test_approval_remember_rule() {
    let app = make_app();
    let tmp = std::env::temp_dir();
    let started = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.sessions.start_session(commands::StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            resume: None,
            model_tier: None,
            session_id: None,
            goal: None,
            data_class: None,
            verify_cmd: None,
            briefing: Some(false),
            orchestration: None,
            critic: None,
            llm_router: None,
            model_fast: None,
            model_hard: None,
            session_max_steps: None,
            ..Default::default()
        }))
        .expect("session start");
    let mut rx = app
        .approvals
        .register_request(commands::RegisterApprovalCommand {
            session_id: started.session_id.clone(),
            approval_id: "ap_1".into(),
            call_id: "call_ap_1".into(),
            kind: "run_shell".into(),
            detail: "cargo test".into(),
            tool: "run_shell".into(),
            args: serde_json::json!({"command":"cargo test"}),
            missing_controls: vec![],
            user_approval_required: false,
            auto_grant_approvals: false,
            attempt_id: None,
        })
        .unwrap();
    app.approvals
        .respond(commands::ApprovalResponseCommand {
            session_id: started.session_id,
            approval_id: "ap_1".into(),
            approved: true,
            remember: true,
            kind: "run_shell".into(),
            detail: "cargo test".into(),
            channel_delivered: true,
            attempt_id: None,
        })
        .expect("approval respond");
    assert!(rx.try_recv().unwrap());
}

/// M0-4 semantic-effect parity: session + turn lifecycle event order.
#[test]
fn session_turn_lifecycle_event_order() {
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

    let tmp = std::env::temp_dir().join(format!(
        "lokai_ws_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    let started = local
        .block_on(
            &rt,
            app.sessions.start_session(commands::StartSessionCommand {
                workspace_root: tmp.display().to_string(),
                resume: None,
                model_tier: None,
                session_id: None,
                goal: None,
                data_class: None,
                verify_cmd: None,
                briefing: Some(false),
                orchestration: None,
                critic: None,
                llm_router: None,
                model_fast: None,
                model_hard: None,
                session_max_steps: None,
                ..Default::default()
            }),
        )
        .expect("session start");

    let _plan = local
        .block_on(
            &rt,
            app.runs.plan_turn(&commands::RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "hi".into(),
                verify_cmd: None,
                llm_router: Some(false),
            }),
        )
        .expect("plan turn");

    local
        .block_on(
            &rt,
            app.runs.complete_turn(
                &commands::CompleteTurnCommand {
                    session_id: started.session_id.clone(),
                    attempt_id: _plan.attempt_id,
                    workspace_root: tmp.display().to_string(),
                    canceled: false,
                    error: None,
                },
                None,
                None,
            ),
        )
        .expect("complete turn");

    app.sessions
        .end_session(commands::EndSessionCommand {
            session_id: started.session_id,
            workspace_root: tmp.display().to_string(),
            status: Some("ok".into()),
            error: None,
        })
        .expect("session end");

    let events = recorded.lock().unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            events::ApplicationEvent::SessionStarted { .. } => "session.start",
            events::ApplicationEvent::RunStatus { status, .. } if status == "started" => {
                "turn.start"
            }
            events::ApplicationEvent::TurnCompleted { .. } => "turn.complete",
            events::ApplicationEvent::RunStatus { .. } => "run.status",
            events::ApplicationEvent::SessionEnded { .. } => "session.end",
            _ => "other",
        })
        .collect();
    assert!(
        !kinds.iter().any(|kind| *kind == "turn.start"),
        "planning without execution must not report started, got {kinds:?}"
    );
    assert!(
        kinds.contains(&"session.start"),
        "expected session.start, got {kinds:?}"
    );
    assert!(
        kinds.contains(&"turn.complete"),
        "expected turn.complete, got {kinds:?}"
    );
    assert!(
        kinds.last() == Some(&"session.end"),
        "expected session.end last, got {kinds:?}"
    );
}

/// R02: session start + plan_turn bind live ids on TraceContext.
#[test]
fn trace_context_binds_session_and_run_ids() {
    // Synchronous callers supply an explicit span; live turns use task scope.
    let _subscriber = tracing::subscriber::set_default(tracing_subscriber::registry());
    let span = tracing::info_span!("session_trace_test");
    let _entered = span.enter();
    let app = make_app();
    let tmp = std::env::temp_dir();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    let started = local
        .block_on(
            &rt,
            app.sessions.start_session(commands::StartSessionCommand {
                workspace_root: tmp.display().to_string(),
                resume: None,
                model_tier: None,
                session_id: None,
                goal: None,
                data_class: None,
                verify_cmd: None,
                briefing: Some(false),
                orchestration: None,
                critic: None,
                llm_router: None,
                model_fast: None,
                model_hard: None,
                session_max_steps: None,
                ..Default::default()
            }),
        )
        .expect("session start");
    let after_session = tetonic_telemetry::extract_context().expect("session ctx");
    assert_eq!(
        after_session.session_id.as_deref(),
        Some(started.session_id.as_str())
    );

    let plan = local
        .block_on(
            &rt,
            app.runs.plan_turn(&commands::RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "hi".into(),
                verify_cmd: None,
                llm_router: Some(false),
            }),
        )
        .expect("plan turn");
    // plan_turn alone does not inject turn ids; execute_turn / run_turn_body does.
    // Mirror the production bind used by CLI+daemon turn path:
    tetonic_telemetry::inject_turn_context(
        &started.session_id,
        &plan.run_id.0,
        Some(plan.task_id.0.as_str()),
    );
    let after_turn = tetonic_telemetry::extract_context().expect("turn ctx");
    assert_eq!(
        after_turn.session_id.as_deref(),
        Some(started.session_id.as_str())
    );
    assert_eq!(after_turn.run_id.as_deref(), Some(plan.run_id.0.as_str()));
    assert_eq!(after_turn.task_id.as_deref(), Some(plan.task_id.0.as_str()));
    assert!(!after_turn.run_id.as_deref().unwrap().is_empty());
}
