//! CODE-01 same-crate compile-path pins. Does not expand `build_agent` visibility.

use super::*;
use crate::definition::CodingAgentDefinition;
use lokai_domain::{DataClass, DisclosureTier};
use lokai_orchestrator::{AgentBuildRequest, RoleId, SessionStartPlan};

fn compile_host(verify_cmd: Option<&str>, force_explain: bool) -> TurnExecutionHost {
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy, None, artifact_store);
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
        tokenizer: Arc::new(lokai_core::HeuristicTokenizer),
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

fn dummy_provider() -> Arc<dyn lokai_inference::InferenceProvider> {
    Arc::new(NoopProvider)
}

struct NoopProvider;
#[async_trait::async_trait]
impl lokai_inference::InferenceProvider for NoopProvider {
    async fn chat(
        &self,
        _req: lokai_inference::ChatRequest,
        _on_token: &mut lokai_inference::TokenSink<'_>,
    ) -> Result<lokai_inference::ChatResponse, lokai_inference::InferenceError> {
        Err(lokai_inference::InferenceError::Provider(
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
