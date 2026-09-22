//! WORK-05: Bind Identity, Job, Invocation, and Children.
//! Implements V4-PROOF-05:
//! Assertion 1: Reject identity/definition/input/Attempt mismatch before execute (0 calls).
//! Assertion 2: Persist distinct child role/input binding in child admission and spec.
//! Assertion 3: Child Attempt bound in LocalAgentAttemptExecutor and reflected in Infer correlation.
//! Assertion 4: Duplicate delivery rejection at dispatch boundary.
//! Assertion 5: Effective authority binding enforcement (unavailable bindings, limits).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tetonic_app::coding_pack::CodingPack;
use tetonic_app::commands::{RunTurnCommand, StartIdentityJobCommand, StartSessionCommand};
use tetonic_app::definition::CodingAgentDefinition;
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::services::RunService;
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_core::{Agent, AgentConfig, Conversation, Step};
use tetonic_domain::{
    ActionKind, AgentAttemptExecutor, AgentIdentity, AgentInvocation, AgentJobSpec,
    AttemptExecutionContext, AttemptId, AttemptState, AuthorizedAction, CandidateOutcome,
    IdentityId, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal,
};
use tetonic_inference::{
    ChatRequest, ChatResponse, GenUsage, InferenceError, InferenceProvenance, InferenceProvider,
    Message, TokenSink,
};
use tetonic_orchestrator::{
    run_orchestrated_turn, ChildAdmit, ChildJob, OrchestratedTurnInput, OrchestrationMode, RoleId,
    RootExecute, SpawnLimits,
};
use tetonic_run::idempotency::job_input_digest;

struct FakeEventSink;
impl ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: ApplicationEvent) {}
}

static WORK05_DB: AtomicU64 = AtomicU64::new(0);

fn make_app() -> (Application, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join(format!(
        "lokai_work05_{}_{}.db",
        std::process::id(),
        WORK05_DB.fetch_add(1, Ordering::Relaxed)
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            tmp.path().join("artifacts"),
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = tetonic_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    (app, tmp)
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

fn empty_invocation(user_input: &str) -> AgentInvocation {
    AgentInvocation {
        instructions: "test instructions".into(),
        user_input: user_input.into(),
        explain_turn: false,
        empty_tool_nudge: false,
        max_steps: 8,
        completion_tool: "finish".into(),
        discipline: tetonic_domain::LoopDiscipline::default(),
    }
}

#[derive(Clone)]
struct CountingProvider {
    chat_calls: Arc<AtomicUsize>,
    recorded_attempt_ids: Arc<Mutex<Vec<Option<String>>>>,
    recorded_task_ids: Arc<Mutex<Vec<Option<String>>>>,
}

impl CountingProvider {
    fn new() -> Self {
        Self {
            chat_calls: Arc::new(AtomicUsize::new(0)),
            recorded_attempt_ids: Arc::new(Mutex::new(Vec::new())),
            recorded_task_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl InferenceProvider for CountingProvider {
    async fn chat(
        &self,
        req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.chat_calls.fetch_add(1, Ordering::SeqCst);
        let att = req.fabric.as_ref().and_then(|m| m.attempt_id.clone());
        self.recorded_task_ids
            .lock()
            .unwrap()
            .push(req.fabric.as_ref().and_then(|m| m.task_id.clone()));
        self.recorded_attempt_ids.lock().unwrap().push(att);
        Ok(ChatResponse {
            message: Message::assistant("done"),
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}

#[derive(Clone)]
struct CountingHost {
    tool_calls: Arc<AtomicUsize>,
    advertised: Vec<ToolAdvertisement>,
}

impl CountingHost {
    fn new() -> Self {
        Self {
            tool_calls: Arc::new(AtomicUsize::new(0)),
            advertised: Vec::new(),
        }
    }

    fn with_tools(tools: &[&str]) -> Self {
        let advertised = tools
            .iter()
            .map(|name| ToolAdvertisement {
                name: (*name).to_string(),
                description: format!("tool {name}"),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            })
            .collect();
        Self {
            tool_calls: Arc::new(AtomicUsize::new(0)),
            advertised,
        }
    }
}

impl ToolHost for CountingHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn propose(&self, _name: &str, _args: &Value) -> Option<ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, _name: &str) -> bool {
        true
    }
    fn is_read_only(&self, name: &str) -> bool {
        !matches!(name, "write_file" | "edit_file" | "bash")
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        self.advertised.clone()
    }
    fn validate_tool_args(&self, _name: &str, _args: &Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        _args: &Value,
        _auth: Option<&AuthorizedAction>,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        self.tool_calls.fetch_add(1, Ordering::SeqCst);
        let _ = ActionKind::ReadFile;
        if name == "lsp_diagnostics" {
            ToolOutcome::ok(
                "2 diagnostic(s) in main.rs",
                "main.rs:42: missing bounds check",
            )
        } else if name == "write_file" {
            ToolOutcome::ok("wrote file", "file written")
        } else {
            ToolOutcome::ok("ok", "ok")
        }
    }
}

fn make_agent(provider: Arc<dyn InferenceProvider>, host: CountingHost) -> Agent {
    Agent::new(provider, host, AgentConfig::default())
}

// ----------------------------------------------------------------------------
// Assertion 1: Pre-Execution Rejection of Mismatches (Asserting 0 Calls)
// ----------------------------------------------------------------------------

#[tokio::test]
async fn work05_assertion1_identity_id_mismatch_rejected_before_execute() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    let (identity, mut spec) = identity_and_spec("test input");
    spec.identity_id = IdentityId::new("wrong_identity_id");

    let cmd = StartIdentityJobCommand {
        identity,
        job_spec: spec,
        invocation: empty_invocation("test input"),
    };

    let res = app.runs.start_identity_job(cmd, &mut agent).await;
    assert!(
        res.is_err(),
        "mismatched identity_id must fail before execute"
    );
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls"
    );
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0, "zero tool calls");
}

#[tokio::test]
async fn work05_assertion1_definition_digest_mismatch_rejected_before_execute() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    let (identity, mut spec) = identity_and_spec("test input");
    spec.definition_digest = "tampered_definition_digest".into();

    let cmd = StartIdentityJobCommand {
        identity,
        job_spec: spec,
        invocation: empty_invocation("test input"),
    };

    let res = app.runs.start_identity_job(cmd, &mut agent).await;
    assert!(
        res.is_err(),
        "mismatched definition_digest must fail before execute"
    );
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls"
    );
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0, "zero tool calls");
}

#[tokio::test]
async fn work05_assertion1_input_digest_mismatch_rejected_before_execute() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    let (identity, spec) = identity_and_spec("expected input");

    // Invocation provides completely different input.
    let cmd = StartIdentityJobCommand {
        identity,
        job_spec: spec,
        invocation: empty_invocation("different input"),
    };

    let res = app.runs.start_identity_job(cmd, &mut agent).await;
    assert!(
        res.is_err(),
        "mismatched input_digest must fail before execute"
    );
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls"
    );
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0, "zero tool calls");
}

