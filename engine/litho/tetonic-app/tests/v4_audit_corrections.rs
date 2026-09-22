//! V4 Audit Corrections Tests (2026-09-11 Audit)
//!
//! Covers:
//! 1. Defect 1: Chat reports failure when verification durably fails (e2e agreement
//!    between durable state, event status/error, and product join result).
//! 2. Defect 1: Commit failure propagation across complete_turn, events, and durable state.
//! 3. Defect 1: Lease loss during finalization propagation across complete_turn, events, and durable state.
//! 4. Defect 2: Nested child spawn acquires broker approval hook under child attempt ID,
//!    receives approval prompt, authorizes execution, and removes hook on completion.
//! 5. Defect 2: Standalone spawn acquires broker approval hook under spawned attempt ID,
//!    receives approval prompt, authorizes execution, and removes hook on completion.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tetonic_app::commands::{
    ApprovalResponseCommand, CompleteTurnCommand, RunTurnCommand, SpawnAgentCommand,
    StartSessionCommand, TurnFinish,
};
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::services::{
    DefaultRunService, FinalizationEffectDriver, FinalizationPolicy, RunService,
};
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_domain::ids::{ArtifactId, AttemptId, TransactionId};
use tetonic_domain::{
    AttemptState, CandidateOutcome, ClaimFinalization, CommitResult, CompletionKind, ContentDigest,
    DataClass, ExecutionTargetId, FailureClass, LeaseAttempt, LeaseId, LeaseProof, RepositoryId,
    RunCommand, RunState, StartAttempt, TaskId, TransactionArtifact, WorkspaceVersion,
    WorkspaceVersionScheme,
};
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, FunctionCall, GenUsage, InferenceError,
    InferenceProvenance, InferenceProvider, Message, NodeInfo, TokenSink, ToolCall,
};
use tetonic_run::{command_envelope, DurableRunSupervisor, RunSupervisor};

static DB_SEQ: AtomicU64 = AtomicU64::new(0);

struct TestApprovalSink {
    events: Mutex<Vec<ApplicationEvent>>,
    approval_tx: tokio::sync::mpsc::UnboundedSender<ApplicationEvent>,
}

impl ApplicationEventSink for TestApprovalSink {
    fn send(&self, event: ApplicationEvent) {
        eprintln!("[SINK EVENT] {:?}", event);
        if matches!(event, ApplicationEvent::ApprovalRequest { .. }) {
            let _ = self.approval_tx.send(event.clone());
        }
        self.events.lock().unwrap().push(event);
    }
}

impl TestApprovalSink {
    fn all_events(&self) -> Vec<ApplicationEvent> {
        self.events.lock().unwrap().clone()
    }
}

fn dummy_workspace_version() -> WorkspaceVersion {
    WorkspaceVersion {
        repository_id: RepositoryId::new("repo_v4_audit"),
        version_scheme: WorkspaceVersionScheme::Manifest,
        git_head: None,
        dirty_state_digest: ContentDigest::new("sha256:dirty"),
        tracked_state_digest: ContentDigest::new("sha256:tracked"),
        relevant_path_digests: BTreeMap::new(),
        index_generation: None,
    }
}

fn dummy_commit_result() -> CommitResult {
    let base = dummy_workspace_version();
    let result = dummy_workspace_version();
    CommitResult {
        transaction_id: TransactionId::new("tx_v4_audit"),
        base_version: base.clone(),
        result_version: result.clone(),
        patch_digest: ContentDigest::new("sha256:commit_v4"),
        artifact: TransactionArtifact {
            transaction_id: TransactionId::new("tx_v4_audit"),
            base_workspace_version: base,
            result_workspace_version: result,
            patch_artifact_id: ArtifactId::new("art_patch_v4"),
            verification_artifact_id: None,
            task_id: None,
            attempt_id: None,
            data_class: DataClass::RepositorySource,
            commit_succeeded: true,
        },
    }
}

type VerifyResult = Result<(), (String, Option<String>)>;
type CommitOutcome = Result<Option<CommitResult>, String>;

#[derive(Default)]
struct MockEffectDriver {
    calls: Arc<Mutex<Vec<String>>>,
    bind_count: AtomicUsize,
    verify_count: AtomicUsize,
    commit_count: AtomicUsize,
    verify_result: Mutex<Option<VerifyResult>>,
    commit_result: Mutex<Option<CommitOutcome>>,
}

