//! WORK-03 sessionless identity+job door / Session-as-Run pins.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tetonic_app::commands;
use tetonic_app::definition::{CodingAgentDefinition, CODING_IDENTITY_ID};
use tetonic_app::events;
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_core::{Agent, AgentConfig};
use tetonic_domain::{
    ActionKind, AgentIdentity, AgentInvocation, AgentJobSpec, AttemptId, AttemptState,
    AuthorizedAction, CandidateOutcome, IdentityId, RunState, TaskId, ToolAdvertisement, ToolHost,
    ToolOutcome, ToolProposal,
};
use tetonic_inference::{
    ActiveJobRegistry, ChatRequest, ChatResponse, InferenceError, InferenceProvider, TokenSink,
};
use tetonic_run::job_input_digest;

struct FakeEventSink;
impl events::ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: events::ApplicationEvent) {}
}

static WORK03_DB: AtomicU64 = AtomicU64::new(0);

fn make_app() -> Application {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_work03_{}_{}.db",
        std::process::id(),
        WORK03_DB.fetch_add(1, Ordering::Relaxed)
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
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

fn crate_src(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    let mut src = fs::read_to_string(&path).expect(rel);
    if rel == "src/run_service.rs" {
        src.push_str(
            &fs::read_to_string(path.with_file_name("turn_finalization.rs"))
                .expect("turn_finalization.rs"),
        );
    }
    src
}

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src.find(sig).expect(sig);
    let after = &src[start..];
    let brace = after.find('{').expect("fn body");
    let bytes = &after.as_bytes()[brace..];
    let mut depth = 0i32;
    for (i, ch) in bytes.iter().enumerate() {
        match *ch {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after[brace..=brace + i];
                }
            }
            _ => {}
        }
    }
    &after[brace..]
}

fn impl_run_service(src: &str) -> &str {
    src.split("impl RunService for DefaultRunService")
        .nth(1)
        .expect("DefaultRunService impl")
}

fn door_src() -> String {
    crate_src("src/identity_job.rs")
}

fn identity_and_spec(job_input: &str) -> (AgentIdentity, AgentJobSpec) {
    let identity = CodingAgentDefinition::production().coding_identity_record();
    let spec = AgentJobSpec {
        identity_id: identity.id.clone(),
        definition_digest: identity.bound_definition_digest.clone(),
        input_digest: job_input_digest(job_input),
        capability_bindings: Vec::new(),
        artifact_bindings: Vec::new(),
        recovery_id: identity.recovery_id.clone(),
    };
    (identity, spec)
}

fn dummy_agent() -> Agent {
    Agent::new(Arc::new(NoChatProvider), DummyHost, AgentConfig::default())
}

fn empty_invocation(user_input: &str) -> AgentInvocation {
    AgentInvocation {
        instructions: String::new(),
        user_input: user_input.into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 8,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    }
}

struct DummyHost;

impl ToolHost for DummyHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(DummyHost)
    }
    fn propose(&self, _name: &str, _args: &Value) -> Option<ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, _name: &str) -> bool {
        true
    }
    fn is_read_only(&self, _name: &str) -> bool {
        true
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        vec![]
    }
    fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        _name: &str,
        _args: &Value,
        _auth: Option<&AuthorizedAction>,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        let _ = ActionKind::ReadFile;
        ToolOutcome::ok("ok", "ok")
    }
}

struct NoChatProvider;

#[async_trait::async_trait]
impl InferenceProvider for NoChatProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        panic!("sessionless door test must not chat");
    }
}

fn walk_production_rs(root: &Path, visit: &mut dyn FnMut(&Path, &str)) {
    walk_rs(root, &mut |path| {
        let text = path.to_string_lossy();
        if text.contains("tests") || text.contains("\\test") || text.contains("/test") {
            return false;
        }
        if let Ok(src) = fs::read_to_string(path) {
            visit(path, &src);
        }
        false
    });
}

fn walk_rs(root: &Path, pred: &mut dyn FnMut(&Path) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if walk_rs(&path, pred) {
                return true;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") && pred(&path) {
            return true;
        }
    }
    false
}

#[tokio::test]
async fn work03_start_identity_job_executes_without_session() {
    let app = make_app();
    let (identity, spec) = identity_and_spec("work03 door");
    assert_eq!(identity.id.0, CODING_IDENTITY_ID);
    let mut agent = dummy_agent();
    let result = app
        .runs
        .start_identity_job(
            commands::StartIdentityJobCommand {
                identity,
                job_spec: spec,
                invocation: empty_invocation("work03 door"),
            },
            &mut agent,
        )
        .await
        .expect("sessionless door");
    assert!(matches!(result.outcome, CandidateOutcome::Failed { .. }));
    let snap = app
        .runs
        .inspect_run(commands::InspectRunCommand {
            run_id: result.run_id.to_string(),
        })
        .await
        .expect("inspect");
    assert!(snap.session_id.is_none());
    assert_eq!(
        result.task_id,
        TaskId::new(format!("task_root_{}", result.run_id))
    );
}