#[tokio::test]
async fn work05_assertion1_prebound_attempt_mismatch_rejected_before_execute() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    // Pre-stamp the agent with an existing Attempt ID.
    agent.stamp_attempt_id("pre_bound_att_123");

    let (identity, spec) = identity_and_spec("test input");
    let cmd = StartIdentityJobCommand {
        identity,
        job_spec: spec,
        invocation: empty_invocation("test input"),
    };

    let res = app.runs.start_identity_job(cmd, &mut agent).await;
    assert!(
        res.is_err(),
        "pre-bound agent must be rejected for fresh start_identity_job"
    );
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls"
    );
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0, "zero tool calls");
}

#[tokio::test]
async fn work05_assertion1_local_executor_mismatched_attempt_fails_before_turn() {
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    // Pre-stamp agent with attempt A.
    agent.stamp_attempt_id("attempt_A");

    let mut conv = Conversation::new();
    let mut executor =
        tetonic_runtime::LocalAgentAttemptExecutor::new(&mut agent, &mut conv, |_| {});

    // Try executing with attempt B.
    let outcome = executor
        .execute(
            empty_invocation("hello"),
            AttemptExecutionContext {
                attempt_id: AttemptId::new("attempt_B"),
            },
        )
        .await;

    match outcome {
        CandidateOutcome::Failed { message } => {
            assert!(
                message.contains("mismatch"),
                "failure message must indicate attempt mismatch: {message}"
            );
        }
        other => panic!("expected CandidateOutcome::Failed, got {other:?}"),
    }

    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls on attempt mismatch"
    );
    assert_eq!(
        host.tool_calls.load(Ordering::SeqCst),
        0,
        "zero tool calls on attempt mismatch"
    );
}

// ----------------------------------------------------------------------------
// Assertion 2: Child Jobs Persist Distinct Role and Input Binding
// ----------------------------------------------------------------------------

