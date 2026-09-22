//! IFACE-02: Per-execution approval ownership.
//! Implements V4-PROOF-06 (Per-Execution Approval Routing Proof).
//!
//! Covers:
//! 1. Concurrent session isolation on shared runtime.
//! 2. Reverse reply order across executions.
//! 3. Reject wrong-Attempt reply (attempt mismatch).
//! 4. Reject duplicate and stale replies.
//! 5. Canceled Attempt pending approval fails closed and cannot authorize another Attempt.
//! 6. Actual authorized tool execution assertion (effect verification).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tetonic_app::commands::{
    ApprovalResponseCommand, CancelRunCommand, RunTurnCommand, StartSessionCommand, TurnFinish,
};
use tetonic_app::errors::AppError;
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_domain::ids::AttemptId;
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, FunctionCall, GenUsage, InferenceError,
    InferenceProvenance, InferenceProvider, Message, NodeInfo, TokenSink, ToolCall,
};

static IFACE02_DB_SEQ: AtomicU64 = AtomicU64::new(0);

struct TestApprovalSink {
    events: Mutex<Vec<ApplicationEvent>>,
    approval_tx: tokio::sync::mpsc::UnboundedSender<ApplicationEvent>,
}

impl ApplicationEventSink for TestApprovalSink {
    fn send(&self, event: ApplicationEvent) {
        if matches!(event, ApplicationEvent::ApprovalRequest { .. }) {
            let _ = self.approval_tx.send(event.clone());
        }
        self.events.lock().unwrap().push(event);
    }
}

struct MockApprovalInferenceProvider;

#[async_trait]
impl InferenceProvider for MockApprovalInferenceProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![NodeInfo {
                id: tetonic_capacity::LOCAL_NODE_ID.into(),
                label: "mock".into(),
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
        let is_tool_return = req
            .messages
            .last()
            .map(|m| m.role.as_str() == "tool")
            .unwrap_or(false);

        if is_tool_return {
            Ok(ChatResponse {
                message: Message::assistant("Execution finished").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({
                            "summary": "Finished tool execution"
                        }),
                    },
                }]),
                usage: GenUsage::default(),
                provenance: InferenceProvenance::default(),
            })
        } else {
            let last_user = req
                .messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| m.content.as_str())
                .unwrap_or("");
            let cmd = if let Some(stripped) = last_user.strip_prefix("CMD:") {
                stripped.trim()
            } else {
                "echo ok"
            };
            Ok(ChatResponse {
                message: Message::assistant("").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: "run_shell".into(),
                        arguments: serde_json::json!({
                            "command": cmd
                        }),
                    },
                }]),
                usage: GenUsage::default(),
                provenance: InferenceProvenance::default(),
            })
        }
    }
}

fn make_test_app(
    approval_tx: tokio::sync::mpsc::UnboundedSender<ApplicationEvent>,
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
        "lokai_iface02_{}_{}.db",
        std::process::id(),
        IFACE02_DB_SEQ.fetch_add(1, Ordering::Relaxed)
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
    app.bind_inference(Arc::new(MockApprovalInferenceProvider), None);
    (app, tmp, store, sink)
}

fn make_workspace(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let ws = tmp.path().join(name);
    std::fs::create_dir_all(&ws).unwrap();
    ws
}

/// Helper to start a test session configured for interactive approvals.
async fn start_test_session(
    app: &Application,
    workspace_root: &std::path::Path,
) -> tetonic_app::commands::StartSessionResultPayload {
    app.sessions
        .start_session(StartSessionCommand {
            workspace_root: workspace_root.display().to_string(),
            briefing: Some(false),
            allow_shell: Some(true),
            auto_grant_approvals: Some(false),
            ..Default::default()
        })
        .await
        .expect("start session")
}

/// Helper to receive the next ApprovalRequest event with a timeout.
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

