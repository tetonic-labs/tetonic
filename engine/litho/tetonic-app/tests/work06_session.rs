//! WORK-06: Session Chat-Only Migration & Remove LiveSession.current_run_id.
//! Implements V4-PROOF-06 (Concurrent Session Isolation and Durable Reconstruction).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tetonic_app::commands::{
    CancelRunCommand, InspectRunCommand, RunTurnCommand, StartSessionCommand,
};
use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};
use tetonic_app::{Application, ApplicationDependencies};
use tetonic_domain::RunState;
use tetonic_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, FunctionCall, InferenceError, InferenceProvider,
    Message, NodeInfo, TokenSink, ToolCall,
};

struct FakeEventSink;
impl ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: ApplicationEvent) {}
}

static WORK06_DB: AtomicU64 = AtomicU64::new(0);

fn make_app_with_db(
    db_path: std::path::PathBuf,
    tmp: &tempfile::TempDir,
) -> (Application, tetonic_memory::SharedStore) {
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
        store: Some(store.clone()),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    (app, store)
}

fn make_app() -> (
    Application,
    tempfile::TempDir,
    tetonic_memory::SharedStore,
    std::path::PathBuf,
) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join(format!(
        "lokai_work06_{}_{}.db",
        std::process::id(),
        WORK06_DB.fetch_add(1, Ordering::Relaxed)
    ));
    let (app, store) = make_app_with_db(db_path.clone(), &tmp);
    (app, tmp, store, db_path)
}

struct MockTurnProvider {
    delay: Duration,
    on_chat_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    requests: std::sync::Mutex<Vec<ChatRequest>>,
}

impl MockTurnProvider {
    fn immediate() -> Self {
        Self {
            delay: Duration::from_millis(0),
            on_chat_tx: None,
            requests: Default::default(),
        }
    }

    fn with_notify(
        delay: Duration,
        on_chat_tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Self {
        Self {
            delay,
            on_chat_tx: Some(on_chat_tx),
            requests: Default::default(),
        }
    }
}

#[async_trait]
impl InferenceProvider for MockTurnProvider {
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
            effective_concurrency: 2,
            generated_at: chrono::Utc::now(),
        }
    }

    async fn chat(
        &self,
        req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let last_user = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        if let Some(tx) = &self.on_chat_tx {
            let _ = tx.send(last_user.clone());
        }
        if self.delay.as_millis() > 0 {
            tokio::time::sleep(self.delay).await;
        }
        self.requests.lock().unwrap().push(req);
        Ok(ChatResponse {
            message: Message::assistant(format!("Finished: {}", last_user)).with_tool_calls(vec![
                ToolCall {
                    function: FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({
                            "summary": format!("Finished: {}", last_user)
                        }),
                    },
                },
            ]),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

/// Resume must preserve the actual inference prefix, not just the audit rows.
/// A mismatch here makes a warm runtime process the entire conversation again.
#[tokio::test]
async fn resumed_request_keeps_the_live_request_prefix() {
    let (app, tmp, _store, db_path) = make_app();
    let provider = Arc::new(MockTurnProvider::immediate());
    app.bind_inference(provider.clone(), None);
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let done = app.arm_turn_join(&started.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "Remember the complete conversation".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .unwrap();
            assert!(done.await.unwrap().ok);
        })
        .await;
    let first = provider.requests.lock().unwrap()[0].clone();
    drop(app);

    let (restarted, _) = make_app_with_db(db_path, &tmp);
    let resumed_provider = Arc::new(MockTurnProvider::immediate());
    restarted.bind_inference(resumed_provider.clone(), None);
    let resumed = restarted
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            session_id: Some(started.session_id.clone()),
            resume: Some(true),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(resumed.resumed);
    local
        .run_until(async {
            let done = restarted.arm_turn_join(&started.session_id);
            restarted
                .submit_chat_turn(RunTurnCommand {
                    session_id: started.session_id.clone(),
                    user_input: "Continue the conversation".into(),
                    verify_cmd: None,
                    llm_router: Some(false),
                })
                .unwrap();
            assert!(done.await.unwrap().ok);
        })
        .await;
    let requests = resumed_provider.requests.lock().unwrap();
    let after = &requests[0];
    assert!(after.messages.len() > first.messages.len());
    assert_eq!(
        serde_json::to_value(&first.messages).unwrap(),
        serde_json::to_value(&after.messages[..first.messages.len()]).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&first.tools).unwrap(),
        serde_json::to_value(&after.tools).unwrap()
    );
    assert_eq!(first.num_ctx, after.num_ctx);
    assert_eq!(first.model, after.model);
}

