//! CODE-01 same-crate compile-path pins. Does not expand `build_agent` visibility.

use super::*;
use crate::definition::CodingAgentDefinition;
use tetonic_domain::{DataClass, DisclosureTier};
use tetonic_orchestrator::{AgentBuildRequest, RoleId, SessionStartPlan};

#[test]
fn git_diff_omits_a_protected_store() {
    let output = "\
stdout:
diff --git a/src/a.rs b/src/a.rs
+hello
diff --git a/lokai.db b/lokai.db
+PRIVATECANARY
 M src/a.rs
";
    let cleaned = without_reserved_git_output(
        output,
        &[std::path::PathBuf::from("/tmp/ws/lokai.db")],
        std::path::Path::new("/tmp/ws"),
    );
    assert!(cleaned.contains("hello"), "{cleaned}");
    assert!(!cleaned.contains("PRIVATECANARY"), "{cleaned}");
    let status = " M src/a.rs\n M lokai.db\n";
    let cleaned = without_reserved_git_output(
        status,
        &[std::path::PathBuf::from("/tmp/ws/lokai.db")],
        std::path::Path::new("/tmp/ws"),
    );
    assert!(cleaned.contains("src/a.rs"), "{cleaned}");
    assert!(!cleaned.contains("lokai.db"), "{cleaned}");
}

#[test]
fn git_diff_omits_a_sqlite_database_that_is_not_the_reserved_store() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = b"SQLite format 3\0".to_vec();
    bytes.extend_from_slice(b"PRIVATECANARY");
    std::fs::write(dir.path().join("notes.txt"), bytes).unwrap();
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
+hello
diff --git a/notes.txt b/notes.txt
+PRIVATECANARY
";
    let cleaned = without_reserved_git_output(diff, &[], dir.path());
    assert!(cleaned.contains("hello"), "{cleaned}");
    assert!(!cleaned.contains("PRIVATECANARY"), "{cleaned}");
    assert!(!cleaned.contains("notes.txt"), "{cleaned}");
    let status = " M src/a.rs\n M notes.txt\n";
    let cleaned = without_reserved_git_output(status, &[], dir.path());
    assert!(cleaned.contains("src/a.rs"), "{cleaned}");
    assert!(!cleaned.contains("notes.txt"), "{cleaned}");
}