impl MockEffectDriver {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            bind_count: AtomicUsize::new(0),
            verify_count: AtomicUsize::new(0),
            commit_count: AtomicUsize::new(0),
            verify_result: Mutex::new(Some(Ok(()))),
            commit_result: Mutex::new(Some(Ok(Some(dummy_commit_result())))),
        }
    }
}

impl FinalizationEffectDriver for MockEffectDriver {
    fn bind_effect_identity(&self, task_id: &TaskId, attempt_id: &AttemptId) -> Result<(), String> {
        self.bind_count.fetch_add(1, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap()
            .push(format!("bind:{}:{}", task_id, attempt_id));
        Ok(())
    }

    fn run_verify(
        &self,
        verify_cmd: &str,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        self.verify_count.fetch_add(1, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap()
            .push(format!("verify:{verify_cmd}"));
        self.verify_result.lock().unwrap().clone().unwrap_or(Ok(()))
    }

    fn commit_workspace(&self) -> Result<Option<CommitResult>, String> {
        self.commit_count.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().unwrap().push("commit".to_string());
        self.commit_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(None))
    }
}

fn make_test_app(
    approval_tx: tokio::sync::mpsc::UnboundedSender<ApplicationEvent>,
    provider: Arc<dyn InferenceProvider>,
) -> (
    Application,
    tempfile::TempDir,
    tetonic_memory::SharedStore,
    Arc<TestApprovalSink>,
) {
    let tmp = tempfile::tempdir().unwrap();

    let db_dir = tmp.path().join("db");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db_path = db_dir.join(format!(
        "lokai_v4_audit_{}_{}.db",
        std::process::id(),
        DB_SEQ.fetch_add(1, Ordering::Relaxed)
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
    let sink = Arc::new(TestApprovalSink {
        events: Mutex::new(Vec::new()),
        approval_tx,
    });
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: sink.clone(),
        index_db: None,
        fabric_hint: None,
    });
    app.bind_inference(provider, None);
    (app, tmp, store, sink)
}

fn make_workspace(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let ws = tmp.path().join(name);
    std::fs::create_dir_all(&ws).unwrap();
    ws
}

async fn next_approval_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ApplicationEvent>,
) -> (String, String, Option<String>) {
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for approval event")
        .expect("approval event channel closed");

    match event {
        ApplicationEvent::ApprovalRequest {
            session_id,
            approval_id,
            attempt_id,
            ..
        } => (session_id, approval_id, attempt_id),
        other => panic!("expected ApprovalRequest event, got: {:?}", other),
    }
}

// ============================================================================
// Provider Mocks
// ============================================================================

struct FinisherInferenceProvider;

#[async_trait]
impl InferenceProvider for FinisherInferenceProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![NodeInfo {
                id: tetonic_capacity::LOCAL_NODE_ID.into(),
                label: "mock_finisher".into(),
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
        _req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        Ok(ChatResponse {
            message: Message::assistant("Execution finished").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: serde_json::json!({
                        "summary": "Finished execution successfully"
                    }),
                },
            }]),
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}

struct NestedShellInferenceProvider {
    root_calls: AtomicU64,
}