/// V4-PROOF-06 Assertion 1: Concurrent Session Isolation on Shared Runtime.
/// Two concurrent sessions on a shared runtime each issue tool calls requiring approval.
/// Neither clobbers the other's approval hook or approval request.
#[tokio::test]
async fn test_concurrent_sessions_isolated_on_shared_runtime() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let (app, tmp, _store, _sink) = make_test_app(approval_tx);

    let ws_a = make_workspace(&tmp, "ws_a");
    let ws_b = make_workspace(&tmp, "ws_b");
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
                user_input: "CMD: echo isolation_a > out_a.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit A");

            // Submit turn on Session B
            app.submit_chat_turn(RunTurnCommand {
                session_id: session_b.session_id.clone(),
                user_input: "CMD: echo isolation_b > out_b.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit B");

            // Receive both approval events
            let (sid_1, app_id_1, att_id_1) = next_approval_event(&mut approval_rx).await;
            let (sid_2, app_id_2, att_id_2) = next_approval_event(&mut approval_rx).await;

            let att_1 = att_id_1.expect("attempt_id required on approval event");
            let att_2 = att_id_2.expect("attempt_id required on approval event");

            // Assert attempt identities are distinct
            assert_ne!(att_1, att_2, "attempt IDs must be distinct");
            assert_ne!(sid_1, sid_2, "session IDs must be distinct");

            // Assert both attempts are registered in runtime action_broker concurrently (no clobber)
            let broker = app.turn.runtime.action_broker();
            assert!(
                broker.has_attempt_approval(&AttemptId::new(&att_1)),
                "attempt 1 hook must be registered"
            );
            assert!(
                broker.has_attempt_approval(&AttemptId::new(&att_2)),
                "attempt 2 hook must be registered"
            );

            // Respond to both approvals
            let delivered_1 = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid_1.clone(),
                    approval_id: app_id_1.clone(),
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att_1.clone()),
                })
                .expect("respond 1");
            assert!(delivered_1);

            let delivered_2 = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid_2.clone(),
                    approval_id: app_id_2.clone(),
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att_2.clone()),
                })
                .expect("respond 2");
            assert!(delivered_2);

            // Await both turn completions
            let res_a: TurnFinish = rx_a.await.expect("join A");
            let res_b: TurnFinish = rx_b.await.expect("join B");

            assert!(
                res_a.ok && !res_a.canceled,
                "turn A must succeed: {res_a:?}"
            );
            assert!(
                res_b.ok && !res_b.canceled,
                "turn B must succeed: {res_b:?}"
            );

            // Assert hooks were unregistered upon turn completion
            assert!(
                !broker.has_attempt_approval(&AttemptId::new(&att_1)),
                "attempt 1 hook unregistered after turn"
            );
            assert!(
                !broker.has_attempt_approval(&AttemptId::new(&att_2)),
                "attempt 2 hook unregistered after turn"
            );
        })
        .await;

    // Assert both files were created in their respective workspaces
    assert!(ws_a.join("out_a.txt").exists());
    assert!(ws_b.join("out_b.txt").exists());
}

/// V4-PROOF-06 Assertion 2: Reverse Reply Order Across Executions.
/// Two pending approvals resolved in reverse order (B before A) correctly authorize
/// their respective attempts and execute their tools without head-of-line blocking.
#[tokio::test]
async fn test_reverse_reply_order() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let (app, tmp, _store, _sink) = make_test_app(approval_tx);

    let ws_a = make_workspace(&tmp, "ws_a");
    let ws_b = make_workspace(&tmp, "ws_b");
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
                user_input: "CMD: echo rev_a > rev_a.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit A");

            // Wait for A to park
            let (sid_a, app_id_a, att_id_a) = next_approval_event(&mut approval_rx).await;
            assert_eq!(sid_a, session_a.session_id);
            let att_a = att_id_a.expect("attempt_id A");

            // Submit turn on Session B
            app.submit_chat_turn(RunTurnCommand {
                session_id: session_b.session_id.clone(),
                user_input: "CMD: echo rev_b > rev_b.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit B");

            // Wait for B to park
            let (sid_b, app_id_b, att_id_b) = next_approval_event(&mut approval_rx).await;
            assert_eq!(sid_b, session_b.session_id);
            let att_b = att_id_b.expect("attempt_id B");

            // REVERSE REPLY ORDER: Respond to Session B FIRST
            let delivered_b = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid_b.clone(),
                    approval_id: app_id_b.clone(),
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att_b.clone()),
                })
                .expect("respond B first");
            assert!(delivered_b);

            // Turn B completes
            let res_b = rx_b.await.expect("join B");
            assert!(res_b.ok && !res_b.canceled);
            assert!(ws_b.join("rev_b.txt").exists(), "B tool executed");

            // Turn A is STILL waiting (file does not exist yet; hook still active)
            assert!(
                !ws_a.join("rev_a.txt").exists(),
                "A file must not exist before A approval"
            );
            assert!(
                app.turn
                    .runtime
                    .action_broker()
                    .has_attempt_approval(&AttemptId::new(&att_a)),
                "A hook must still be parked"
            );

            // Respond to Session A SECOND
            let delivered_a = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid_a.clone(),
                    approval_id: app_id_a.clone(),
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att_a.clone()),
                })
                .expect("respond A second");
            assert!(delivered_a);

            // Turn A completes
            let res_a = rx_a.await.expect("join A");
            assert!(res_a.ok && !res_a.canceled);
            assert!(ws_a.join("rev_a.txt").exists(), "A tool executed");
        })
        .await;
}