#[tokio::test]
async fn work05_assertion2_child_jobs_persist_distinct_spec_and_input_digest() {
    let (app, tmp) = make_app();
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session");

    let parent_turn = RunTurnCommand {
        session_id: started.session_id.clone(),
        user_input: "parent job input".into(),
        verify_cmd: None,
        llm_router: Some(false),
    };
    let plan = app.runs.plan_turn(&parent_turn).await.expect("plan_turn");

    let parent_spec = app
        .runs
        .active_job_spec(&plan.attempt_id)
        .expect("parent active job spec");
    let parent_digest = app
        .runs
        .active_input_digest(&plan.attempt_id)
        .expect("parent active input digest");

    // Admit child with distinct role and input.
    let child_job = app.runs.child_job(&started.session_id);
    let critic_input = "critic review input";
    let child_att = child_job
        .admit_child(ChildAdmit {
            agent_id: "agent_critic_1".into(),
            role: Some(RoleId("critic".into())),
            job_input: critic_input.into(),
        })
        .await
        .expect("admit_child");

    let child_spec = app
        .runs
        .active_job_spec(&child_att)
        .expect("child active job spec");
    let child_digest = app
        .runs
        .active_input_digest(&child_att)
        .expect("child active input digest");

    // Verify child input digest differs from parent.
    assert_ne!(
        child_digest, parent_digest,
        "child input digest must differ from parent when child input differs"
    );
    assert_eq!(
        child_digest,
        job_input_digest(critic_input),
        "child input digest must match child's own prompt"
    );

    // Verify child AgentJobSpec differs from parent.
    assert_ne!(
        child_spec.recovery_id, parent_spec.recovery_id,
        "child recovery_id must include agent_id and role"
    );
    assert!(
        child_spec.recovery_id.contains("agent_critic_1"),
        "recovery_id must contain child agent_id"
    );
    assert!(
        child_spec.recovery_id.contains("critic"),
        "recovery_id must contain child role"
    );
    assert_eq!(
        child_spec.input_digest,
        job_input_digest(critic_input),
        "child job_spec must carry child's own input digest"
    );

    // Use a separate supervisor instance reading the durable journal, not ActiveTurnRun.
    let db_path = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "db"))
        .unwrap();
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let recovered = tetonic_run::DurableRunSupervisor::new(Some(store));
    use tetonic_run::RunSupervisor;
    let snapshot = recovered.snapshot(plan.run_id.clone()).await.unwrap();
    let task_id = snapshot.attempts[&child_att].task_id.clone();
    assert_eq!(
        snapshot.tasks[&task_id].binding.job_spec.as_ref(),
        Some(&child_spec)
    );
    assert_eq!(
        snapshot.tasks[&task_id].binding.job_role.as_deref(),
        Some("critic")
    );
    assert_eq!(
        snapshot.attempts[&child_att].input_digest,
        child_spec.input_digest
    );

    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = Agent::new(
        provider.clone(),
        host,
        AgentConfig {
            specialist_role: Some("critic".into()),
            task_id: Some(plan.task_id.to_string()), // old inherited root correlation
            ..AgentConfig::default()
        },
    );
    let mut invocation = empty_invocation(critic_input);
    invocation.explain_turn = true;
    let outcome = app
        .runs
        .execute_attempt(
            child_att.clone(),
            &mut agent,
            &mut Conversation::new(),
            invocation,
            &mut |_| {},
        )
        .await;
    assert!(
        matches!(outcome, CandidateOutcome::Completed { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        provider.recorded_task_ids.lock().unwrap()[0].as_deref(),
        Some(task_id.0.as_str())
    );
    assert_eq!(
        provider.recorded_attempt_ids.lock().unwrap()[0].as_deref(),
        Some(child_att.0.as_str())
    );
    child_job
        .complete_child(child_att.clone(), outcome)
        .await
        .unwrap();

    // A second sibling must not replay the first sibling's admission/finalization commands.
    let second = child_job
        .admit_child(ChildAdmit {
            agent_id: "revision_2".into(),
            role: Some(RoleId("coder".into())),
            job_input: "revise findings".into(),
        })
        .await
        .unwrap();
    assert_ne!(second, child_att);
    let mut revision = Agent::new(
        provider.clone(),
        CountingHost::new(),
        AgentConfig {
            specialist_role: Some("coder".into()),
            ..AgentConfig::default()
        },
    );
    let mut invocation = empty_invocation("revise findings");
    invocation.explain_turn = true;
    let outcome = app
        .runs
        .execute_attempt(
            second.clone(),
            &mut revision,
            &mut Conversation::new(),
            invocation,
            &mut |_| {},
        )
        .await;
    assert!(
        matches!(outcome, CandidateOutcome::Completed { .. }),
        "{outcome:?}"
    );
    child_job
        .complete_child(second.clone(), outcome)
        .await
        .unwrap();
    let snapshot = recovered.snapshot(plan.run_id).await.unwrap();
    assert_eq!(
        snapshot.attempts[&child_att].state,
        tetonic_domain::AttemptState::Succeeded
    );
    assert_eq!(
        snapshot.attempts[&second].state,
        tetonic_domain::AttemptState::Succeeded
    );
}

// ----------------------------------------------------------------------------
// Assertion 3: Child Attempt Bound in Executor and Reflected in Infer Correlation
// ----------------------------------------------------------------------------

#[tokio::test]
async fn work05_assertion3_child_attempt_bound_in_executor_and_reflected_in_infer() {
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    // Agent starts unbound.
    assert_eq!(agent.bound_attempt_id(), None);

    let child_attempt_id = AttemptId::new("att_child_critic_999");
    let mut conv = Conversation::new();
    let mut executor =
        tetonic_runtime::LocalAgentAttemptExecutor::new(&mut agent, &mut conv, |_| {});

    let _outcome = executor
        .execute(
            empty_invocation("run child turn"),
            AttemptExecutionContext {
                attempt_id: child_attempt_id.clone(),
            },
        )
        .await;

    // 1. Agent should now be bound to the child attempt ID.
    assert_eq!(agent.bound_attempt_id(), Some("att_child_critic_999"));

    // 2. Infer provider should have received the child attempt ID in FabricCallMeta.
    assert_eq!(provider.chat_calls.load(Ordering::SeqCst), 1);
    let recorded = provider.recorded_attempt_ids.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0],
        Some("att_child_critic_999".into()),
        "Infer call must correlate to child Attempt ID"
    );
}

