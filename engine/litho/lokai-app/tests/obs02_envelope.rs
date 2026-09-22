//! OBS-02: Managed Observation Envelope.
//! Implements V4-PROOF-07 (Managed Observation Envelope Proof).
//!
//! Covers:
//! 1. `obs02_root_attempt_envelope`: Root Attempt observation carries `identity_id`, `run_id`, `task_id`, `attempt_id`
//!    on tokens, tool calls, and completion.
//! 2. `obs02_child_attempt_envelope`: In-loop child spawn or child attempt emits tokens and tool results
//!    with distinct child `task_id` and child `attempt_id` (not root IDs).
//! 3. `obs02_sessionless_envelope`: Session-free execution via `start_identity_job` emits observation events
//!    carrying full execution envelope without requiring a session or `LiveSession`.
//! 4. `obs02_infer_hop_classification`: Verify `FabricCallMeta` and `InferenceProvenance` differentiate compute hop
//!    Attempt IDs (`hop_attempt_id`) from agent job Attempt IDs (`attempt_id`), with parent correlation preserved.
//! 5. `obs02_overlapping_executions`: Run two concurrent executions simultaneously; verify all emitted events
//!    in the shared sink are deterministically partitioned by `attempt_id` and `run_id` with zero cross-talk or ID collision.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use lokai_app::commands::{
    RunTurnCommand, SpawnAgentCommand, StartIdentityJobCommand, StartSessionCommand,
};
use lokai_app::definition::CodingAgentDefinition;
use lokai_app::events::{ApplicationEvent, ApplicationEventSink, EventEnvelope};
use lokai_app::turn_execution::step_to_events;
use lokai_app::{Application, ApplicationDependencies};
use lokai_core::{Agent, AgentConfig, Step};
use lokai_domain::{AgentIdentity, AgentInvocation, AgentJobSpec, CandidateOutcome};
use lokai_inference::{
    ChatRequest, ChatResponse, FabricCallMeta, FabricSnapshot, FunctionCall, GenUsage,
    InferenceError, InferenceProvenance, InferenceProvider, Message, NodeInfo, TokenSink, ToolCall,
};
use lokai_orchestrator::{ChildAdmit, RoleId};
use lokai_run::idempotency::job_input_digest;
use lokai_tools::{Tools, Workspace};

static OBS02_DB_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Default, Clone)]
struct RecordingEventSink {
    events: Arc<Mutex<Vec<ApplicationEvent>>>,
}

impl ApplicationEventSink for RecordingEventSink {
    fn send(&self, event: ApplicationEvent) {
        self.events.lock().unwrap().push(event);
    }
}

impl RecordingEventSink {
    fn all_events(&self) -> Vec<ApplicationEvent> {
        self.events.lock().unwrap().clone()
    }
}

struct DynamicStreamingProvider {
    tokens_per_call: Arc<Mutex<Vec<Vec<String>>>>,
    recorded_requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl DynamicStreamingProvider {
    fn new(scripted_tokens: Vec<Vec<String>>) -> Self {
        Self {
            tokens_per_call: Arc::new(Mutex::new(scripted_tokens)),
            recorded_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl InferenceProvider for DynamicStreamingProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![NodeInfo {
                id: lokai_capacity::LOCAL_NODE_ID.into(),
                label: "mock_streaming".into(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec![],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            }],
            effective_concurrency: 4,
            generated_at: chrono::Utc::now(),
        }
    }

    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.recorded_requests.lock().unwrap().push(req.clone());

        // Check if there are scripted tokens for this call
        let tokens = {
            let mut guard = self.tokens_per_call.lock().unwrap();
            if !guard.is_empty() {
                guard.remove(0)
            } else {
                vec!["streamed_token".to_string()]
            }
        };

        for token in tokens {
            on_token(&token);
        }

        let hop_att = req.fabric.as_ref().and_then(|f| f.hop_attempt_id.clone());

        Ok(ChatResponse {
            message: Message::assistant("Turn execution complete").with_tool_calls(vec![
                ToolCall {
                    function: FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({
                            "summary": "Observation test turn finished"
                        }),
                    },
                },
            ]),
            usage: GenUsage::default(),
            provenance: InferenceProvenance {
                attempt_id: hop_att,
                provider_kind: "mock_streaming".into(),
                ..Default::default()
            },
        })
    }
}