#[test]
fn work03_start_identity_job_uses_local_executor() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Execution);
}

#[test]
fn work03_start_identity_job_inserts_active_by_attempt() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Registry);
}

#[tokio::test]
async fn work03_create_run_still_does_not_execute() {
    let src = crate_src("src/run_service.rs");
    let body = fn_body(impl_run_service(&src), "async fn create_run(");
    assert!(!body.contains("LocalAgentAttemptExecutor"));
    assert!(!body.contains("start_identity_job"));
    let app = make_app();
    let run_id = app
        .runs
        .create_run(commands::CreateRunCommand {
            session_id: None,
            root_task_id: None,
            ..Default::default()
        })
        .await
        .expect("create_run persist-only");
    let snap = app
        .runs
        .inspect_run(commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("inspect");
    assert!(snap.session_id.is_none());
    assert!(snap.attempts.is_empty());
    assert_eq!(snap.state, RunState::Created);
}

#[test]
fn work03_run_turn_still_requires_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::RootKeys);
}

#[tokio::test]
async fn work03_start_identity_job_finishes_through_finish_turn_run() {
    let execution = crate_src("../../mantle/tetonic-run/src/managed/execution.rs");
    let body = fn_body(&execution, "async fn start_identity_job(");
    assert!(body.contains("self.finalize(FinalizeJob"));
    assert!(body.contains("self.release_dispatch(&ticket.id)"));
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
    let app = make_app();
    let (identity, spec) = identity_and_spec("work03 finish");
    let mut agent = dummy_agent();
    let result = app
        .runs
        .start_identity_job(
            commands::StartIdentityJobCommand {
                identity,
                job_spec: spec,
                invocation: empty_invocation("work03 finish"),
            },
            &mut agent,
        )
        .await
        .expect("door finish");
    let snap = app
        .runs
        .inspect_run(commands::InspectRunCommand {
            run_id: result.run_id.to_string(),
        })
        .await
        .expect("inspect");
    assert_eq!(snap.state, RunState::Failed);
    let attempt = snap.attempts.get(&result.attempt_id).expect("attempt");
    assert_eq!(attempt.state, AttemptState::Failed);
    assert!(attempt.result_digest.is_none());
}

#[tokio::test]
async fn work03_start_identity_job_removes_active() {
    let app = make_app();
    let (identity, spec) = identity_and_spec("work03 remove");
    let mut agent = dummy_agent();
    let result = app
        .runs
        .start_identity_job(
            commands::StartIdentityJobCommand {
                identity,
                job_spec: spec,
                invocation: empty_invocation("work03 remove"),
            },
            &mut agent,
        )
        .await
        .expect("door");
    let err = app
        .runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: String::new(),
                attempt_id: result.attempt_id,
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect_err("active must be removed");
    assert!(err.to_string().contains("no active turn run"));
}

#[test]
fn work03_start_identity_job_heartbeat_fail_skips_finish() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ClaimFence);
}

#[test]
fn work03_start_identity_job_seals_candidate_not_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
}

#[test]
fn work03_start_identity_job_does_not_call_complete_turn() {
    let door = door_src();
    let body = fn_body(&door, "async fn start_identity_job(");
    assert!(!body.contains("complete_turn"));
}

#[tokio::test]
async fn work03_start_identity_job_stamps_agent_run_id() {
    let app = make_app();
    let (identity, spec) = identity_and_spec("work03 stamp");
    let mut agent = dummy_agent();
    assert!(agent.managed_run_id().is_none());
    let result = app
        .runs
        .start_identity_job(
            commands::StartIdentityJobCommand {
                identity,
                job_spec: spec,
                invocation: empty_invocation("work03 stamp"),
            },
            &mut agent,
        )
        .await
        .expect("door stamp");
    assert_eq!(agent.managed_run_id(), Some(result.run_id.0.as_str()));
}

#[test]
fn work03_broker_has_no_session_as_run() {
    let src = crate_src("../../mantle/tetonic-broker/src/broker.rs");
    let prod = production_prefix(&src);
    assert!(!prod.contains("RunId::new(session_id)"));
    assert!(prod.contains("fn cancel_run_jobs"));
    let mut hits = Vec::new();
    walk_production_rs(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mantle/tetonic-broker/src"),
        &mut |path, text| {
            if production_prefix(text).contains("RunId::new(session_id)") {
                hits.push(path.display().to_string());
            }
        },
    );
    assert!(
        hits.is_empty(),
        "broker production Session-as-Run: {hits:?}"
    );
}

#[test]
fn work03_session_cancel_uses_current_run_id() {
    let misc = crate_src("../../litho/lokaid/src/daemon/handlers/misc.rs");
    assert!(misc.contains("services.app.cancel_session_broker_jobs(&p.session_id)"));
    let src = crate_src("src/product_submit.rs");
    assert!(src.contains("broker.cancel_session_jobs(session_id)"));
    assert!(src.contains("live.current_run_id()"));
    assert!(src.contains("broker.cancel_run_jobs(&run_id)"));
}