// ----------------------------------------------------------------------------
// Assertion 4: Duplicate Delivery Rejection at Dispatch Boundary
// ----------------------------------------------------------------------------

#[tokio::test]
async fn work05_session_busy_rejects_a_second_job() {
    let (app, tmp) = make_app();
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session");

    let (chat_tx, mut chat_rx) = tokio::sync::mpsc::unbounded_channel();
    app.bind_inference(
        Arc::new(MockDelayProvider {
            delay: Duration::from_millis(200),
            on_chat: Some(chat_tx),
        }),
        None,
    );

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);

            // First submission succeeds.
            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "turn 1 initial".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("first submit_chat_turn");

            // Wait until first turn is in flight.
            chat_rx.recv().await.expect("chat started");

            // Duplicate submission while in flight must fail closed.
            let dup_res = app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "turn 1 duplicate".into(),
                verify_cmd: None,
                llm_router: Some(false),
            });
            assert!(
                dup_res.is_err(),
                "duplicate delivery while turn is active must fail closed"
            );

            rx.await.expect("finish");
        })
        .await;
}

struct MockDelayProvider {
    delay: Duration,
    on_chat: Option<tokio::sync::mpsc::UnboundedSender<()>>,
}

#[async_trait]
impl InferenceProvider for MockDelayProvider {
    async fn chat(
        &self,
        _req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        if let Some(tx) = &self.on_chat {
            let _ = tx.send(());
        }
        tokio::time::sleep(self.delay).await;
        Ok(ChatResponse {
            message: Message::assistant("delayed response"),
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}

// ----------------------------------------------------------------------------
// Assertion 5: Effective Authority Binding Enforcement
// ----------------------------------------------------------------------------

#[tokio::test]
async fn work05_assertion5_unavailable_capability_binding_rejected_before_execute() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    let (identity, mut spec) = identity_and_spec("test authority");
    spec.capability_bindings = vec!["workspace:unresolved-write".into()];

    let cmd = StartIdentityJobCommand {
        identity,
        job_spec: spec,
        invocation: empty_invocation("test authority"),
    };

    let res = app.runs.start_identity_job(cmd, &mut agent).await;
    assert!(
        res.is_err(),
        "unavailable capability binding must be rejected before execution"
    );
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls"
    );
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0, "zero tool calls");
}

#[tokio::test]
async fn work05_assertion5_substituted_capability_binding_rejected_before_execute() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    let (identity, mut spec) = identity_and_spec("test authority");
    spec.capability_bindings = vec!["host:admin".into()];

    let cmd = StartIdentityJobCommand {
        identity,
        job_spec: spec,
        invocation: empty_invocation("test authority"),
    };

    let res = app.runs.start_identity_job(cmd, &mut agent).await;
    assert!(
        res.is_err(),
        "substituted capability binding must be rejected before execution"
    );
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls"
    );
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0, "zero tool calls");
}

// ----------------------------------------------------------------------------
// Architectural Invariant Locks (None ESTABLISHED)
// ----------------------------------------------------------------------------

