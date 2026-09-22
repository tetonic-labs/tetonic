//! WORK-01 identity / JobSpec / executor pins.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use tetonic_app::commands;
use tetonic_app::definition::{CodingAgentDefinition, CODING_IDENTITY_ID};
use tetonic_app::events;
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_core::{Agent, AgentConfig, Conversation};
use tetonic_domain::{
    ActionKind, AgentAttemptExecutor, AgentIdentity, AgentInvocation, AgentJobSpec,
    AttemptExecutionContext, AttemptId, AuthorizedAction, CandidateOutcome, IdentityId, RunId,
    TaskId, TaskInputBinding, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal,
};
use tetonic_inference::{
    ChatRequest, ChatResponse, InferenceError, InferenceProvider, Message, TokenSink,
};
use tetonic_run::{binding_input_digest, job_input_digest};
use tetonic_runtime::LocalAgentAttemptExecutor;

struct FakeEventSink;
impl events::ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: events::ApplicationEvent) {}
}

static WORK01_DB: AtomicU64 = AtomicU64::new(0);

fn make_app() -> (Application, tetonic_memory::SharedStore) {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_work01_{}_{}.db",
        std::process::id(),
        WORK01_DB.fetch_add(1, Ordering::Relaxed)
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

async fn inspect(app: &Application, run_id: &RunId) -> tetonic_domain::RunSnapshot {
    app.runs
        .inspect_run(commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("inspect_run")
}

#[tokio::test]
async fn work01_begin_turn_run_names_job_spec() {
    let (app, _store) = make_app();
    let started = start_fresh(&app).await;
    let user_input = "work01 distinctive user input";
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: user_input.into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");
    let snap = inspect(&app, &plan.run_id).await;
    let spec = snap.job_spec.as_ref().expect("job_spec");
    assert_eq!(spec.identity_id.0, CODING_IDENTITY_ID);
    assert_ne!(spec.identity_id.0, started.session_id);
    assert_eq!(
        spec.definition_digest,
        CodingAgentDefinition::production().definition_digest()
    );
    assert_eq!(spec.input_digest, job_input_digest(user_input));
    assert_eq!(plan.job_spec, spec.clone());
    let attempt = snap
        .attempts
        .get(&plan.attempt_id)
        .expect("attempt after plan");
    assert_eq!(attempt.input_digest, spec.input_digest);
}

#[tokio::test]
async fn work01_create_run_names_job_spec_without_session() {
    let (app, _store) = make_app();
    let run_id = app
        .runs
        .create_run(commands::CreateRunCommand {
            session_id: None,
            root_task_id: None,
            ..Default::default()
        })
        .await
        .expect("create_run");
    let snap = inspect(&app, &run_id).await;
    assert!(snap.session_id.is_none());
    let spec = snap.job_spec.expect("job_spec");
    assert_eq!(spec.identity_id.0, CODING_IDENTITY_ID);
    assert_eq!(spec.input_digest, job_input_digest(""));
}

#[tokio::test]
async fn work01_identity_record_is_not_session() {
    let (app, store) = make_app();
    let started = start_fresh(&app).await;
    app.runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "after a turn".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");
    let id = IdentityId::new(CODING_IDENTITY_ID);
    let rec = store
        .read(move |db| tetonic_run::get_identity(db, &id))
        .await
        .expect("read")
        .expect("get_identity")
        .expect("identity row");
    assert_eq!(rec.id.0, CODING_IDENTITY_ID);
    assert_ne!(rec.id.0, started.session_id);
    assert_eq!(rec.owning_application, "coding");
}

#[test]
fn work01_code01_absence_pin_replaced() {
    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let domain = fs::read_to_string(engine_root.join("core/tetonic-domain/src/identity.rs"))
        .expect("identity.rs");
    assert!(domain.contains("struct AgentIdentity"));
    assert!(domain.contains("struct AgentJobSpec"));
    for (layer, name) in [
        ("mantle", "tetonic-run"),
        ("core", "tetonic-core"),
        ("mantle", "tetonic-node"),
    ] {
        assert!(
            !source_tree_mentions(
                &engine_root.join(layer).join(name).join("src"),
                "CodingAgentDefinition"
            ),
            "{name} must not import CodingAgentDefinition"
        );
    }
}

#[tokio::test]
async fn work01_spawn_create_run_names_job_spec() {
    let (app, _store) = make_app();
    let started = start_fresh(&app).await;
    app.runs
        .register_spawn_task(&started.session_id, "a0_s0", "coder", "")
        .await
        .expect("register_spawn_task");
    let live = app.sessions.live(&started.session_id).expect("live");
    let run_id = live.current_run_id().expect("spawn minted a run");
    let snap = inspect(&app, &run_id).await;
    let spec = snap
        .job_spec
        .as_ref()
        .expect("spawn CreateRun names JobSpec");
    assert_eq!(spec.identity_id.0, CODING_IDENTITY_ID);
    assert_ne!(spec.identity_id.0, started.session_id);
    assert_eq!(spec.input_digest, job_input_digest(""));
    assert_ne!(spec.input_digest, job_input_digest(&started.session_id));
    let root = TaskId::new(format!("task_root_{}", run_id));
    assert!(
        snap.tasks.contains_key(&root),
        "empty register_spawn_task is a root job"
    );
    assert!(
        !snap.tasks.contains_key(&TaskId::new("task_spawn_a0_s0")),
        "empty execute_spawn must not AddTask a child"
    );
}

#[tokio::test]
async fn work01_job_spec_is_not_invocation() {
    let (app, _store) = make_app();
    let started = start_fresh(&app).await;
    let user_input = "work01 invocation pin";
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id,
            user_input: user_input.into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");
    let snap = inspect(&app, &plan.run_id).await;
    let spec_json = serde_json::to_string(snap.job_spec.as_ref().expect("job_spec")).unwrap();
    assert!(!spec_json.contains("instructions"));
    assert!(!spec_json.contains("empty_tool_nudge"));
    assert!(!spec_json.contains("completion_tool"));
    let invocation = CodingAgentDefinition::production().compile_invocation(
        "",
        "",
        Path::new("."),
        &AgentConfig::default(),
        user_input,
    );
    assert_eq!(
        job_input_digest(&invocation.user_input),
        plan.job_spec.input_digest
    );
    assert_ne!(invocation.instructions, plan.job_spec.input_digest);
}

#[tokio::test]
async fn work01_local_executor_delegates_to_turn() {
    let mut agent = Agent::new(Arc::new(NoChatProvider), DummyHost, AgentConfig::default());
    let mut conversation = Conversation::from_audit_messages(vec![Message::user("keep existing")]);
    let before = conversation.len();
    assert!(!conversation.is_empty());
    let mut executor = LocalAgentAttemptExecutor::new(&mut agent, &mut conversation, |_| {});
    let outcome = executor
        .execute(
            AgentInvocation {
                instructions: String::new(),
                user_input: "later".into(),
                explain_turn: false,
                empty_tool_nudge: false,
                max_steps: 8,
                completion_tool: "finish".into(),
                discipline: tetonic_domain::LoopDiscipline::default(),
            },
            AttemptExecutionContext {
                attempt_id: AttemptId::new("att_work01"),
            },
        )
        .await;
    assert!(matches!(outcome, CandidateOutcome::Failed { .. }));
    assert_eq!(conversation.len(), before);
    assert!(!conversation.is_empty());
}

#[test]
fn work01_one_local_executor_impl() {
    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut impls = Vec::new();
    let layers = [
        "core",
        "strata",
        "mantle",
        "atmos",
        "litho",
        "portals",
        "product",
        "manager",
        "substrate",
        "compute",
        "capabilities",
        "infrastructure",
        "tooling",
        "bins",
        "crates",
    ];
    for layer in layers {
        walk_production_rs(&engine_root.join(layer), &mut |path, src| {
            if src.contains("AgentAttemptExecutor for") {
                impls.push(path.display().to_string());
            }
        });
    }
    assert_eq!(
        impls.len(),
        1,
        "exactly one production impl AgentAttemptExecutor, found {impls:?}"
    );
    assert!(
        impls[0]
            .replace('\\', "/")
            .ends_with("tetonic-runtime/src/executor.rs")
            || impls[0]
                .replace('\\', "/")
                .ends_with("lokai-runtime/src/executor.rs")
    );
}

#[test]
fn work01_infer_hop_create_run_has_no_job_spec() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/tetonic-run/src/infer_admission.rs"),
    )
    .expect("infer_admission.rs");
    assert!(src.contains("job_spec: None"));
    assert!(!src.contains("job_spec: Some"));
    assert!(tetonic_run::hop_job_spec_must_be_none(None));
}

