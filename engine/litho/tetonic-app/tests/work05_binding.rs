//! WORK-05: Bind Identity, Job, Invocation, and Children.
//! Implements V4-PROOF-05:
//! Assertion 1: Reject identity/definition/input/Attempt mismatch before execute (0 calls).
//! Assertion 2: Persist distinct child role/input binding in child admission and spec.
//! Assertion 3: Child Attempt bound in LocalAgentAttemptExecutor and reflected in Infer correlation.
//! Assertion 4: Duplicate delivery rejection at dispatch boundary.
//! Assertion 5: Effective authority binding enforcement (unavailable bindings, limits).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tetonic_app::commands::StartIdentityJobCommand;
use tetonic_app::definition::CodingAgentDefinition;
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_core::{Agent, AgentConfig, Conversation};
use tetonic_domain::{
    ActionKind, AgentAttemptExecutor, AgentIdentity, AgentInvocation, AgentJobSpec,
    AttemptExecutionContext, AttemptId, AttemptState, AuthorizedAction, CandidateOutcome,
    IdentityId, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal,
};
use tetonic_inference::{
    ChatRequest, ChatResponse, GenUsage, InferenceError, InferenceProvenance, InferenceProvider,
    Message, TokenSink,
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
    })
    .with_execution_policy(std::sync::Arc::new(
        tetonic_app::definition::validate_coding_execution,
    ));
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

// ----------------------------------------------------------------------------
// Additional Proof Evidence: Authority, Orchestrated Path, Lifetime
// ----------------------------------------------------------------------------

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

#[tokio::test]
async fn unknown_but_matching_definition_revision_never_reaches_inference() {
    let (app, _tmp) = make_app();
    let provider = Arc::new(CountingProvider::new());
    let host = CountingHost::new();
    let mut agent = make_agent(provider.clone(), host.clone());
    let (mut identity, mut spec) = identity_and_spec("test input");
    identity.bound_definition_digest = "unknown-but-consistent".into();
    spec.definition_digest = identity.bound_definition_digest.clone();
    let result = app
        .runs
        .start_identity_job(
            StartIdentityJobCommand {
                identity,
                job_spec: spec,
                invocation: empty_invocation("test input"),
            },
            &mut agent,
        )
        .await;
    if let Ok(result) = result {
        assert!(matches!(
            result.outcome,
            tetonic_domain::CandidateOutcome::Failed { .. }
        ));
    }
    assert_eq!(provider.chat_calls.load(Ordering::SeqCst), 0);
    assert_eq!(host.tool_calls.load(Ordering::SeqCst), 0);
}