#[tokio::test]
async fn same_attempt_concurrent_dispatch_runs_one_executor_and_enforces_input_and_limits() {
    let (app, tmp) = make_app();
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    let plan = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session.session_id,
            user_input: "bound input".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .unwrap();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut first = make_agent(provider.clone(), host.clone());
    let mut second = make_agent(provider.clone(), host.clone());
    let mut first_convo = Conversation::new();
    let mut second_convo = Conversation::new();
    let mut step_a = |_| {};
    let mut step_b = |_| {};
    let mismatch = app
        .runs
        .execute_attempt(
            plan.attempt_id.clone(),
            &mut first,
            &mut first_convo,
            empty_invocation("wrong input"),
            &mut step_a,
        )
        .await;
    assert!(matches!(mismatch, CandidateOutcome::Failed { .. }));
    let mut excessive = empty_invocation("bound input");
    excessive.max_steps = 1000;
    let limited = app
        .runs
        .execute_attempt(
            plan.attempt_id.clone(),
            &mut first,
            &mut first_convo,
            excessive,
            &mut step_a,
        )
        .await;
    assert!(matches!(limited, CandidateOutcome::Failed { .. }));
    assert_eq!(provider.chat_calls.load(Ordering::SeqCst), 0);
    let mut invocation = empty_invocation("bound input");
    invocation.explain_turn = true;
    let (a, b) = tokio::join!(
        app.runs.execute_attempt(
            plan.attempt_id.clone(),
            &mut first,
            &mut first_convo,
            invocation.clone(),
            &mut step_a
        ),
        app.runs.execute_attempt(
            plan.attempt_id.clone(),
            &mut second,
            &mut second_convo,
            invocation.clone(),
            &mut step_b
        ),
    );
    assert_eq!(
        usize::from(matches!(a, CandidateOutcome::Completed { .. }))
            + usize::from(matches!(b, CandidateOutcome::Completed { .. })),
        1,
        "{a:?} {b:?}"
    );
    let replay = app
        .runs
        .execute_attempt(
            plan.attempt_id,
            &mut second,
            &mut second_convo,
            invocation,
            &mut step_b,
        )
        .await;
    assert!(matches!(replay, CandidateOutcome::Failed { .. }));
    assert_eq!(provider.chat_calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sessionless_cancel_stops_a_pending_provider() {
    let (app, _tmp) = make_app();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    struct PendingProvider(tokio::sync::mpsc::UnboundedSender<String>);
    #[async_trait]
    impl InferenceProvider for PendingProvider {
        async fn chat(
            &self,
            req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            self.0.send(req.fabric.unwrap().run_id.unwrap()).unwrap();
            std::future::pending().await
        }
    }
    let provider = Arc::new(PendingProvider(tx));
    let mut agent = Agent::new(provider, CountingHost::new(), AgentConfig::default());
    let (identity, job_spec) = identity_and_spec("cancel me");
    let execute = app.runs.start_identity_job(
        StartIdentityJobCommand {
            identity,
            job_spec,
            invocation: empty_invocation("cancel me"),
        },
        &mut agent,
    );
    // Inspect the persisted Run once the provider has entered, without borrowing Agent.
    let cancel = async {
        let run = rx.recv().await.unwrap();
        app.runs
            .cancel_run(tetonic_app::commands::CancelByRunCommand { run_id: run })
            .await
            .unwrap();
    };
    let (result, _) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(execute, cancel)
    })
    .await
    .unwrap();
    assert!(matches!(
        result.unwrap().outcome,
        CandidateOutcome::Canceled { .. }
    ));
}

struct TestRootExecute {
    runs: Arc<dyn RunService>,
}

#[async_trait]
impl RootExecute for TestRootExecute {
    async fn execute(
        &self,
        attempt_id: AttemptId,
        agent: &mut Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome {
        self.runs
            .execute_attempt(attempt_id, agent, conversation, invocation, on_step)
            .await
    }
}

#[derive(Clone)]
struct OrchestratedTurnProvider {
    root_step: Arc<AtomicUsize>,
    chat_calls: Arc<AtomicUsize>,
    recorded_attempts: Arc<Mutex<Vec<Option<String>>>>,
    recorded_tasks: Arc<Mutex<Vec<Option<String>>>>,
    recorded_runs: Arc<Mutex<Vec<Option<String>>>>,
}

impl OrchestratedTurnProvider {
    fn new() -> Self {
        Self {
            root_step: Arc::new(AtomicUsize::new(0)),
            chat_calls: Arc::new(AtomicUsize::new(0)),
            recorded_attempts: Arc::new(Mutex::new(Vec::new())),
            recorded_tasks: Arc::new(Mutex::new(Vec::new())),
            recorded_runs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl InferenceProvider for OrchestratedTurnProvider {
    async fn chat(
        &self,
        req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.chat_calls.fetch_add(1, Ordering::SeqCst);
        let fabric = req.fabric.as_ref();
        self.recorded_runs
            .lock()
            .unwrap()
            .push(fabric.and_then(|m| m.run_id.clone()));
        self.recorded_tasks
            .lock()
            .unwrap()
            .push(fabric.and_then(|m| m.task_id.clone()));
        self.recorded_attempts
            .lock()
            .unwrap()
            .push(fabric.and_then(|m| m.attempt_id.clone()));

        let user_msg = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");

        if user_msg.contains("Review the edits") {
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                    function: tetonic_inference::FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({"summary": "REVISE: missing bounds check"}),
                    },
                }]),
                usage: GenUsage::default(),
                provenance: InferenceProvenance::default(),
            })
        } else if user_msg.contains("Address critic feedback") {
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                    function: tetonic_inference::FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({"summary": "APPROVE: fixed missing bounds check"}),
                    },
                }]),
                usage: GenUsage::default(),
                provenance: InferenceProvenance::default(),
            })
        } else {
            let step = self.root_step.fetch_add(1, Ordering::SeqCst);
            match step {
                0 => Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: "write_file".into(),
                            arguments: serde_json::json!({"path": "src/main.rs", "content": "fn main() {}"}),
                        },
                    }]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                }),
                1 => Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: "lsp_diagnostics".into(),
                            arguments: serde_json::json!({"path": "src/main.rs"}),
                        },
                    }]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                }),
                _ => Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: "finish".into(),
                            arguments: serde_json::json!({"summary": "initial edit done"}),
                        },
                    }]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                }),
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Additional Proof Evidence: Authority, Orchestrated Path, Lifetime
// ----------------------------------------------------------------------------