#[async_trait]
impl InferenceProvider for NestedShellInferenceProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![NodeInfo {
                id: tetonic_capacity::LOCAL_NODE_ID.into(),
                label: "mock_nested".into(),
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
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let meta = req.fabric.as_ref();
        let aid = meta.and_then(|m| m.agent_id.as_deref()).unwrap_or("a0");

        if aid == "a0" {
            let call = self.root_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                // First root call: spawn child coder
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![ToolCall {
                        function: FunctionCall {
                            name: "spawn_agent".into(),
                            arguments: serde_json::json!({
                                "role": "coder",
                                "task": "execute child shell command"
                            }),
                        },
                    }]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                })
            } else {
                // Root finished after child returns
                Ok(ChatResponse {
                    message: Message::assistant("root completed").with_tool_calls(vec![ToolCall {
                        function: FunctionCall {
                            name: "finish".into(),
                            arguments: serde_json::json!({
                                "summary": "root finished successfully"
                            }),
                        },
                    }]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                })
            }
        } else {
            // Child specialist agent
            let is_tool_return = req
                .messages
                .last()
                .map(|m| m.role.as_str() == "tool")
                .unwrap_or(false);

            if is_tool_return {
                let last_content = req
                    .messages
                    .last()
                    .map(|m| m.content.as_str())
                    .unwrap_or("");
                if last_content.contains("nested_child_shell_ok") {
                    Ok(ChatResponse {
                        message: Message::assistant("child completed").with_tool_calls(vec![
                            ToolCall {
                                function: FunctionCall {
                                    name: "finish".into(),
                                    arguments: serde_json::json!({
                                        "summary": "child shell execution complete"
                                    }),
                                },
                            },
                        ]),
                        usage: GenUsage::default(),
                        provenance: InferenceProvenance::default(),
                    })
                } else {
                    Ok(ChatResponse {
                        message: Message::assistant("child aborted: shell denied").with_tool_calls(
                            vec![ToolCall {
                                function: FunctionCall {
                                    name: "finish".into(),
                                    arguments: serde_json::json!({
                                        "summary": "error: shell approval was denied"
                                    }),
                                },
                            }],
                        ),
                        usage: GenUsage::default(),
                        provenance: InferenceProvenance::default(),
                    })
                }
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("").with_tool_calls(vec![ToolCall {
                        function: FunctionCall {
                            name: "run_shell".into(),
                            arguments: serde_json::json!({
                                "command": "echo nested_child_shell_ok"
                            }),
                        },
                    }]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                })
            }
        }
    }
}

struct StandaloneShellInferenceProvider;

#[async_trait]
impl InferenceProvider for StandaloneShellInferenceProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![NodeInfo {
                id: tetonic_capacity::LOCAL_NODE_ID.into(),
                label: "mock_standalone".into(),
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
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        eprintln!(
            "[PROVIDER CHAT] aid={:?}",
            req.fabric.as_ref().and_then(|f| f.agent_id.as_deref())
        );
        let is_tool_return = req
            .messages
            .last()
            .map(|m| m.role.as_str() == "tool")
            .unwrap_or(false);

        if is_tool_return {
            let last_content = req
                .messages
                .last()
                .map(|m| m.content.as_str())
                .unwrap_or("");
            if last_content.contains("standalone_shell_ok") {
                Ok(ChatResponse {
                    message: Message::assistant("standalone finished").with_tool_calls(vec![
                        ToolCall {
                            function: FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({
                                    "summary": "standalone shell execution complete"
                                }),
                            },
                        },
                    ]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                })
            } else {
                Ok(ChatResponse {
                    message: Message::assistant("standalone aborted: shell denied")
                        .with_tool_calls(vec![ToolCall {
                            function: FunctionCall {
                                name: "finish".into(),
                                arguments: serde_json::json!({
                                    "summary": "error: shell approval was denied"
                                }),
                            },
                        }]),
                    usage: GenUsage::default(),
                    provenance: InferenceProvenance::default(),
                })
            }
        } else {
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: "run_shell".into(),
                        arguments: serde_json::json!({
                            "command": "echo standalone_shell_ok"
                        }),
                    },
                }]),
                usage: GenUsage::default(),
                provenance: InferenceProvenance::default(),
            })
        }
    }
}

// ============================================================================
// Defect 1 Tests: Verification, Commit, and Lease Loss Failure Agreement
// ============================================================================