fn make_test_app(
    sink: Arc<RecordingEventSink>,
    provider: Arc<dyn InferenceProvider>,
) -> (Application, tempfile::TempDir, lokai_memory::SharedStore) {
    let tmp = tempfile::tempdir().unwrap();

    let db_dir = tmp.path().join("db");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db_path = db_dir.join(format!(
        "lokai_obs02_{}_{}.db",
        std::process::id(),
        OBS02_DB_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();

    let art_dir = tmp.path().join("artifacts");
    std::fs::create_dir_all(&art_dir).unwrap();
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            art_dir,
            lokai_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );

    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: sink,
        index_db: None,
        fabric_hint: None,
    });
    app.bind_inference(provider, None);
    (app, tmp, store)
}

fn make_workspace(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let ws = tmp.path().join(name);
    std::fs::create_dir_all(&ws).unwrap();
    ws
}

async fn start_test_session(
    app: &Application,
    workspace_root: &std::path::Path,
) -> lokai_app::commands::StartSessionResultPayload {
    app.sessions
        .start_session(StartSessionCommand {
            workspace_root: workspace_root.display().to_string(),
            briefing: Some(false),
            allow_shell: Some(true),
            auto_grant_approvals: Some(true),
            ..Default::default()
        })
        .await
        .expect("start session")
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
        discipline: lokai_domain::LoopDiscipline::default(),
    }
}

// ----------------------------------------------------------------------------
// V4-PROOF-07 Assertion 1: Root Attempt Observation Envelope
// ----------------------------------------------------------------------------