#[tokio::test]
async fn work05_assertion1_overprivileged_host_rejected_before_execute() {
    let (app, tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    // Host advertises mutating tool `write_file` while executing for read-only role "critic"
    let host = CountingHost::with_tools(&["write_file", "read_file"]);
    let mut agent = Agent::new(
        provider.clone(),
        host.clone(),
        AgentConfig {
            specialist_role: Some("critic".into()),
            ..AgentConfig::default()
        },
    );

    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session");

    let _parent = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "parent input".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let child_job = app.runs.child_job(&started.session_id);
    let child_att = child_job
        .admit_child(ChildAdmit {
            agent_id: "critic_overpriv".into(),
            role: Some(RoleId("critic".into())),
            job_input: "test overprivileged host".into(),
        })
        .await
        .expect("admit_child");

    let mut invocation = empty_invocation("test overprivileged host");
    invocation.explain_turn = true;
    let mut conv = Conversation::new();
    let outcome = app
        .runs
        .execute_attempt(child_att, &mut agent, &mut conv, invocation, &mut |_| {})
        .await;

    match outcome {
        CandidateOutcome::Failed { message } => {
            assert!(
                message.contains("overprivileged tool host"),
                "expected overprivileged tool host error: {message}"
            );
        }
        other => panic!("expected CandidateOutcome::Failed, got {other:?}"),
    }

    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls on overprivileged host"
    );
    assert_eq!(
        host.tool_calls.load(Ordering::SeqCst),
        0,
        "zero tool calls on overprivileged host"
    );
}

#[tokio::test]
async fn work05_assertion1_child_unauthorized_role_denied() {
    let (app, tmp) = make_app();
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session");

    let _parent = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "parent input".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let child_job = app.runs.child_job(&started.session_id);
    let res = child_job
        .admit_child(ChildAdmit {
            agent_id: "rogue_agent".into(),
            role: Some(RoleId("unauthorized_super_admin".into())),
            job_input: "rogue task".into(),
        })
        .await;

    assert!(
        res.is_err(),
        "child requesting role not in toolset_subscriptions must fail closed"
    );
}

#[tokio::test]
async fn work05_assertion1_unadmitted_artifact_binding_rejected_before_execute() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());

    let (identity, mut spec) = identity_and_spec("test artifact binding");
    spec.artifact_bindings = vec!["art_unadmitted_123".into()];

    let cmd = StartIdentityJobCommand {
        identity,
        job_spec: spec,
        invocation: empty_invocation("test artifact binding"),
    };

    let res = app.runs.start_identity_job(cmd, &mut agent).await;
    assert!(
        res.is_err(),
        "unadmitted artifact binding must fail before execute"
    );
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        0,
        "zero model calls"
    );
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0, "zero tool calls");
}

#[tokio::test]
async fn work05_assertion1_authorized_child_variation_succeeds() {
    let (app, tmp) = make_app();
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session");

    let _parent = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "parent input".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let child_job = app.runs.child_job(&started.session_id);

    // Variation 1: Role "critic" with read-only tools
    let critic_att = child_job
        .admit_child(ChildAdmit {
            agent_id: "auth_critic".into(),
            role: Some(RoleId("critic".into())),
            job_input: "read-only review".into(),
        })
        .await
        .expect("admit critic");

    let provider = Arc::new(CountingProvider::new());
    let critic_host = CountingHost::with_tools(&["read_file", "search_code"]);
    let mut critic_agent = Agent::new(
        provider.clone(),
        critic_host,
        AgentConfig {
            specialist_role: Some("critic".into()),
            ..AgentConfig::default()
        },
    );
    let mut critic_invocation = empty_invocation("read-only review");
    critic_invocation.explain_turn = true;
    let critic_outcome = app
        .runs
        .execute_attempt(
            critic_att.clone(),
            &mut critic_agent,
            &mut Conversation::new(),
            critic_invocation,
            &mut |_| {},
        )
        .await;
    assert!(matches!(critic_outcome, CandidateOutcome::Completed { .. }));
    child_job
        .complete_child(critic_att.clone(), critic_outcome)
        .await
        .unwrap();

    // Variation 2: Role "coder" with mutating tools
    let coder_att = child_job
        .admit_child(ChildAdmit {
            agent_id: "auth_coder".into(),
            role: Some(RoleId("coder".into())),
            job_input: "coder implementation".into(),
        })
        .await
        .expect("admit coder");

    let coder_host = CountingHost::with_tools(&["read_file", "write_file", "edit_file"]);
    let mut coder_agent = Agent::new(
        provider.clone(),
        coder_host,
        AgentConfig {
            specialist_role: Some("coder".into()),
            ..AgentConfig::default()
        },
    );
    let mut coder_invocation = empty_invocation("coder implementation");
    coder_invocation.explain_turn = true;
    let coder_outcome = app
        .runs
        .execute_attempt(
            coder_att.clone(),
            &mut coder_agent,
            &mut Conversation::new(),
            coder_invocation,
            &mut |_| {},
        )
        .await;
    assert!(matches!(coder_outcome, CandidateOutcome::Completed { .. }));
    child_job
        .complete_child(coder_att.clone(), coder_outcome)
        .await
        .unwrap();
}