/// Defect 1 Test 1: Chat reports failure when verification durably fails.
/// Verifies end-to-end agreement across:
/// 1. Product join result (`TurnFinish`: `ok == false`, `error` contains verification error).
/// 2. Emitted `TurnCompleted` event (`status == "error"`, `error` populated).
/// 3. Emitted `RunStatus` event (`status == "error"`, `error` populated).
/// 4. Durable store (`RunState::Failed`, `AttemptState::Failed`, `FailureClass::VerificationFailed`).
#[tokio::test]
async fn v4_audit_verification_failure_e2e() {
    let (approval_tx, _approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let (app, tmp, store, sink) = make_test_app(approval_tx, Arc::new(FinisherInferenceProvider));

    let ws = make_workspace(&tmp, "ws_verify_fail");
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            allow_shell: Some(true),
            auto_grant_approvals: Some(true),
            ..Default::default()
        })
        .await
        .expect("start session");

    let local = tokio::task::LocalSet::new();
    let finish: TurnFinish = local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "test verification failure".into(),
                verify_cmd: Some(
                    "cargo test --manifest-path nonexistent_manifest_12345.toml".into(),
                ),
                llm_router: Some(false),
            })
            .expect("submit chat turn");

            rx.await.expect("join turn")
        })
        .await;

    // 1. Assert product join result reports failure and carries error
    assert!(
        !finish.ok,
        "chat turn must report failure when verification fails"
    );
    assert!(!finish.canceled, "turn must not be reported as canceled");
    let err_msg = finish
        .error
        .expect("turn finish must carry verification failure error");
    assert!(
        err_msg.to_lowercase().contains("error")
            || err_msg.to_lowercase().contains("exit code")
            || err_msg.to_lowercase().contains("nonexistent"),
        "error message must describe verification failure: {err_msg}"
    );

    // 2. Assert emitted events agree on error status and error text
    let events = sink.all_events();
    let turn_completed = events
        .iter()
        .find(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
        .expect("TurnCompleted event must be emitted");

    let (run_id_opt, att_id_opt) = match turn_completed {
        ApplicationEvent::TurnCompleted {
            status,
            error,
            run_id,
            attempt_id,
            ..
        } => {
            assert_eq!(status, "error", "TurnCompleted status must be 'error'");
            assert!(
                error.is_some(),
                "TurnCompleted error must carry verification failure text"
            );
            (run_id.clone(), attempt_id.clone())
        }
        _ => unreachable!(),
    };

    let run_status = events
        .iter()
        .rev()
        .find(|e| matches!(e, ApplicationEvent::RunStatus { .. }))
        .expect("RunStatus event must be emitted");

    match run_status {
        ApplicationEvent::RunStatus { status, error, .. } => {
            assert_eq!(status, "error", "terminal RunStatus status must be 'error'");
            assert!(
                error.is_some(),
                "terminal RunStatus error must carry verification failure text"
            );
        }
        _ => unreachable!(),
    }

    // 3. Assert durable state agrees on failure
    let run_id = run_id_opt
        .map(tetonic_domain::RunId::new)
        .expect("run_id on TurnCompleted");
    let attempt_id = att_id_opt
        .map(tetonic_domain::AttemptId::new)
        .expect("attempt_id on TurnCompleted");

    let supervisor = DurableRunSupervisor::new(Some(store));
    let snap = supervisor.snapshot(run_id).await.expect("snapshot");
    assert_eq!(
        snap.state,
        RunState::Failed,
        "durable RunState must be Failed"
    );
    let attempt = snap.attempts.get(&attempt_id).expect("attempt in snapshot");
    assert_eq!(
        attempt.state,
        AttemptState::Failed,
        "durable AttemptState must be Failed"
    );
    assert_eq!(
        attempt.failure_class,
        Some(FailureClass::VerificationFailed),
        "durable failure_class must be VerificationFailed"
    );
}