/// V4-PROOF-06 Assertion 3: Reject Wrong-Attempt Reply.
/// An approval response with an attempt_id mismatch returns an error,
/// leaves the parked wait untouched, and allows a subsequent correct response to succeed.
#[tokio::test]
async fn test_reject_wrong_attempt_reply() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let (app, tmp, _store, _sink) = make_test_app(approval_tx);

    let ws = make_workspace(&tmp, "ws");
    let session = start_test_session(&app, &ws).await;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "CMD: echo mismatch > mismatch.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit turn");

            let (sid, app_id, att_id) = next_approval_event(&mut approval_rx).await;
            let real_att = att_id.expect("attempt_id");

            // Attempt to respond with a mismatched attempt_id
            let err_res = app.approvals.respond(ApprovalResponseCommand {
                session_id: sid.clone(),
                approval_id: app_id.clone(),
                approved: true,
                remember: false,
                kind: "run_shell".into(),
                detail: "".into(),
                channel_delivered: true,
                attempt_id: Some("attempt_wrong_9999".into()),
            });

            match err_res {
                Err(AppError::InvalidRequest(msg)) => {
                    assert!(
                        msg.contains("approval attempt mismatch"),
                        "expected mismatch message, got: {msg}"
                    );
                }
                other => panic!(
                    "expected InvalidRequest for mismatched attempt, got: {:?}",
                    other
                ),
            }

            // Assert file was NOT created and turn has not completed
            assert!(!ws.join("mismatch.txt").exists());

            // Now respond with matching attempt_id
            let ok_res = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid.clone(),
                    approval_id: app_id.clone(),
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(real_att),
                })
                .expect("matching respond");
            assert!(ok_res);

            let res = rx.await.expect("join turn");
            assert!(res.ok && !res.canceled);
            assert!(ws.join("mismatch.txt").exists());
        })
        .await;
}

/// V4-PROOF-06 Assertion 4: Reject Duplicate and Stale Replies.
/// Submitting a duplicate response to an already-completed approval request returns an error.
/// Submitting a response to an unknown approval request returns an error.
#[tokio::test]
async fn test_reject_duplicate_and_stale_reply() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let (app, tmp, _store, _sink) = make_test_app(approval_tx);

    let ws = make_workspace(&tmp, "ws");
    let session = start_test_session(&app, &ws).await;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&session.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "CMD: echo dup > dup.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit turn");

            let (sid, app_id, att_id) = next_approval_event(&mut approval_rx).await;
            let att = att_id.expect("attempt_id");

            // Initial response succeeds
            let first_res = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid.clone(),
                    approval_id: app_id.clone(),
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att.clone()),
                })
                .expect("first response");
            assert!(first_res);

            let res = rx.await.expect("join turn");
            assert!(res.ok && !res.canceled);

            // Duplicate response to same approval_id must fail
            let dup_res = app.approvals.respond(ApprovalResponseCommand {
                session_id: sid.clone(),
                approval_id: app_id.clone(),
                approved: true,
                remember: false,
                kind: "run_shell".into(),
                detail: "".into(),
                channel_delivered: true,
                attempt_id: Some(att.clone()),
            });

            match dup_res {
                Err(AppError::InvalidRequest(msg)) => {
                    assert!(
                        msg.contains("duplicate or stale"),
                        "expected duplicate or stale message, got: {msg}"
                    );
                }
                other => panic!("expected duplicate error, got: {:?}", other),
            }

            // Unknown approval request with attempt_id must fail
            let stale_res = app.approvals.respond(ApprovalResponseCommand {
                session_id: sid.clone(),
                approval_id: "nonexistent_approval_xyz".into(),
                approved: true,
                remember: false,
                kind: "run_shell".into(),
                detail: "".into(),
                channel_delivered: true,
                attempt_id: Some(att),
            });

            match stale_res {
                Err(AppError::InvalidRequest(msg)) => {
                    assert!(
                        msg.contains("stale or unknown"),
                        "expected stale or unknown message, got: {msg}"
                    );
                }
                other => panic!("expected stale or unknown error, got: {:?}", other),
            }
        })
        .await;
}