#[tokio::test]
async fn work05_assertion2_orchestrated_root_critic_revision_production_path() {
    let (app, tmp) = make_app();
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session");

    let parent_turn = RunTurnCommand {
        session_id: started.session_id.clone(),
        user_input: "implement robust parser".into(),
        verify_cmd: Some("cargo test".into()),
        llm_router: Some(false),
    };
    let plan = app.runs.plan_turn(&parent_turn).await.expect("plan_turn");

    let provider = Arc::new(OrchestratedTurnProvider::new());
    let prov_clone = provider.clone();
    let root_att = plan.attempt_id.clone();
    let root_task = plan.task_id.clone();
    let run_id = plan.run_id.clone();

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let mut conversation = Conversation::new();
            let spawn_serial = Arc::new(AtomicU32::new(0));
            let turn_spawn_count = Arc::new(AtomicU32::new(0));

            let outcome = run_orchestrated_turn(
                &mut conversation,
                &OrchestratedTurnInput {
                    user_text: &parent_turn.user_input,
                    orchestration: OrchestrationMode::Auto,
                    critic_enabled: true,
                    verify_gated: true,
                    workspace_root: tmp.path(),
                    index_db: None,
                    code_index: None,
                    pack: &CodingPack,
                    llm_route: None,
                    session_prefers_hard: false,
                    spawn_limits: SpawnLimits::default(),
                    session_max_steps: 10,
                    root_attempt_id: root_att.clone(),
                },
                spawn_serial,
                turn_spawn_count,
                |build, prompt| {
                    let host = if build.role.as_ref().is_some_and(|r| r.as_str() == "critic") {
                        CountingHost::with_tools(&[
                            "read_file",
                            "search_code",
                            "lsp_diagnostics",
                            "finish",
                        ])
                    } else {
                        CountingHost::with_tools(&[
                            "read_file",
                            "write_file",
                            "edit_file",
                            "lsp_diagnostics",
                            "finish",
                        ])
                    };
                    let agent = Agent::new(
                        prov_clone.clone(),
                        host,
                        AgentConfig {
                            specialist_role: build.role.map(|r| r.as_str().to_string()),
                            ..AgentConfig::default()
                        },
                    );
                    let mut invocation = empty_invocation(prompt);
                    invocation.explain_turn = build.explain_turn;
                    Ok((agent, invocation))
                },
                |_aid, _step| {},
                None::<fn(tetonic_orchestrator::RouteDecision, &'static str)>,
                TestRootExecute {
                    runs: app.runs.execution_service(),
                },
                app.runs.child_job(&started.session_id),
            )
            .await
            .expect("orchestrated turn must succeed");

            assert!(
                outcome.outcome.is_completed(),
                "terminal outcome: {:?}",
                outcome.outcome
            );

            assert_eq!(provider.chat_calls.load(Ordering::SeqCst), 5);

            let recorded_tasks = provider.recorded_tasks.lock().unwrap().clone();
            let recorded_attempts = provider.recorded_attempts.lock().unwrap().clone();

            assert_eq!(recorded_tasks[0], Some(root_task.to_string()));
            assert_eq!(recorded_attempts[0], Some(root_att.to_string()));

            let critic_task_id = recorded_tasks[3].clone().expect("critic task id");
            let critic_attempt_id = recorded_attempts[3].clone().expect("critic attempt id");
            assert!(
                critic_task_id.starts_with("task_spawn_"),
                "critic task must be spawned"
            );
            assert_ne!(critic_task_id, root_task.to_string());
            assert_ne!(critic_attempt_id, root_att.to_string());

            let revision_task_id = recorded_tasks[4].clone().expect("revision task id");
            let revision_attempt_id = recorded_attempts[4].clone().expect("revision attempt id");
            assert!(
                revision_task_id.starts_with("task_spawn_"),
                "revision task must be spawned"
            );
            assert_ne!(revision_task_id, critic_task_id);
            assert_ne!(revision_attempt_id, critic_attempt_id);

            let db_path = fs::read_dir(tmp.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.extension().is_some_and(|e| e == "db"))
                .unwrap();
            let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
            let supervisor = tetonic_run::DurableRunSupervisor::new(Some(store));
            use tetonic_run::RunSupervisor;
            let snapshot = supervisor.snapshot(run_id).await.unwrap();

            let c_att = AttemptId::new(critic_attempt_id);
            let r_att = AttemptId::new(revision_attempt_id);

            assert_eq!(snapshot.attempts[&c_att].state, AttemptState::Succeeded);
            assert_eq!(snapshot.attempts[&r_att].state, AttemptState::Succeeded);

            let c_task = snapshot.attempts[&c_att].task_id.clone();
            let r_task = snapshot.attempts[&r_att].task_id.clone();

            assert_eq!(
                snapshot.tasks[&c_task].binding.job_role.as_deref(),
                Some("critic")
            );
            assert_eq!(
                snapshot.tasks[&r_task].binding.job_role.as_deref(),
                Some("coder")
            );

            let c_spec = snapshot.tasks[&c_task].binding.job_spec.as_ref().unwrap();
            let r_spec = snapshot.tasks[&r_task].binding.job_spec.as_ref().unwrap();

            assert_ne!(c_spec.input_digest, r_spec.input_digest);
            assert_ne!(c_spec.recovery_id, r_spec.recovery_id);
        })
        .await;
}