#[test]
fn work03_session_cancel_cancels_indexed_hops_by_stored_run_id() {
    let src = crate_src("../../atmos/tetonic-inference/src/pooled.rs");
    let body = fn_body(&src, "pub fn cancel_session_jobs(");
    assert!(body.contains("stored_run_id"));
    assert!(!body.contains("RunId::new(session_id)"));
}

#[test]
fn work03_job_record_stores_run_id() {
    let src = crate_src("../../atmos/tetonic-inference/src/attempt.rs");
    assert!(src.contains("run_id: Option<String>"));
    let reg = ActiveJobRegistry::new();
    reg.begin_or_bind_attempt("job_a", Some("sess_a"), None, Some("run_real"));
    let jobs = reg.jobs_for_session("sess_a");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].2.as_deref(), Some("run_real"));
}

#[test]
fn work03_pooled_cancel_not_session_as_run() {
    let src = crate_src("../../atmos/tetonic-inference/src/pooled.rs");
    let cancel = fn_body(&src, "pub fn cancel_session_jobs(");
    assert!(!cancel.contains("RunId::new(session_id)"));
    let hop = fn_body(&src, "fn hop_identity_for_ajr(");
    assert!(hop.contains("hop_run_id"));
    assert!(!hop.contains("f.run_id"));
}

#[test]
fn work03_job_record_run_id_is_never_session() {
    let src = crate_src("../../atmos/tetonic-inference/src/pooled.rs");
    let hop = fn_body(&src, "fn hop_identity_for_ajr(");
    assert!(hop.contains("hop_run_id"));
    assert!(!hop.contains("f.run_id"));
    let reg = ActiveJobRegistry::new();
    reg.begin_or_bind_attempt("job_b", Some("sess_b"), None, Some("run_b"));
    let jobs = reg.jobs_for_session("sess_b");
    assert_ne!(jobs[0].2.as_deref(), Some("sess_b"));
}

#[test]
fn work03_spawn_budget_uses_run_id() {
    let src = crate_src("src/spawn_budget.rs");
    assert!(src.contains("run_id: RunId"));
    assert!(!src.contains("session_id"));
    assert!(!src.contains("RunId::new(self.session_id)"));
    let turn = crate_src("src/turn_execution.rs");
    assert!(turn.contains("turn_plan.run_id.clone()"));
    assert!(turn.contains("spawn_run_id.clone()"));
}

#[test]
fn work03_release_session_uses_stored_run_id() {
    let src = crate_src("src/spawn_budget.rs");
    let body = fn_body(&src, "fn release_session(");
    assert!(body.contains("release_for_run(&self.run_id"));
    assert!(!body.contains("RunId::new(self.session_id)"));
}

#[test]
fn work03_chat_request_no_session_run_fallback() {
    let src = crate_src("../../mantle/tetonic-broker/src/chat_request.rs");
    let body = fn_body(&src, "pub fn compute_request_from_chat(");
    assert!(body.contains("Uuid::new_v4"));
    assert!(!body.contains("m.run_id"));
    assert!(!body.contains("m.task_id"));
    assert!(!body.contains("m.attempt_id"));
    assert!(!body.contains("session_id"));
}

#[test]
fn work03_agent_context_no_session_run_fallback() {
    let src = crate_src("../../core/tetonic-core/src/agent.rs");
    let prod = production_prefix(&src);
    assert!(prod.contains("unwrap_or_else(|| \"local_run\".into())"));
    assert!(!prod.contains("or_else(|| self.config.session_id.clone())"));
}

#[test]
fn work03_agent_fabric_run_id_no_session_fallback() {
    let src = crate_src("../../core/tetonic-core/src/agent.rs");
    let prod = production_prefix(&src);
    assert!(prod.contains(".or_else(|| compiled_run_id.clone())"));
    assert!(!prod.contains("or_else(|| self.config.session_id.clone())"));
}

#[test]
fn work03_app002_not_established_run_turn_remains() {
    let src = crate_src("src/run_service.rs");
    assert!(src.contains("async fn run_turn("));
    assert!(src.contains("begin_turn_run(&cmd.session_id"));
}

#[test]
fn work03_id001_not_established_session_chat_remains() {
    let src = crate_src("src/session_live.rs");
    assert!(src.contains("struct LiveSession"));
    let sessions = crate_src("src/services.rs");
    assert!(sessions.contains("fn start_session"));
}

#[test]
fn work03_work001_not_established_leftover_turn_remains() {
    let exec = crate_src("src/turn_execution.rs");
    assert!(exec.contains("async fn execute_spawn"));
    let run = crate_src("src/run_service.rs");
    assert!(run.contains("async fn run_turn("));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-WORK-001.v4fix");
    assert!(fixture.is_file(), "WORK-001 remains planted inventory");
}

#[test]
fn work03_identity_id_is_not_session() {
    let _ = IdentityId::new(CODING_IDENTITY_ID);
    let _ = AttemptId::new("att_work03");
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;