/// V4-PROOF-06 Assertion 5: Canceled Attempt Pending Approval Fails Closed.
/// Canceling an Attempt with a pending approval immediately fails closed (false)
/// all waiters for that Attempt, unregisters its hook, and ensures late responses
/// cannot authorize another Attempt. Concurrent Session B remains unaffected.
#[tokio::test]
async fn test_canceled_attempt_cannot_authorize_other_attempt() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let (app, tmp, _store, _sink) = make_test_app(approval_tx);

    let ws_a = make_workspace(&tmp, "ws_a");
    let ws_b = make_workspace(&tmp, "ws_b");
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
                user_input: "CMD: echo cancel_a > cancel_a.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit A");

            // Wait for A's approval event
            let (sid_a, app_id_a, att_id_a) = next_approval_event(&mut approval_rx).await;
            let att_a = att_id_a.expect("attempt_id A");

            // Submit turn on Session B
            app.submit_chat_turn(RunTurnCommand {
                session_id: session_b.session_id.clone(),
                user_input: "CMD: echo cancel_b > cancel_b.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit B");

            // Wait for B's approval event
            let (sid_b, app_id_b, att_id_b) = next_approval_event(&mut approval_rx).await;
            let att_b = att_id_b.expect("attempt_id B");

            // Cancel Session A
            app.sessions
                .cancel_session(CancelRunCommand {
                    session_id: session_a.session_id.clone(),
                    pooled_cancel: false,
                })
                .await
                .expect("cancel session A");

            // Turn A completes as canceled
            let res_a = rx_a.await.expect("join A");
            assert!(res_a.canceled, "Turn A must be canceled");

            // Attempt A hook is unregistered
            assert!(
                !app.turn
                    .runtime
                    .action_broker()
                    .has_attempt_approval(&AttemptId::new(&att_a)),
                "canceled Attempt A hook must be unregistered"
            );

            // Tool for A was NOT executed
            assert!(
                !ws_a.join("cancel_a.txt").exists(),
                "canceled A tool must not execute"
            );

            // Late response to canceled Attempt A is rejected (duplicate/stale)
            let late_res = app.approvals.respond(ApprovalResponseCommand {
                session_id: sid_a.clone(),
                approval_id: app_id_a.clone(),
                approved: true,
                remember: false,
                kind: "run_shell".into(),
                detail: "".into(),
                channel_delivered: true,
                attempt_id: Some(att_a.clone()),
            });

            match late_res {
                Err(AppError::InvalidRequest(msg)) => {
                    assert!(
                        msg.contains("duplicate or stale"),
                        "late response to canceled attempt must be rejected as stale, got: {msg}"
                    );
                }
                other => panic!("expected stale error for late response, got: {:?}", other),
            }

            // Attempt B is STILL valid, isolated, and running!
            assert!(
                app.turn
                    .runtime
                    .action_broker()
                    .has_attempt_approval(&AttemptId::new(&att_b)),
                "Attempt B hook must still be active"
            );

            // Respond to Attempt B
            let delivered_b = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid_b.clone(),
                    approval_id: app_id_b.clone(),
                    approved: true,
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att_b.clone()),
                })
                .expect("respond B");
            assert!(delivered_b);

            // Turn B completes successfully
            let res_b = rx_b.await.expect("join B");
            assert!(res_b.ok && !res_b.canceled, "Turn B must succeed");
            assert!(
                ws_b.join("cancel_b.txt").exists(),
                "Turn B tool must execute"
            );
        })
        .await;
}

/// V4-PROOF-06 Assertion 6: Actual Authorized Tool Execution.
/// Verifies the physical side effects of tool execution:
/// - Explicit denial (approved: false) prevents tool execution (file is NOT created).
/// - Explicit approval (approved: true) executes the tool (file IS created with expected content).
#[tokio::test]
async fn test_actual_authorized_tool_execution() {
    let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let (app, tmp, _store, _sink) = make_test_app(approval_tx);

    let ws = make_workspace(&tmp, "ws");
    let session = start_test_session(&app, &ws).await;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // Case 1: Denied approval
            let rx_deny = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "CMD: echo denied_payload > denied.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit turn deny");

            let (sid, app_id, att_id) = next_approval_event(&mut approval_rx).await;
            let att = att_id.expect("attempt_id");

            let delivered = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid.clone(),
                    approval_id: app_id.clone(),
                    approved: false, // DENIED
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att),
                })
                .expect("respond deny");
            assert!(delivered);

            let res_deny = rx_deny.await.expect("join deny");
            assert!(res_deny.ok, "agent handles denied tool gracefully");
            assert!(
                !ws.join("denied.txt").exists(),
                "denied command must NOT create file"
            );

            // Case 2: Approved approval
            let rx_allow = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "CMD: echo approved_payload > approved.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit turn allow");

            let (sid2, app_id2, att_id2) = next_approval_event(&mut approval_rx).await;
            let att2 = att_id2.expect("attempt_id 2");

            let delivered2 = app
                .approvals
                .respond(ApprovalResponseCommand {
                    session_id: sid2.clone(),
                    approval_id: app_id2.clone(),
                    approved: true, // APPROVED
                    remember: false,
                    kind: "run_shell".into(),
                    detail: "".into(),
                    channel_delivered: true,
                    attempt_id: Some(att2),
                })
                .expect("respond allow");
            assert!(delivered2);

            let res_allow = rx_allow.await.expect("join allow");
            assert!(res_allow.ok, "turn finishes after approval");
            assert!(
                ws.join("approved.txt").exists(),
                "approved command must create file"
            );

            let content = std::fs::read_to_string(ws.join("approved.txt")).unwrap();
            assert!(
                content.contains("approved_payload"),
                "file must contain expected payload, got: {content}"
            );
        })
        .await;
}