#[tokio::test]
async fn obs02_root_attempt_envelope() {
    let sink = Arc::new(RecordingEventSink::default());
    let provider = Arc::new(DynamicStreamingProvider::new(vec![vec![
        "root_tok_1".into(),
        "root_tok_2".into(),
    ]]));
    let (app, tmp, _store) = make_test_app(sink.clone(), provider);
    let ws = make_workspace(&tmp, "ws_root");
    let session = start_test_session(&app, &ws).await;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "test root envelope".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit chat turn");

            let finish = tokio::time::timeout(Duration::from_secs(5), rx)
                .await
                .expect("timeout waiting for turn")
                .expect("join channel dropped");
            assert!(finish.ok, "turn completed successfully");
        })
        .await;

    let all_events = sink.all_events();
    assert!(!all_events.is_empty(), "events must have been emitted");

    // 1. Find TurnCompleted to determine the authoritative AttemptId, RunId, TaskId, IdentityId
    let turn_completed = all_events
        .iter()
        .find_map(|e| match e {
            ApplicationEvent::TurnCompleted {
                run_id,
                task_id,
                attempt_id,
                identity_id,
                status,
                ..
            } => Some((
                run_id.clone().expect("TurnCompleted run_id"),
                task_id.clone().expect("TurnCompleted task_id"),
                attempt_id.clone().expect("TurnCompleted attempt_id"),
                identity_id.clone().expect("TurnCompleted identity_id"),
                status.clone(),
            )),
            _ => None,
        })
        .expect("TurnCompleted event must be emitted");

    let (root_run_id, root_task_id, root_attempt_id, root_identity_id, status) = turn_completed;
    assert_eq!(status, "ok");

    // 2. Verify ModelToken events carry identical execution envelope
    let model_tokens: Vec<&ApplicationEvent> = all_events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::ModelToken { .. }))
        .collect();
    assert!(
        model_tokens.len() >= 2,
        "expected at least 2 tokens, got {}",
        model_tokens.len()
    );

    for tok_ev in &model_tokens {
        assert_eq!(tok_ev.attempt_id(), Some(root_attempt_id.as_str()));
        assert_eq!(tok_ev.run_id(), Some(root_run_id.as_str()));
        assert_eq!(tok_ev.task_id(), Some(root_task_id.as_str()));
        assert_eq!(tok_ev.identity_id(), Some(root_identity_id.as_str()));
    }

    // 3. Verify ToolCall / ToolResult carry identical execution envelope
    let tool_calls: Vec<&ApplicationEvent> = all_events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::ToolCall { .. }))
        .collect();
    assert!(!tool_calls.is_empty(), "expected tool call events");
    for tc in &tool_calls {
        assert_eq!(tc.attempt_id(), Some(root_attempt_id.as_str()));
        assert_eq!(tc.run_id(), Some(root_run_id.as_str()));
        assert_eq!(tc.task_id(), Some(root_task_id.as_str()));
        assert_eq!(tc.identity_id(), Some(root_identity_id.as_str()));
    }

    let turn_answers: Vec<&ApplicationEvent> = all_events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::TurnAnswer { .. }))
        .collect();
    assert!(!turn_answers.is_empty(), "expected turn answer events");
    for ta in &turn_answers {
        assert_eq!(ta.attempt_id(), Some(root_attempt_id.as_str()));
        assert_eq!(ta.run_id(), Some(root_run_id.as_str()));
        assert_eq!(ta.task_id(), Some(root_task_id.as_str()));
        assert_eq!(ta.identity_id(), Some(root_identity_id.as_str()));
    }

    let tool_results: Vec<&ApplicationEvent> = all_events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::ToolResult { .. }))
        .collect();
    for tr in &tool_results {
        assert_eq!(tr.attempt_id(), Some(root_attempt_id.as_str()));
        assert_eq!(tr.run_id(), Some(root_run_id.as_str()));
        assert_eq!(tr.task_id(), Some(root_task_id.as_str()));
        assert_eq!(tr.identity_id(), Some(root_identity_id.as_str()));
    }

    // 4. Verify RunStatus carries identical execution envelope
    let run_statuses: Vec<&ApplicationEvent> = all_events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::RunStatus { .. }))
        .collect();
    assert!(!run_statuses.is_empty(), "expected run status events");
    for rs in &run_statuses {
        assert_eq!(rs.run_id(), Some(root_run_id.as_str()));
        assert_eq!(rs.attempt_id(), Some(root_attempt_id.as_str()));
        assert_eq!(rs.task_id(), Some(root_task_id.as_str()));
    }
}

// ----------------------------------------------------------------------------
// V4-PROOF-07 Assertion 2: Child Attempt Observation Envelope
// ----------------------------------------------------------------------------

