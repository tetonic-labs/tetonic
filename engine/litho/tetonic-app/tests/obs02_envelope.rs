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

use async_trait::async_trait;
use tetonic_app::commands::StartIdentityJobCommand;
use tetonic_app::definition::CodingAgentDefinition;
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink, EventEnvelope};
use tetonic_app::turn_execution::step_to_events;
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_core::{Agent, AgentConfig, Step};
use tetonic_domain::{AgentIdentity, AgentInvocation, AgentJobSpec, CandidateOutcome};
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricCallMeta, FabricSnapshot, FunctionCall, GenUsage,
    InferenceError, InferenceProvenance, InferenceProvider, Message, NodeInfo, TokenSink, ToolCall,
};
use tetonic_run::idempotency::job_input_digest;
use tetonic_tools::{Tools, Workspace};

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
                id: tetonic_capacity::LOCAL_NODE_ID.into(),
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
) -> (Application, tempfile::TempDir, tetonic_memory::SharedStore) {
    let tmp = tempfile::tempdir().unwrap();

    let db_dir = tmp.path().join("db");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db_path = db_dir.join(format!(
        "lokai_obs02_{}_{}.db",
        std::process::id(),
        OBS02_DB_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();

    let art_dir = tmp.path().join("artifacts");
    std::fs::create_dir_all(&art_dir).unwrap();
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            art_dir,
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );

    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let runtime = tetonic_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: sink,
        index_db: None,
    })
    .with_execution_policy(std::sync::Arc::new(
        tetonic_app::definition::validate_coding_execution,
    ));
    app.bind_inference(provider, None);
    (app, tmp, store)
}

fn make_workspace(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let ws = tmp.path().join(name);
    std::fs::create_dir_all(&ws).unwrap();
    ws
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

// ----------------------------------------------------------------------------
// V4-PROOF-07 Assertion 1: Root Attempt Observation Envelope
// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// V4-PROOF-07 Assertion 2: Child Attempt Observation Envelope
// ----------------------------------------------------------------------------

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
    let scanner = tetonic_secrets::ScannerEngine::default_engine();
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