#[tokio::test]
async fn classifier_stops_when_the_attempt_is_canceled() {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered_flag = entered.clone();
    let cancel_flag = cancel.clone();
    let task = tokio::spawn(async move {
        stop_or(
            &cancel_flag,
            || false,
            || async { false },
            async {
                entered_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                std::future::pending::<u8>().await
            },
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !entered.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("classifier did not start");
    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .expect("classifier did not stop")
        .unwrap();
    assert!(stopped.is_none());
}

#[tokio::test]
async fn classifier_stops_when_authority_is_revoked() {
    let revoked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered_flag = entered.clone();
    let revoked_flag = revoked.clone();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let task = tokio::spawn(async move {
        stop_or(
            &cancel,
            || false,
            || {
                let revoked_flag = revoked_flag.clone();
                async move { revoked_flag.load(std::sync::atomic::Ordering::SeqCst) }
            },
            async {
                entered_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                std::future::pending::<u8>().await
            },
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !entered.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("classifier did not start");
    revoked.store(true, std::sync::atomic::Ordering::SeqCst);
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .expect("classifier did not stop")
        .unwrap();
    assert!(stopped.is_none());
}

#[test]
fn classifier_keeps_the_session_data_class() {
    let mut host = compile_host(None, false);
    host.plan.data_class = DataClass::Secret;
    let fabric = session_classifier_fabric(&host.plan, "sess", "run", "task", "attempt");
    assert_eq!(fabric.data_class, DataClass::Secret);
    assert_eq!(fabric.disclosure_tier, host.plan.disclosure_tier);
    assert_eq!(fabric.session_id.as_deref(), Some("sess"));
    assert_eq!(fabric.run_id.as_deref(), Some("run"));
    assert_ne!(fabric.data_class, DataClass::default());
}

fn compile_host(verify_cmd: Option<&str>, force_explain: bool) -> TurnExecutionHost {
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = tetonic_runtime::EngineRuntime::new(policy, None, artifact_store);
    TurnExecutionHost {
        live_session: None,
        runtime: Arc::new(runtime),
        provider: dummy_provider(),
        store: None,
        index_db: None,
        workspace_root: std::env::temp_dir().display().to_string(),
        tool_workspace: std::env::temp_dir(),
        model_fast: "fast-model".into(),
        model_hard: "hard-model".into(),
        num_ctx: 2048,
        session_max_steps: 16,
        explicit_hard_tier: false,
        orchestration: OrchestrationMode::Single,
        critic_enabled: false,
        llm_router: false,
        plan: SessionStartPlan {
            data_class: DataClass::default(),
            disclosure_tier: DisclosureTier::default(),
            briefing: Some("session briefing".into()),
            project_context: Some("project notes".into()),
            verify_cmd: verify_cmd.map(str::to_string),
        },
        session_id: "sess-code01".into(),
        allow_shell: false,
        force_explain,
        spawn_limits: SpawnLimits::default(),
        tokenizer: Arc::new(tetonic_core::HeuristicTokenizer),
        cancel: Arc::new(AtomicBool::new(false)),
        approvals: Arc::new(crate::approval::DefaultApprovalService::new(
            None,
            Arc::new(NoopSink),
        )),
        audit_factory: None,
        spawn_track: None,
        turn_spawn_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        compute_broker: None,
        secret_scanner: None,
        redaction_sink: None,
        auto_grant_approvals: false,
    }
}

struct NoopSink;
impl ApplicationEventSink for NoopSink {
    fn send(&self, _event: ApplicationEvent) {}
}

fn dummy_provider() -> Arc<dyn tetonic_inference::InferenceProvider> {
    Arc::new(NoopProvider)
}

struct NoopProvider;
#[async_trait::async_trait]
impl tetonic_inference::InferenceProvider for NoopProvider {
    async fn chat(
        &self,
        _req: tetonic_inference::ChatRequest,
        _on_token: &mut tetonic_inference::TokenSink<'_>,
    ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
        Err(tetonic_inference::InferenceError::Provider(
            "code01 test provider".into(),
        ))
    }
}

fn role_build(role: &str, spawned: bool) -> AgentBuildRequest {
    AgentBuildRequest {
        agent_id: "a0".into(),
        role: Some(RoleId::new(role)),
        dynamic_spec: None,
        use_hard_model: false,
        orchestration_tools: false,
        max_steps: None,
        explain_turn: false,
        spawned,
        inherited_workspace_version: None,
    }
}

#[test]
fn code01_build_agent_compiles_from_definition() {
    let host = compile_host(None, false);
    let def = CodingAgentDefinition::production();
    let mut base_build = role_build("coder", false);
    base_build.role = None;
    let base = compile_build_agent_config(&host, None, &base_build);
    for role_name in ["planner", "coder", "reviewer", "debugger"] {
        let build = role_build(role_name, false);
        let compiled = compile_build_agent_config(&host, None, &build);
        let role = RoleId::new(role_name);
        let expected = def.apply_role(&base, &role, "a0");
        assert_eq!(compiled.system_overlay, expected.system_overlay);
        assert_eq!(compiled.specialist_role, expected.specialist_role);
        assert_eq!(compiled.explain_turn, expected.explain_turn);
        assert_eq!(compiled.max_steps, expected.max_steps);
        assert_eq!(
            compiled.system_overlay.as_deref(),
            Some(def.overlay(&role).as_str())
        );
        assert_eq!(CodingPack.allowed_tools(&role), def.allowed_tools(&role));
        assert_eq!(
            CodingPack.spawn_allowed_tools(&role),
            def.spawn_allowed_tools(&role)
        );
        let spawned_cfg = compile_build_agent_config(&host, None, &role_build(role_name, true));
        assert_eq!(spawned_cfg.system_overlay, compiled.system_overlay);
    }
}

#[test]
fn code01_execute_spawn_parses_from_production() {
    let def = CodingAgentDefinition::production();
    for alias in [
        "planner", "plan", "coder", "code", "debugger", "debug", "reviewer", "review", "critic",
    ] {
        assert_eq!(CodingPack.parse(alias), def.parse(alias));
        assert!(parse_spawn_role(alias).is_ok(), "alias {alias}");
    }
    assert_eq!(CodingPack.parse("nope"), def.parse("nope"));
    match parse_spawn_role("nope") {
        Err(AppError::InvalidRequest(msg)) => assert!(msg.contains("unknown role")),
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
}

#[test]
fn code01_verify_slot_stays_host_plan() {
    let host = compile_host(Some("cargo test -p lokai-app"), false);
    assert_eq!(
        host.plan.verify_cmd.as_deref(),
        Some("cargo test -p lokai-app")
    );
}

#[test]
fn code01_prompt_finish_remain_kernel_overlay_compile() {
    let host = compile_host(None, false);
    let def = CodingAgentDefinition::production();
    let compiled = compile_build_agent_config(&host, None, &role_build("planner", false));
    let overlay = compiled.system_overlay.expect("pack overlay");
    assert_eq!(overlay, def.overlay(&RoleId::new("planner")));
    assert!(
        !overlay.contains("You are operating inside the user's workspace"),
        "kernel prompt must not be injected as system_overlay"
    );
    assert!(compiled.briefing.is_none());
}

#[test]
fn specialist_node_is_not_reported_before_the_agent_is_built() {
    let src = include_str!("turn_execution.rs");
    let start = src
        .find("|build, user_input|")
        .expect("specialist build closure");
    let end = src[start..]
        .find("|aid, step|")
        .expect("step callback");
    let body = &src[start..start + end];
    let build = body.find("build_agent(").expect("build");
    let started = body.find("NodeStarted").expect("node started");
    assert!(
        build < started,
        "a specialist that fails to build must not be reported as started"
    );
}

#[test]
fn spawn_does_not_report_started_before_the_specialist_is_built() {
    let src = include_str!("turn_execution.rs");
    let spawn = src
        .find("pub(crate) async fn execute_spawn(")
        .expect("execute_spawn");
    let body = &src[spawn..];
    let register = body.find("register_spawn_task").expect("register");
    let specialist = body.find("run_spawned_specialist").expect("specialist");
    let before_specialist = &body[register..specialist];
    assert!(
        !before_specialist.contains("\"started\""),
        "a spawn that fails before the specialist is built must not report started"
    );
    let execute = src
        .find("impl RootExecute for AppRootExecute")
        .expect("root execute");
    let execute_body = &src[execute..spawn];
    let report = execute_body.find("report_run_status").expect("report");
    let claim = execute_body.find(".execute_attempt(").expect("execute");
    assert!(report < claim);
    assert!(execute_body.contains("announce_start"));
}