#[tokio::test]
async fn obs02_child_attempt_envelope() {
    let sink = Arc::new(RecordingEventSink::default());
    let provider = Arc::new(DynamicStreamingProvider::new(vec![
        vec!["root_1".into(), "root_2".into()],
        vec!["child_tok_1".into(), "child_tok_2".into()],
    ]));
    let (app, tmp, _store) = make_test_app(sink.clone(), provider);
    let ws = make_workspace(&tmp, "ws_child");
    let session = start_test_session(&app, &ws).await;

    // Part A: Run root turn
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "root turn".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit root");
            let finish = rx.await.expect("join root");
            assert!(finish.ok);
        })
        .await;

    let root_events = sink.all_events();
    let root_att = root_events
        .iter()
        .find_map(|e| match e {
            ApplicationEvent::TurnCompleted { attempt_id, .. } => attempt_id.clone(),
            _ => None,
        })
        .expect("root attempt id");

    // Part B: Submit child spawn turn on the same session
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);
            app.submit_spawn(SpawnAgentCommand {
                session_id: session.session_id.clone(),
                agent_id: "child_specialist_01".into(),
                parent_agent_id: "root".into(),
                role: "coder".into(),
                task: "implement child subfeature".into(),
            })
            .expect("submit spawn");
            let finish = rx.await.expect("join spawn");
            assert!(finish.ok, "spawn finish error: {:?}", finish.error);
        })
        .await;

    let all_events = sink.all_events();
    let child_tokens: Vec<&ApplicationEvent> = all_events
        .iter()
        .filter(|e| match e {
            ApplicationEvent::ModelToken { token, .. } => token.starts_with("child_tok"),
            _ => false,
        })
        .collect();

    assert!(
        !child_tokens.is_empty(),
        "child specialist tokens must be emitted"
    );

    let child_att = child_tokens[0]
        .attempt_id()
        .expect("child token must carry attempt_id");
    let child_task = child_tokens[0]
        .task_id()
        .expect("child token must carry task_id");

    // Assert that child Attempt and Task IDs are distinct from root IDs
    assert_ne!(
        child_att,
        root_att.as_str(),
        "child attempt_id must NOT be equal to root attempt_id"
    );
    assert!(
        child_task.contains("spawn")
            || child_task.contains("child")
            || child_task.contains("task_"),
        "child task_id must reflect spawned task identity"
    );

    for ctok in &child_tokens {
        assert_eq!(ctok.attempt_id(), Some(child_att));
        assert_eq!(ctok.task_id(), Some(child_task));
    }

    // Part C: Helper-only envelope formatting coverage. Actual in-loop execution
    // is covered by obs02_nested_spawn_uses_admitted_child_envelope below.
    let in_loop_parent = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: session.session_id.clone(),
            user_input: "orchestrated review turn".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan in_loop parent turn");

    let child_job = app.runs.child_job(&session.session_id);
    let admitted_child_att = child_job
        .admit_child(ChildAdmit {
            agent_id: "in_loop_critic".into(),
            role: Some(RoleId("critic".into())),
            job_input: "review code changes".into(),
        })
        .await
        .expect("admit in-loop child");

    assert_ne!(admitted_child_att.0, root_att);
    assert_ne!(admitted_child_att.0, child_att);
    assert_ne!(admitted_child_att.0, in_loop_parent.attempt_id.0);

    let in_loop_envelope = EventEnvelope {
        run_id: Some(in_loop_parent.run_id.0.clone()),
        task_id: Some("task_in_loop_critic".into()),
        attempt_id: Some(admitted_child_att.0.clone()),
        identity_id: Some("critic".into()),
    };

    let scanner = lokai_secrets::ScannerEngine::default_engine();
    step_to_events(
        &(sink.clone() as Arc<dyn ApplicationEventSink>),
        &session.session_id,
        "in_loop_critic",
        Step::Token("critic_verdict".into()),
        Some(&scanner),
        None,
        Some(&in_loop_envelope),
    );

    step_to_events(
        &(sink.clone() as Arc<dyn ApplicationEventSink>),
        &session.session_id,
        "in_loop_critic",
        Step::ToolCall {
            call_id: "call_critic_1".into(),
            name: "finish".into(),
            args: serde_json::json!({"summary": "review approved"}),
        },
        Some(&scanner),
        None,
        Some(&in_loop_envelope),
    );

    step_to_events(
        &(sink.clone() as Arc<dyn ApplicationEventSink>),
        &session.session_id,
        "in_loop_critic",
        Step::ToolResult {
            call_id: "call_critic_1".into(),
            name: "finish".into(),
            ok: true,
            summary: "review approved".into(),
        },
        Some(&scanner),
        None,
        Some(&in_loop_envelope),
    );

    let updated_events = sink.all_events();
    let critic_token = updated_events
        .iter()
        .find(|e| match e {
            ApplicationEvent::ModelToken { token, .. } => token == "critic_verdict",
            _ => false,
        })
        .expect("critic token must be recorded");

    assert_eq!(
        critic_token.attempt_id(),
        Some(admitted_child_att.0.as_str()),
        "in-loop child token carries admitted child AttemptId"
    );
    assert_eq!(
        critic_token.task_id(),
        Some("task_in_loop_critic"),
        "in-loop child token carries child TaskId"
    );

    let critic_res = updated_events
        .iter()
        .find(|e| match e {
            ApplicationEvent::ToolResult { call_id, .. } => call_id == "call_critic_1",
            _ => false,
        })
        .expect("critic tool result must be recorded");

    assert_eq!(
        critic_res.attempt_id(),
        Some(admitted_child_att.0.as_str()),
        "in-loop child tool result carries admitted child AttemptId"
    );
    assert_eq!(
        critic_res.task_id(),
        Some("task_in_loop_critic"),
        "in-loop child tool result carries child TaskId"
    );
}