/// Defect 1 Test 2: Commit failure propagation across complete_turn, events, and durable state.
#[tokio::test]
async fn v4_audit_commit_failure_e2e() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let supervisor: Arc<dyn RunSupervisor> =
        Arc::new(DurableRunSupervisor::new(Some(store.clone())));
    let (approval_tx, _approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = Arc::new(TestApprovalSink {
        events: Mutex::new(Vec::new()),
        approval_tx,
    });
    let art_dir = tmp.path().join("artifacts");
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            art_dir,
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runs = Arc::new(
        DefaultRunService::new(
            Some(store.clone()),
            Arc::new(tetonic_policy::PolicyEngine::default()),
            sink.clone(),
            supervisor.clone(),
            Arc::new(tetonic_app::SessionLiveStore::new()),
            artifact_store,
        )
        .with_identity_supplier(Arc::new(|input| {
            tetonic_app::definition::coding_identity_and_job_spec(input)
        })),
    );

    let session_id = store
        .write(|db| {
            db.start_session("test_workspace", "agent", "test_model")
                .map_err(|e| tetonic_app::errors::AppError::PersistenceFailed(e.to_string()))
        })
        .await
        .unwrap()
        .unwrap();

    let plan = runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "Task with failing commit".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    let driver = Arc::new(MockEffectDriver::new());
    *driver.commit_result.lock().unwrap() =
        Some(Err("disk I/O error during workspace commit".into()));

    let policy = Some(FinalizationPolicy {
        effect_driver: Some(driver.clone()),
        verify_cmd: None,
    });

    let cmd = CompleteTurnCommand {
        session_id: session_id.clone(),
        attempt_id: plan.attempt_id.clone(),
        workspace_root: "".into(),
        canceled: false,
        error: None,
    };

    let outcome = runs
        .complete_turn(
            &cmd,
            Some(&CandidateOutcome::Completed {
                summary: "done".into(),
                kind: CompletionKind::Finish,
            }),
            policy,
        )
        .await
        .expect("complete_turn must resolve final outcome");

    // 1. Assert returned outcome is Failed
    match &outcome {
        CandidateOutcome::Failed { message } => {
            assert!(
                message.contains("disk I/O error during workspace commit"),
                "returned outcome must reflect commit error: {message}"
            );
        }
        other => panic!("expected CandidateOutcome::Failed, got: {:?}", other),
    }

    // 2. Assert events reflect error
    let events = sink.all_events();
    let turn_completed = events
        .iter()
        .find(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
        .expect("TurnCompleted");
    match turn_completed {
        ApplicationEvent::TurnCompleted { status, error, .. } => {
            assert_eq!(status, "error");
            assert!(error.as_ref().unwrap().contains("disk I/O error"));
        }
        _ => unreachable!(),
    }

    // 3. Assert durable state is Failed with CommitFailed
    let snap = supervisor.snapshot(plan.run_id).await.expect("snapshot");
    assert_eq!(snap.state, RunState::Failed);
    let attempt = snap.attempts.get(&plan.attempt_id).expect("attempt");
    assert_eq!(attempt.state, AttemptState::Failed);
    assert_eq!(
        attempt.failure_class,
        Some(FailureClass::PermanentExecutionFailure),
        "failure_class must be PermanentExecutionFailure"
    );
}

/// Defect 1 Test 3: Lease loss during finalization propagation.
#[tokio::test]
async fn v4_audit_lease_loss_during_verify_e2e() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let supervisor: Arc<dyn RunSupervisor> =
        Arc::new(DurableRunSupervisor::new(Some(store.clone())));
    let (approval_tx, _approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = Arc::new(TestApprovalSink {
        events: Mutex::new(Vec::new()),
        approval_tx,
    });
    let art_dir = tmp.path().join("artifacts");
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            art_dir,
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runs = Arc::new(
        DefaultRunService::new(
            Some(store.clone()),
            Arc::new(tetonic_policy::PolicyEngine::default()),
            sink.clone(),
            supervisor.clone(),
            Arc::new(tetonic_app::SessionLiveStore::new()),
            artifact_store,
        )
        .with_identity_supplier(Arc::new(|input| {
            tetonic_app::definition::coding_identity_and_job_spec(input)
        })),
    );

    let session_id = store
        .write(|db| {
            db.start_session("test_workspace", "agent", "test_model")
                .map_err(|e| tetonic_app::errors::AppError::PersistenceFailed(e.to_string()))
        })
        .await
        .unwrap()
        .unwrap();

    let plan = runs
        .plan_turn(&RunTurnCommand {
            session_id: session_id.clone(),
            user_input: "Task with competing claim".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    // Competitor claims finalization before this attempt completes
    let competitor = AttemptId::new("att_competitor_winner");
    let lease_id = LeaseId::new("lease_comp_winner");
    let snap = supervisor.snapshot(plan.run_id.clone()).await.unwrap();
    let seq = snap.sequence;

    supervisor
        .handle(RunCommand::CreateAttempt(tetonic_domain::CreateAttempt {
            envelope: command_envelope("comp_create", Some(seq), "test"),
            run_id: plan.run_id.clone(),
            task_id: plan.task_id.clone(),
            attempt_id: competitor.clone(),
            delivery_key: None,
        }))
        .await
        .unwrap();

    let snap = supervisor.snapshot(plan.run_id.clone()).await.unwrap();
    let seq = snap.sequence;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let leased = supervisor
        .handle(RunCommand::LeaseAttempt(LeaseAttempt {
            envelope: command_envelope("comp_lease", Some(seq), "test"),
            run_id: plan.run_id.clone(),
            attempt_id: competitor.clone(),
            lease_id: lease_id.clone(),
            lease_epoch: 0,
            holder: ExecutionTargetId::local(),
            issued_at: now,
            expires_at: now + 3600,
            heartbeat_interval_secs: 30,
        }))
        .await
        .unwrap();

    let comp_rec = leased.snapshot.attempts.get(&competitor).unwrap();
    let comp_lease = comp_rec.lease.as_ref().unwrap();
    let lease_proof = LeaseProof {
        lease_id: comp_lease.lease_id.clone(),
        lease_epoch: comp_lease.lease_epoch,
        holder: comp_lease.holder.clone(),
    };

    supervisor
        .handle(RunCommand::StartAttempt(StartAttempt {
            envelope: command_envelope("comp_start", Some(leased.sequence), "test"),
            run_id: plan.run_id.clone(),
            attempt_id: competitor.clone(),
            lease_proof: lease_proof.clone(),
        }))
        .await
        .unwrap();

    let snap = supervisor.snapshot(plan.run_id.clone()).await.unwrap();
    supervisor
        .handle(RunCommand::ClaimFinalization(ClaimFinalization {
            envelope: command_envelope("comp_claim", Some(snap.sequence), "test"),
            run_id: plan.run_id.clone(),
            attempt_id: competitor.clone(),
            task_id: plan.task_id.clone(),
            task_version: 1,
            input_digest: plan.job_spec.input_digest.clone(),
            lease_proof,
        }))
        .await
        .unwrap();

    // Now the losing attempt tries to finalize
    let loser_driver = Arc::new(MockEffectDriver::new());
    let policy = Some(FinalizationPolicy {
        effect_driver: Some(loser_driver.clone()),
        verify_cmd: Some("cargo test".into()),
    });

    let cmd = CompleteTurnCommand {
        session_id: session_id.clone(),
        attempt_id: plan.attempt_id.clone(),
        workspace_root: "".into(),
        canceled: false,
        error: None,
    };

    let outcome = runs
        .complete_turn(
            &cmd,
            Some(&CandidateOutcome::Completed {
                summary: "losing attempt done".into(),
                kind: CompletionKind::Finish,
            }),
            policy,
        )
        .await
        .expect("complete_turn must resolve");

    // 1. Assert outcome is Failed with lease loss indication
    match &outcome {
        CandidateOutcome::Failed { message } => {
            assert!(
                message.contains("lease lost"),
                "returned outcome must reflect lease loss: {message}"
            );
        }
        other => panic!("expected CandidateOutcome::Failed, got: {:?}", other),
    }

    // 2. Driver must never have been called
    assert_eq!(loser_driver.bind_count.load(Ordering::SeqCst), 0);
    assert_eq!(loser_driver.verify_count.load(Ordering::SeqCst), 0);
    assert_eq!(loser_driver.commit_count.load(Ordering::SeqCst), 0);

    // 3. Emitted TurnCompleted status must be "error"
    let events = sink.all_events();
    let turn_completed = events
        .iter()
        .find(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
        .expect("TurnCompleted");
    match turn_completed {
        ApplicationEvent::TurnCompleted { status, error, .. } => {
            assert_eq!(status, "error");
            assert!(error.as_ref().unwrap().contains("lease lost"));
        }
        _ => unreachable!(),
    }
}

// ============================================================================
// Defect 2 Tests: Child Attempts Broker Approval Hook Registration & Removal
// ============================================================================

/// Defect 2 Test 1: Nested child spawn shell execution observes approval prompt,
/// authorizes execution, and removes hook upon completion.
#[tokio::test]
async fn v4_audit_nested_spawn_shell_approval_prompt_and_execution() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(NestedShellInferenceProvider {
        root_calls: AtomicU64::new(0),
    });
    let (app, tmp, _store, sink) = make_test_app(approval_tx, provider);

    let ws = make_workspace(&tmp, "ws_nested_approval");
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            orchestration: Some("auto".into()),
            critic: Some(false),
            allow_shell: Some(true),
            auto_grant_approvals: Some(false),
            ..Default::default()
        })
        .await
        .expect("start session");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "run nested child shell test".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit chat turn");

            // Receive child approval event
            let (sid, app_id, att_id) = next_approval_event(&mut approval_rx).await;
            let child_att_str = att_id.expect("attempt_id required on child approval event");
            let child_att = AttemptId::new(&child_att_str);

            // Assert broker has attempt approval hook registered for child attempt
            let broker = app.turn.runtime.action_broker();
            assert!(
                broker.has_attempt_approval(&child_att),
                "broker must have registered approval hook for child attempt {child_att:?}"
            );

            // Respond to child approval request
            let delivered = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid,
                    approval_id: app_id,
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(child_att_str.clone()),
                })
                .expect("respond to child approval");
            assert!(
                delivered,
                "approval response must be delivered to child hook"
            );

            // Await turn completion bounded by timeout
            let finish: TurnFinish = tokio::time::timeout(Duration::from_secs(10), rx)
                .await
                .expect("turn join timeout")
                .expect("join turn");
            assert!(
                finish.ok,
                "nested turn must succeed, got error: {:?}",
                finish.error
            );
            assert!(!finish.canceled, "turn must not be canceled");

            // Assert tool result confirms authorized shell execution
            let events = sink.all_events();
            let tool_result = events
                .iter()
                .find(|e| matches!(e, ApplicationEvent::ToolResult { tool, .. } if tool == "run_shell"))
                .expect("must emit ToolResult for run_shell");
            match tool_result {
                ApplicationEvent::ToolResult { ok, summary, .. } => {
                    assert!(ok, "run_shell tool result must be ok when approved");
                    assert!(
                        summary.contains("nested_child_shell_ok"),
                        "run_shell output must contain expected marker: {summary}"
                    );
                }
                _ => unreachable!(),
            }

            // Assert child attempt approval hook was removed upon completion
            assert!(
                !broker.has_attempt_approval(&child_att),
                "broker must unregister child attempt approval hook after completion"
            );
        })
        .await;
}