#[tokio::test]
async fn work01_active_still_session_keyed() {
    let (app, _store) = make_app();
    let started = start_fresh(&app).await;
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "session leftover".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");
    let live = app.sessions.live(&started.session_id).expect("live");
    assert_eq!(live.current_run_id(), Some(plan.run_id));
    let src =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/run_service.rs"))
            .expect("run_service.rs");
    assert!(src.contains("fn bind_live_run(&self, session_id: &str"));
    assert!(!src.contains("insert(cmd.session_id.clone(), active.clone())"));
}

#[test]
fn work01_app001_identity_inventory_not_established() {
    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let domain = fs::read_to_string(engine_root.join("core/tetonic-domain/src/identity.rs"))
        .expect("identity.rs");
    assert!(domain.contains("struct AgentIdentity"));
    assert!(domain.contains("struct AgentJobSpec"));
    let fixture = engine_root.join("tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-WORK-001.v4fix");
    assert!(fixture.is_file(), "WORK-001 remains planted inventory");
    let _types: Option<(AgentIdentity, AgentJobSpec)> = None;
}

#[tokio::test]
async fn work01_create_run_projects_job_spec() {
    let (app, _store) = make_app();
    let spec = AgentJobSpec {
        identity_id: IdentityId::new(CODING_IDENTITY_ID),
        definition_digest: "def-test".into(),
        input_digest: job_input_digest("hello-work01"),
        capability_bindings: Vec::new(),
        artifact_bindings: Vec::new(),
        recovery_id: CODING_IDENTITY_ID.into(),
    };
    assert_ne!(
        spec.input_digest,
        binding_input_digest(&TaskInputBinding::default())
    );
    let _ = spec;
    let run_id = app
        .runs
        .create_run(commands::CreateRunCommand {
            session_id: None,
            root_task_id: Some("task_work01_proj".into()),
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
        .expect("inspect_run");
    let projected = snap.job_spec.expect("create_run projects job_spec");
    assert_eq!(projected.identity_id, IdentityId::new(CODING_IDENTITY_ID));
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
        panic!("empty-instruction turn must not chat");
    }
}

fn source_tree_mentions(root: &Path, needle: &str) -> bool {
    walk_rs(root, &mut |path| {
        fs::read_to_string(path)
            .map(|src| {
                src.lines().any(|line| {
                    let trimmed = line.trim_start();
                    !trimmed.starts_with("//") && line.contains(needle)
                })
            })
            .unwrap_or(false)
    })
}

fn walk_production_rs(root: &Path, visit: &mut dyn FnMut(&Path, &str)) {
    walk_rs(root, &mut |path| {
        let text = path.to_string_lossy();
        if text.contains("tests") || text.contains("work01_") || text.contains("code01_") {
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