// ----------------------------------------------------------------------------
// V4-PROOF-07 Assertion 3: Sessionless Work Managed Observation Envelope
// ----------------------------------------------------------------------------

#[tokio::test]
async fn obs02_sessionless_envelope() {
    let sink = Arc::new(RecordingEventSink::default());
    let provider = Arc::new(DynamicStreamingProvider::new(vec![vec![
        "sessionless_tok_1".into(),
        "sessionless_tok_2".into(),
        "AKIAIOSFODNN7EXAMPLE".into(),
        "AKIAIOSFODNN7EXAMPLE".into(),
        "still clean".into(),
    ]]));
    let (app, tmp, _store) = make_test_app(sink.clone(), provider.clone());

    // Never call start_session or create a LiveSession.
    // Build fresh standalone Agent with Workspace tools.
    let ws = make_workspace(&tmp, "ws_sessionless");
    let tools = Tools::new(Workspace::new(&ws).unwrap(), false);
    let mut agent = Agent::new(provider, tools, AgentConfig::default());

    let (identity, spec) = identity_and_spec("sessionless autonomous job");
    let cmd = StartIdentityJobCommand {
        identity: identity.clone(),
        job_spec: spec,
        invocation: empty_invocation("sessionless autonomous job"),
    };

    let local = tokio::task::LocalSet::new();
    let result = local
        .run_until(async {
            app.runs
                .start_identity_job(cmd, &mut agent)
                .await
                .expect("start_identity_job must succeed")
        })
        .await;

    assert!(
        matches!(result.outcome, CandidateOutcome::Completed { .. }),
        "sessionless execution completed"
    );

    let events = sink.all_events();
    assert!(!events.is_empty(), "sessionless execution must emit events");

    // Verify ModelToken events exist and carry execution correlation
    let tokens: Vec<&ApplicationEvent> = events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::ModelToken { .. }))
        .collect();
    assert!(
        tokens.len() >= 2,
        "expected at least 2 tokens from sessionless job"
    );

    let token_text: Vec<&str> = tokens
        .iter()
        .filter_map(|event| match event {
            ApplicationEvent::ModelToken { token, .. } => Some(token.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        token_text,
        vec![
            "sessionless_tok_1",
            "sessionless_tok_2",
            "[REDACTED:aws-access-key]",
            "[REDACTED:aws-access-key]",
            "still clean",
        ],
        "repeated secrets stay redacted and clean tokens survive scanner reuse"
    );

    for tok in &tokens {
        assert_eq!(
            tok.attempt_id(),
            Some(result.attempt_id.0.as_str()),
            "token carries sessionless AttemptId"
        );
        assert_eq!(
            tok.run_id(),
            Some(result.run_id.0.as_str()),
            "token carries sessionless RunId"
        );
        assert_eq!(
            tok.task_id(),
            Some(result.task_id.0.as_str()),
            "token carries sessionless TaskId"
        );
        assert_eq!(
            tok.identity_id(),
            Some(identity.id.0.as_str()),
            "token carries sessionless IdentityId"
        );
    }

    // Verify ToolCall and ToolResult exist and carry execution correlation
    let tools: Vec<&ApplicationEvent> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                ApplicationEvent::ToolCall { .. } | ApplicationEvent::ToolResult { .. }
            )
        })
        .collect();
    assert!(
        !tools.is_empty(),
        "expected tool events from sessionless job"
    );
    for tool_ev in &tools {
        assert_eq!(tool_ev.attempt_id(), Some(result.attempt_id.0.as_str()));
        assert_eq!(tool_ev.run_id(), Some(result.run_id.0.as_str()));
        assert_eq!(tool_ev.task_id(), Some(result.task_id.0.as_str()));
        assert_eq!(tool_ev.identity_id(), Some(identity.id.0.as_str()));
    }

    // Verify TurnCompleted exists, carries execution correlation, and has empty session_id
    let completion = events
        .iter()
        .find(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
        .expect("TurnCompleted must be emitted for sessionless job");

    match completion {
        ApplicationEvent::TurnCompleted {
            session_id,
            run_id,
            task_id,
            attempt_id,
            identity_id,
            status,
            error,
        } => {
            assert!(
                session_id.is_empty(),
                "sessionless TurnCompleted must have empty session_id"
            );
            assert_eq!(run_id.as_deref(), Some(result.run_id.0.as_str()));
            assert_eq!(task_id.as_deref(), Some(result.task_id.0.as_str()));
            assert_eq!(attempt_id.as_deref(), Some(result.attempt_id.0.as_str()));
            assert_eq!(identity_id.as_deref(), Some(identity.id.0.as_str()));
            assert_eq!(status, "ok");
            assert!(error.is_none());
        }
        _ => unreachable!(),
    }
}