/// Defect 2 Test 2: Nested child shell execution when denied does NOT produce
/// output marker and cleans up approval hook.
#[tokio::test]
async fn v4_audit_nested_spawn_shell_approval_denied_blocks_execution() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(NestedShellInferenceProvider {
        root_calls: AtomicU64::new(0),
    });
    let (app, tmp, _store, sink) = make_test_app(approval_tx, provider);

    let ws = make_workspace(&tmp, "ws_nested_approval_denied");
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            orchestration: Some("auto".into()),
            critic: Some(false),
            allow_shell: Some(true),
            auto_grant_approvals: Some(false),
            ..Default::default()
        })
        .await
        .expect("start session");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "run nested child shell test".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit chat turn");

            let (sid, app_id, att_id) = next_approval_event(&mut approval_rx).await;
            let child_att_str = att_id.expect("attempt_id required on child approval event");
            let child_att = AttemptId::new(&child_att_str);

            let broker = app.turn.runtime.action_broker();
            assert!(broker.has_attempt_approval(&child_att));

            // Explicitly deny approval
            let delivered = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid,
                    approval_id: app_id,
                    approved: false,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(child_att_str.clone()),
                })
                .expect("respond to child approval");
            assert!(delivered);

            let _finish: TurnFinish = tokio::time::timeout(Duration::from_secs(10), rx)
                .await
                .expect("turn join timeout")
                .expect("join turn");

            // Verify tool result was NOT ok and marker was NOT output
            let events = sink.all_events();
            let tool_result = events
                .iter()
                .find(|e| matches!(e, ApplicationEvent::ToolResult { tool, .. } if tool == "run_shell"))
                .expect("must emit ToolResult for run_shell");
            match tool_result {
                ApplicationEvent::ToolResult { ok, summary, .. } => {
                    assert!(!ok, "run_shell must NOT succeed when denied");
                    assert!(
                        !summary.contains("nested_child_shell_ok"),
                        "denied shell must not output marker: {summary}"
                    );
                }
                _ => unreachable!(),
            }

            // Broker unregisters hook even on denial
            assert!(
                !broker.has_attempt_approval(&child_att),
                "broker must unregister child attempt approval hook after completion"
            );
        })
        .await;
}