#[tokio::test]
async fn work05_assertion3_sessionless_waiter_drop_neither_abandons_nor_leaks() {
    let (app, _tmp) = make_app();
    let (chat_entered_tx, mut chat_entered_rx) = tokio::sync::mpsc::unbounded_channel();
    let (chat_release_tx, chat_release_rx) = tokio::sync::mpsc::unbounded_channel();

    struct ControlledDelayProvider {
        entered: tokio::sync::mpsc::UnboundedSender<()>,
        release: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<()>>>,
    }
    #[async_trait]
    impl InferenceProvider for ControlledDelayProvider {
        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let _ = self.entered.send(());
            let _ = self.release.lock().await.recv().await;
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![
                    tetonic_inference::ToolCall {
                        function: tetonic_inference::FunctionCall {
                            name: "finish".into(),
                            arguments: serde_json::json!({"summary": "sessionless completed"}),
                        },
                    },
                ]),
                usage: GenUsage::default(),
                provenance: InferenceProvenance::default(),
            })
        }
    }

    let provider = Arc::new(ControlledDelayProvider {
        entered: chat_entered_tx,
        release: Arc::new(tokio::sync::Mutex::new(chat_release_rx)),
    });
    let host = CountingHost::new();
    let agent = make_agent(provider, host);

    let (identity, job_spec) = identity_and_spec("detached sessionless job");
    let cmd = StartIdentityJobCommand {
        identity,
        job_spec,
        invocation: empty_invocation("detached sessionless job"),
    };

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let (attempt_id, rx) = app
                .runs
                .submit_identity_job(cmd, agent)
                .await
                .expect("submit_identity_job");

            let run_id = app.runs.run_id_for_attempt(&attempt_id).expect("run_id");

            drop(rx);

            chat_entered_rx.recv().await.expect("provider entered");
            chat_release_tx.send(()).unwrap();

            let db_path = fs::read_dir(_tmp.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.extension().is_some_and(|e| e == "db"))
                .unwrap();
            let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
            let supervisor = tetonic_run::DurableRunSupervisor::new(Some(store));
            use tetonic_run::RunSupervisor;

            let mut attempts = 0;
            let mut succeeded = false;
            while attempts < 400 {
                tokio::task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(20)).await;
                if let Ok(snap) = supervisor.snapshot(run_id.clone()).await {
                    if let Some(att) = snap.attempts.get(&attempt_id) {
                        if att.state == AttemptState::Succeeded
                            && app.runs.active_job_spec(&attempt_id).is_none()
                        {
                            succeeded = true;
                            break;
                        }
                    }
                }
                attempts += 1;
            }

            assert!(
                succeeded,
                "attempt must reach Succeeded despite waiter drop"
            );
            assert!(
                app.runs.active_job_spec(&attempt_id).is_none(),
                "active registry must remove attempt on completion"
            );
        })
        .await;
}

#[test]
fn work05_invariants_remain_unestablished() {
    let inv_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/architecture/v4/01-INVARIANTS.md");
    if !inv_path.exists() {
        return;
    }
    let text = fs::read_to_string(&inv_path).expect("read 01-INVARIANTS.md");
    assert!(
        !text.contains("INV-V4-WORK-003: ESTABLISHED"),
        "INV-V4-WORK-003 must not be ESTABLISHED before GATE-02"
    );
    assert!(
        !text.contains("INV-V4-ID-001: ESTABLISHED"),
        "INV-V4-ID-001 must not be ESTABLISHED before GATE-02"
    );
    assert!(
        !text.contains("INV-V4-APP-002: ESTABLISHED"),
        "INV-V4-APP-002 must not be ESTABLISHED before GATE-02"
    );
    assert!(
        !text.contains("INV-V4-CMP-001: ESTABLISHED"),
        "INV-V4-CMP-001 must not be ESTABLISHED before GATE-02"
    );
}