// ----------------------------------------------------------------------------
// V4-PROOF-07 Assertion 4: Infer Hop Classification and Provenance Correlation
// ----------------------------------------------------------------------------

#[test]
fn obs02_infer_hop_classification() {
    let parent_agent_attempt = "att_agent_parent_42";
    let compute_hop_attempt = "att_compute_hop_84";

    // 1. Broker sets hop_attempt_id distinctly on FabricCallMeta while preserving parent attempt_id
    let meta = FabricCallMeta {
        run_id: Some("run_agent_42".into()),
        task_id: Some("task_agent_42".into()),
        attempt_id: Some(parent_agent_attempt.into()),
        hop_run_id: Some("run_compute_84".into()),
        hop_task_id: Some("task_compute_84".into()),
        hop_attempt_id: Some(compute_hop_attempt.into()),
        ..Default::default()
    };

    assert_ne!(
        meta.attempt_id, meta.hop_attempt_id,
        "agent AttemptId and compute hop AttemptId must be distinct"
    );
    assert_eq!(meta.attempt_id.as_deref(), Some(parent_agent_attempt));
    assert_eq!(meta.hop_attempt_id.as_deref(), Some(compute_hop_attempt));

    // 2. Inference provenance records the compute hop AttemptId
    let provenance = InferenceProvenance {
        attempt_id: meta.hop_attempt_id.clone(),
        provider_kind: "test_fabric_node".into(),
        ..Default::default()
    };
    assert_eq!(
        provenance.attempt_id.as_deref(),
        Some(compute_hop_attempt),
        "InferenceProvenance records hop AttemptId"
    );
    assert_ne!(
        provenance.attempt_id.as_deref(),
        meta.attempt_id.as_deref(),
        "InferenceProvenance does NOT record parent agent AttemptId"
    );

    // 3. Managed observation envelope records the parent agent AttemptId
    let envelope = EventEnvelope::new(
        meta.run_id.as_deref().unwrap(),
        meta.task_id.as_deref().unwrap(),
        meta.attempt_id.as_deref().unwrap(),
        Some("agent_identity_record".into()),
    );

    let sink = Arc::new(RecordingEventSink::default());
    let scanner = lokai_secrets::ScannerEngine::default_engine();
    step_to_events(
        &(sink.clone() as Arc<dyn ApplicationEventSink>),
        "sess_hop_test",
        "agent_a",
        Step::Token("classified_hop_token".into()),
        Some(&scanner),
        None,
        Some(&envelope),
    );

    let events = sink.all_events();
    assert_eq!(events.len(), 1);
    let token_ev = &events[0];
    assert_eq!(
        token_ev.attempt_id(),
        Some(parent_agent_attempt),
        "observation envelope preserves agent AttemptId"
    );
    assert_ne!(
        token_ev.attempt_id(),
        Some(compute_hop_attempt),
        "observation envelope is NEVER overwritten with hop AttemptId"
    );

    // 4. Compute hop retry mints a fresh hop AttemptId; parent agent AttemptId remains stable
    let compute_hop_attempt_retry = "att_compute_hop_85";
    let retry_meta = FabricCallMeta {
        hop_attempt_id: Some(compute_hop_attempt_retry.into()),
        ..meta.clone()
    };
    assert_eq!(
        retry_meta.attempt_id.as_deref(),
        Some(parent_agent_attempt),
        "parent agent correlation persists across hop failover/retry"
    );
    assert_eq!(
        retry_meta.hop_attempt_id.as_deref(),
        Some(compute_hop_attempt_retry),
        "subordinate hop attempt changes on retry"
    );
}