/// Defect 2 Test 3: Standalone spawn shell execution observes approval prompt,
/// authorizes execution, and removes hook upon completion.
#[tokio::test]
async fn v4_audit_standalone_spawn_shell_approval_prompt_and_execution() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(StandaloneShellInferenceProvider);
    let (app, tmp, _store, sink) = make_test_app(approval_tx, provider);

    let ws = make_workspace(&tmp, "ws_standalone_approval");
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            allow_shell: Some(true),
            auto_grant_approvals: Some(false),
            ..Default::default()
        })
        .await
        .expect("start session");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);

            app.submit_spawn(SpawnAgentCommand {
                session_id: session.session_id.clone(),
                agent_id: "standalone_specialist".into(),
                parent_agent_id: "root".into(),
                role: "coder".into(),
                task: "execute standalone shell command".into(),
            })
            .expect("submit standalone spawn");

            // Receive standalone approval event
            let (sid, app_id, att_id) = next_approval_event(&mut approval_rx).await;
            let spawn_att_str = att_id.expect("attempt_id required on spawn approval event");
            let spawn_att = AttemptId::new(&spawn_att_str);

            // Assert broker has attempt approval hook registered for spawned attempt
            let broker = app.turn.runtime.action_broker();
            assert!(
                broker.has_attempt_approval(&spawn_att),
                "broker must have registered approval hook for standalone spawn attempt {spawn_att:?}"
            );

            // Respond to spawn approval request
            let delivered = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid,
                    approval_id: app_id,
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(spawn_att_str.clone()),
                })
                .expect("respond to spawn approval");
            assert!(delivered, "approval response must be delivered to spawn hook");

            // Await spawn completion bounded by timeout
            let finish: TurnFinish = tokio::time::timeout(Duration::from_secs(10), rx)
                .await
                .expect("spawn join timeout")
                .expect("join spawn");
            assert!(
                finish.ok,
                "standalone spawn must succeed, got error: {:?}",
                finish.error
            );
            assert!(!finish.canceled, "turn must not be canceled");

            // Assert tool result confirms authorized shell execution
            let events = sink.all_events();
            let tool_result = events
                .iter()
                .find(|e| matches!(e, ApplicationEvent::ToolResult { tool, .. } if tool == "run_shell"))
                .expect("must emit ToolResult for run_shell");
            match tool_result {
                ApplicationEvent::ToolResult { ok, summary, .. } => {
                    assert!(ok, "run_shell tool result must be ok when approved");
                    assert!(
                        summary.contains("standalone_shell_ok"),
                        "run_shell output must contain expected marker: {summary}"
                    );
                }
                _ => unreachable!(),
            }

            // Assert standalone spawn attempt approval hook was removed upon completion
            assert!(
                !broker.has_attempt_approval(&spawn_att),
                "broker must unregister standalone spawn attempt approval hook after completion"
            );
        })
        .await;
}