/// V4-PROOF-06 Assertion 1: Concurrent Session Isolation and Multi-Turn Chat (BH-MULTI).
#[tokio::test]
async fn routed_roles_keep_the_same_catalog_and_append_their_scope() {
    let (app, tmp, _store, _db) = make_app();
    let provider = Arc::new(MockTurnProvider::immediate());
    app.bind_inference(provider.clone(), None);
    app.bind_num_ctx(32768);
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            orchestration: Some("auto".into()),
            critic: Some(false),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for prompt in ["hello", "explain the code", "implement a function"] {
                let done = app.arm_turn_join(&session.session_id);
                app.submit_chat_turn(RunTurnCommand {
                    session_id: session.session_id.clone(),
                    user_input: prompt.into(),
                    verify_cmd: None,
                    llm_router: Some(false),
                })
                .unwrap();
                let outcome = done.await.unwrap();
                assert!(outcome.ok, "{outcome:?}");
            }
        })
        .await;
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    for pair in requests.windows(2) {
        assert_eq!(
            serde_json::to_value(&pair[0].tools).unwrap(),
            serde_json::to_value(&pair[1].tools).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&pair[0].messages).unwrap(),
            serde_json::to_value(&pair[1].messages[..pair[0].messages.len()]).unwrap()
        );
    }
    let planner_scope = &requests[1].messages.last().unwrap().content;
    assert!(planner_scope.contains("planner"));
    let permitted = planner_scope
        .split("use ONLY these tools: ")
        .nth(1)
        .unwrap()
        .split('.')
        .next()
        .unwrap();
    assert!(!permitted.contains("write_file"));
    assert!(!permitted.contains("spawn_agent"));
    assert!(requests[2]
        .messages
        .last()
        .unwrap()
        .content
        .contains("write_file"));
}