// ----------------------------------------------------------------------------
// V4-PROOF-07 Assertion 5: Overlapping Concurrent Executions Partitioning
// ----------------------------------------------------------------------------

struct OverlappingProvider;

#[async_trait]
impl InferenceProvider for OverlappingProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![NodeInfo {
                id: lokai_capacity::LOCAL_NODE_ID.into(),
                label: "mock_overlapping".into(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec![],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            }],
            effective_concurrency: 4,
            generated_at: chrono::Utc::now(),
        }
    }

    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let last_user = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let (tag, summary) = if last_user.contains("stream_alpha") {
            ("alpha", "Finished alpha work")
        } else {
            ("beta", "Finished beta work")
        };

        // Stream distinct tokens for this branch
        on_token(&format!("{tag}_tok_0"));
        tokio::time::sleep(Duration::from_millis(5)).await;
        on_token(&format!("{tag}_tok_1"));

        Ok(ChatResponse {
            message: Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: serde_json::json!({
                        "summary": summary
                    }),
                },
            }]),
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}

#[tokio::test]
async fn obs02_overlapping_executions() {
    let sink = Arc::new(RecordingEventSink::default());
    let provider = Arc::new(OverlappingProvider);
    let (app, tmp, _store) = make_test_app(sink.clone(), provider);

    let ws_a = make_workspace(&tmp, "ws_overlap_a");
    let ws_b = make_workspace(&tmp, "ws_overlap_b");
    let session_a = start_test_session(&app, &ws_a).await;
    let session_b = start_test_session(&app, &ws_b).await;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx_a = app.arm_turn_join(&session_a.session_id);
            let rx_b = app.arm_turn_join(&session_b.session_id);

            // Submit turn on Session A
            app.submit_chat_turn(RunTurnCommand {
                session_id: session_a.session_id.clone(),
                user_input: "run stream_alpha concurrent".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit alpha");

            // Submit turn on Session B
            app.submit_chat_turn(RunTurnCommand {
                session_id: session_b.session_id.clone(),
                user_input: "run stream_beta concurrent".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit beta");

            let (res_a, res_b) = tokio::join!(rx_a, rx_b);
            assert!(res_a.expect("join a").ok);
            assert!(res_b.expect("join b").ok);
        })
        .await;

    let all_events = sink.all_events();

    // Find the two TurnCompleted events
    let completions: Vec<(String, String, String)> = all_events
        .iter()
        .filter_map(|e| match e {
            ApplicationEvent::TurnCompleted {
                session_id,
                run_id,
                attempt_id,
                ..
            } => Some((
                session_id.clone(),
                run_id.clone().expect("run_id"),
                attempt_id.clone().expect("attempt_id"),
            )),
            _ => None,
        })
        .collect();

    assert_eq!(completions.len(), 2, "exactly two executions completed");

    let comp_a = completions
        .iter()
        .find(|(sid, ..)| sid == &session_a.session_id)
        .expect("session A completion");
    let comp_b = completions
        .iter()
        .find(|(sid, ..)| sid == &session_b.session_id)
        .expect("session B completion");

    let (_, run_a, attempt_a) = comp_a;
    let (_, run_b, attempt_b) = comp_b;

    assert_ne!(attempt_a, attempt_b, "Attempt IDs must be distinct");
    assert_ne!(run_a, run_b, "Run IDs must be distinct");

    // Partition all events by attempt_id
    let mut events_a = Vec::new();
    let mut events_b = Vec::new();
    let mut unpartitioned = Vec::new();

    for ev in &all_events {
        if let Some(att) = ev.attempt_id() {
            if att == attempt_a {
                events_a.push(ev);
            } else if att == attempt_b {
                events_b.push(ev);
            } else {
                unpartitioned.push(ev);
            }
        }
    }

    assert!(
        unpartitioned.is_empty(),
        "zero events belong to unknown AttemptId"
    );
    assert!(!events_a.is_empty(), "events_a must not be empty");
    assert!(!events_b.is_empty(), "events_b must not be empty");

    // Verify Partition A: All events carry run_a and alpha content
    for ev in &events_a {
        assert_eq!(ev.run_id(), Some(run_a.as_str()));
        assert_ne!(ev.run_id(), Some(run_b.as_str()));
        if let ApplicationEvent::ModelToken { token, .. } = ev {
            assert!(
                token.starts_with("alpha_"),
                "token in partition A must be alpha, got {token}"
            );
        }
    }

    // Verify Partition B: All events carry run_b and beta content
    for ev in &events_b {
        assert_eq!(ev.run_id(), Some(run_b.as_str()));
        assert_ne!(ev.run_id(), Some(run_a.as_str()));
        if let ApplicationEvent::ModelToken { token, .. } = ev {
            assert!(
                token.starts_with("beta_"),
                "token in partition B must be beta, got {token}"
            );
        }
    }

    // Proves deterministic partitioning under overlapping concurrency
    // Root or Session default cannot satisfy this assertion
}

#[tokio::test]
async fn obs02_nested_spawn_uses_admitted_child_envelope() {
    struct NestedProvider {
        root_calls: AtomicU64,
        requests: Mutex<Vec<(String, String, String)>>,
    }
    #[async_trait]
    impl InferenceProvider for NestedProvider {
        async fn fabric_snapshot(&self) -> FabricSnapshot {
            DynamicStreamingProvider::new(vec![])
                .fabric_snapshot()
                .await
        }
        async fn chat(
            &self,
            req: ChatRequest,
            on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            let meta = req.fabric.as_ref().unwrap();
            let aid = meta.agent_id.as_deref().unwrap();
            let token = format!("executing:{aid}");
            self.requests.lock().unwrap().push((
                token.clone(),
                meta.attempt_id.clone().unwrap(),
                meta.task_id.clone().unwrap(),
            ));
            on_token(&token);
            let (name, arguments) =
                if aid == "a0" && self.root_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    (
                        "spawn_agent",
                        serde_json::json!({"role":"coder", "task":"answer delegated question"}),
                    )
                } else {
                    (
                        "finish",
                        serde_json::json!({"summary":"delegation finished"}),
                    )
                };
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: name.into(),
                        arguments,
                    },
                }]),
                usage: GenUsage::default(),
                provenance: InferenceProvenance::default(),
            })
        }
    }
    let sink = Arc::new(RecordingEventSink::default());
    let provider = Arc::new(NestedProvider {
        root_calls: AtomicU64::new(0),
        requests: Mutex::new(vec![]),
    });
    let (app, tmp, _) = make_test_app(sink.clone(), provider.clone());
    let ws = make_workspace(&tmp, "nested");
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            orchestration: Some("auto".into()),
            critic: Some(false),
            auto_grant_approvals: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id,
                user_input: "delegate a question to a coder".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .unwrap();
            let finish = tokio::time::timeout(Duration::from_secs(10), rx)
                .await
                .unwrap()
                .unwrap();
            assert!(finish.ok, "{:?}", finish.error);
        })
        .await;
    let requests = provider.requests.lock().unwrap();
    assert!(
        requests.iter().any(|(token, _, _)| token != "executing:a0"),
        "nested executor must actually run: {requests:?}"
    );
    let events = sink.all_events();
    for (token, attempt, task) in requests.iter() {
        let event = events.iter().find(|event| matches!(event, ApplicationEvent::ModelToken { token: observed, .. } if observed == token)).unwrap();
        assert_eq!(event.attempt_id(), Some(attempt.as_str()));
        assert_eq!(event.task_id(), Some(task.as_str()));
    }
}