/// Defect 2 Test 4: Standalone spawn shell execution when denied does NOT produce
/// output marker and cleans up approval hook.
#[tokio::test]
async fn v4_audit_standalone_spawn_shell_approval_denied_blocks_execution() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(StandaloneShellInferenceProvider);
    let (app, tmp, _store, sink) = make_test_app(approval_tx, provider);

    let ws = make_workspace(&tmp, "ws_standalone_approval_denied");
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            allow_shell: Some(true),
            auto_grant_approvals: Some(false),
            ..Default::default()
        })
        .await
        .expect("start session");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);

            app.submit_spawn(SpawnAgentCommand {
                session_id: session.session_id.clone(),
                agent_id: "standalone_specialist".into(),
                parent_agent_id: "root".into(),
                role: "coder".into(),
                task: "execute standalone shell command".into(),
            })
            .expect("submit standalone spawn");

            let (sid, app_id, att_id) = next_approval_event(&mut approval_rx).await;
            let spawn_att_str = att_id.expect("attempt_id required on spawn approval event");
            let spawn_att = AttemptId::new(&spawn_att_str);

            let broker = app.turn.runtime.action_broker();
            assert!(broker.has_attempt_approval(&spawn_att));

            // Explicitly deny approval
            let delivered = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid,
                    approval_id: app_id,
                    approved: false,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(spawn_att_str.clone()),
                })
                .expect("respond to spawn approval");
            assert!(delivered);

            let _finish: TurnFinish = tokio::time::timeout(Duration::from_secs(10), rx)
                .await
                .expect("spawn join timeout")
                .expect("join spawn");

            // Verify tool result was NOT ok and marker was NOT output
            let events = sink.all_events();
            let tool_result = events
                .iter()
                .find(|e| matches!(e, ApplicationEvent::ToolResult { tool, .. } if tool == "run_shell"))
                .expect("must emit ToolResult for run_shell");
            match tool_result {
                ApplicationEvent::ToolResult { ok, summary, .. } => {
                    assert!(!ok, "run_shell must NOT succeed when denied");
                    assert!(
                        !summary.contains("standalone_shell_ok"),
                        "denied shell must not output marker: {summary}"
                    );
                }
                _ => unreachable!(),
            }

            // Broker unregisters hook even on denial
            assert!(
                !broker.has_attempt_approval(&spawn_att),
                "broker must unregister standalone spawn attempt approval hook after completion"
            );
        })
        .await;
}