/// V4-PROOF-06 Assertion 1: Concurrent Session Isolation and Multi-Turn Chat (BH-MULTI).
/// Overlapping sessions on a shared runtime operate independently, preserving leased conversation
/// and multi-turn message history across turns.
#[tokio::test]
async fn work06_assertion1_concurrent_session_isolation_and_multi_turn_chat() {
    let (app, tmp, store, _db) = make_app();
    app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

    let ws_a = tmp.path().join("ws_a");
    let ws_b = tmp.path().join("ws_b");
    std::fs::create_dir_all(&ws_a).unwrap();
    std::fs::create_dir_all(&ws_b).unwrap();

    let started_a = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws_a.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session A");

    let started_b = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: ws_b.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start_session B");

    assert_ne!(started_a.session_id, started_b.session_id);

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // Turn 1 on Session A
            let rx_a1 = app.arm_turn_join(&started_a.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: started_a.session_id.clone(),
                user_input: "Turn 1 for A".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit A1");
            let res_a1 = rx_a1.await.expect("join A1");
            assert!(res_a1.ok && !res_a1.canceled);

            // Turn 1 on Session B
            let rx_b1 = app.arm_turn_join(&started_b.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: started_b.session_id.clone(),
                user_input: "Turn 1 for B".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit B1");
            let res_b1 = rx_b1.await.expect("join B1");
            assert!(res_b1.ok && !res_b1.canceled);

            // Turn 2 on Session A (multi-turn chat)
            let rx_a2 = app.arm_turn_join(&started_a.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: started_a.session_id.clone(),
                user_input: "Turn 2 for A".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit A2");
            let res_a2 = rx_a2.await.expect("join A2");
            assert!(res_a2.ok && !res_a2.canceled);

            // Turn 2 on Session B (multi-turn chat)
            let rx_b2 = app.arm_turn_join(&started_b.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: started_b.session_id.clone(),
                user_input: "Turn 2 for B".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit B2");
            let res_b2 = rx_b2.await.expect("join B2");
            assert!(res_b2.ok && !res_b2.canceled);
        })
        .await;

    // Verify chat history isolation in store
    let msgs_a = store
        .write({
            let sid = started_a.session_id.clone();
            move |db| db.list_messages_for_resume(&sid, 100)
        })
        .await
        .unwrap()
        .unwrap();

    let msgs_b = store
        .write({
            let sid = started_b.session_id.clone();
            move |db| db.list_messages_for_resume(&sid, 100)
        })
        .await
        .unwrap()
        .unwrap();

    let contents_a: Vec<String> = msgs_a.into_iter().map(|m| m.content).collect();
    let contents_b: Vec<String> = msgs_b.into_iter().map(|m| m.content).collect();

    assert!(contents_a.iter().any(|c| c.contains("Turn 1 for A")));
    assert!(contents_a.iter().any(|c| c.contains("Turn 2 for A")));
    assert!(!contents_a.iter().any(|c| c.contains("Turn 1 for B")));
    assert!(!contents_a.iter().any(|c| c.contains("Turn 2 for B")));

    assert!(contents_b.iter().any(|c| c.contains("Turn 1 for B")));
    assert!(contents_b.iter().any(|c| c.contains("Turn 2 for B")));
    assert!(!contents_b.iter().any(|c| c.contains("Turn 1 for A")));
    assert!(!contents_b.iter().any(|c| c.contains("Turn 2 for A")));
}

/// V4-PROOF-06 Assertion 2: Session Cancellation Isolation.
/// Canceling Session A cancels only Session A's active manager Attempt/Run,
/// leaving Session B's active Attempt/Run running and unaffected to complete successfully.
#[tokio::test]
async fn work06_assertion2_session_cancellation_isolation() {
    let (app, tmp, _store, _db) = make_app();
    let (chat_tx, mut chat_rx) = tokio::sync::mpsc::unbounded_channel();
    app.bind_inference(
        Arc::new(MockTurnProvider::with_notify(
            Duration::from_millis(400),
            chat_tx,
        )),
        None,
    );

    let started_a = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start A");

    let started_b = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start B");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx_a = app.arm_turn_join(&started_a.session_id);
            let rx_b = app.arm_turn_join(&started_b.session_id);

            // Submit turn on Session A
            app.submit_chat_turn(RunTurnCommand {
                session_id: started_a.session_id.clone(),
                user_input: "Long turn A".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit A");

            // Submit turn on Session B
            app.submit_chat_turn(RunTurnCommand {
                session_id: started_b.session_id.clone(),
                user_input: "Long turn B".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit B");

            // Wait until both turns enter inference
            let first = chat_rx.recv().await.expect("first chat entered");
            let second = chat_rx.recv().await.expect("second chat entered");
            assert!(
                (first.contains("Long turn A") && second.contains("Long turn B"))
                    || (first.contains("Long turn B") && second.contains("Long turn A"))
            );

            // Cancel Session A only
            app.sessions
                .cancel_session(CancelRunCommand {
                    session_id: started_a.session_id.clone(),
                    pooled_cancel: false,
                })
                .await
                .expect("cancel session A");

            // Await both joins
            let res_a = rx_a.await.expect("join A");
            let res_b = rx_b.await.expect("join B");

            // Session A was canceled
            assert!(
                res_a.canceled || !res_a.ok,
                "Session A must be canceled/failed, got: {res_a:?}"
            );

            // Session B continued and succeeded unaffected!
            assert!(
                res_b.ok && !res_b.canceled,
                "Session B must succeed unaffected, got: {res_b:?}"
            );
        })
        .await;
}

/// V4-PROOF-06 Assertion 3: Session Chat-Only and Zero Execution Authority.
/// Manager owns Attempt correlation and parent lookup via internal state (session_runs).
/// LiveSession contains no execution authority; clearing LiveSession current_run projection
/// does not disrupt manager parent resolution.
#[tokio::test]
async fn work06_assertion3_session_chat_only_no_execution_authority() {
    let (app, tmp, _store, _db) = make_app();
    app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start");

    // Plan turn to populate manager-owned session_runs and active registry
    let plan = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "check manager parent lookup".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan_turn");

    // Manager knows the active parent attempt for this session
    let parent = app.runs.active_parent_attempt(&started.session_id);
    assert_eq!(parent, Some(plan.attempt_id.clone()));

    // Manager knows the active run for this session
    let active_run = app.runs.active_run_for_session(&started.session_id);
    assert_eq!(active_run, Some(plan.run_id.clone()));

    // Now clear the LiveSession projection completely
    let live = app
        .sessions
        .live(&started.session_id)
        .expect("live session");
    live.clear_current_run();
    assert_eq!(live.current_run_id(), None);

    // Manager STILL resolves the parent active attempt from its own records!
    let parent_after_clear = app.runs.active_parent_attempt(&started.session_id);
    assert_eq!(
        parent_after_clear,
        Some(plan.attempt_id),
        "manager must resolve parent attempt from manager-owned records without reading LiveSession"
    );
}

/// V4-PROOF-06 Assertion 4: Durable Reconstruction Across Restarts.
/// Process restart reconstructs execution state entirely from the durable RunSupervisor journal,
/// and chat history entirely from the product Conversation store, without reading LiveSession
/// as an execution identity (BH-ID-SESSION defect resolved).
#[tokio::test]
async fn work06_assertion4_durable_reconstruction_across_restart() {
    let (app, tmp, _store, db_path) = make_app();
    app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.path().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("start");

    let local = tokio::task::LocalSet::new();
    let run_id = local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);
            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "Durable turn before restart".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit");

            let res = rx.await.expect("join");
            assert!(res.ok && !res.canceled, "turn must succeed: {res:?}");
            let live = app.sessions.live(&started.session_id).expect("live");
            live.last_run_id().expect("last_run_id")
        })
        .await;

    // Simulate process loss/restart: drop old application and re-open from db_path
    drop(app);

    let (restarted_app, restarted_store) = make_app_with_db(db_path, &tmp);

    // 1. Reconstruct execution state from durable RunSupervisor journal
    let snap = restarted_app
        .runs
        .inspect_run(InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("inspect_run from durable journal");

    assert_eq!(snap.run_id, run_id);
    assert!(
        matches!(snap.state, RunState::Succeeded),
        "execution state reconstructed from journal as Succeeded"
    );
    assert!(!snap.tasks.is_empty(), "tasks reconstructed from journal");

    // 2. Reconstruct chat history from product Conversation store
    let messages = restarted_store
        .write({
            let sid = started.session_id.clone();
            move |db| db.list_messages_for_resume(&sid, 50)
        })
        .await
        .unwrap()
        .unwrap();

    assert!(
        messages
            .iter()
            .any(|m| m.content.contains("Durable turn before restart")),
        "chat message reconstructed from product history"
    );

    // 3. In the restarted application, no LiveSession exists in memory yet
    assert_eq!(restarted_app.sessions.live_count(), 0);
    assert!(!restarted_app.sessions.has_live(&started.session_id));
}
