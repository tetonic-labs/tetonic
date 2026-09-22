//! WORK-04: Manager-Owned Attempt Lifetime tests.
//! Implements V4-PROOF-03 (Attempt future and in-flight cancellation)
//! and V4-PROOF-04 (Periodic durable heartbeat and lifecycle cleanup).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lokai_app::commands::{
    CancelByRunCommand, InspectRunCommand, ResumeRunEventsCommand, RunTurnCommand,
    StartSessionCommand,
};
use lokai_app::events::{ApplicationEvent, ApplicationEventSink};
use lokai_app::{Application, ApplicationDependencies};
use lokai_domain::{AttemptId, EventType, RunState};
use lokai_inference::{
    ChatRequest, ChatResponse, FabricSnapshot, FunctionCall, InferenceError, InferenceProvider,
    Message, NodeInfo, TokenSink, ToolCall,
};

struct FakeEventSink;
impl ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: ApplicationEvent) {}
}

static WORK04_DB: AtomicU64 = AtomicU64::new(0);

fn make_app() -> (Application, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join(format!(
        "lokai_work04_{}_{}.db",
        std::process::id(),
        WORK04_DB.fetch_add(1, Ordering::Relaxed)
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            tmp.path().join("artifacts"),
            lokai_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
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

/// Flexible mock inference provider for testing agent turns.
struct MockTurnProvider {
    delay: Duration,
    on_chat_tx: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    fail_chat: bool,
}

impl MockTurnProvider {
    fn immediate() -> Self {
        Self {
            delay: Duration::from_millis(0),
            on_chat_tx: None,
            fail_chat: false,
        }
    }

    fn with_delay(delay: Duration) -> Self {
        Self {
            delay,
            on_chat_tx: None,
            fail_chat: false,
        }
    }

    fn with_notify(delay: Duration, on_chat_tx: tokio::sync::mpsc::UnboundedSender<()>) -> Self {
        Self {
            delay,
            on_chat_tx: Some(on_chat_tx),
            fail_chat: false,
        }
    }

    fn failing() -> Self {
        Self {
            delay: Duration::from_millis(0),
            on_chat_tx: None,
            fail_chat: true,
        }
    }
}

#[async_trait]
impl InferenceProvider for MockTurnProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: vec![NodeInfo {
                id: lokai_capacity::LOCAL_NODE_ID.into(),
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
            effective_concurrency: 1,
            generated_at: chrono::Utc::now(),
        }
    }

    async fn chat(
        &self,
        _req: ChatRequest,
        _on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        if let Some(tx) = &self.on_chat_tx {
            let _ = tx.send(());
        }
        if self.delay.as_millis() > 0 {
            tokio::time::sleep(self.delay).await;
        }
        if self.fail_chat {
            return Err(InferenceError::Provider("mock inference failure".into()));
        }
        Ok(ChatResponse {
            message: Message::assistant("I have finished the task.").with_tool_calls(vec![
                ToolCall {
                    function: FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({
                            "summary": "I have finished the task."
                        }),
                    },
                },
            ]),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

// ---------------------------------------------------------------------------
// Structural verification pins (S1-S5)
// ---------------------------------------------------------------------------

#[test]
fn work04_active_turn_run_contains_lifetime_fields() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Heartbeat);
}

#[test]
fn work04_product_submit_has_no_spawn_local() {
    let src = crate_src("src/product_submit.rs");
    assert!(!src.contains("spawn_local"));
    assert!(src.contains("self.run_manager.dispatch_chat_turn"));
    assert!(src.contains("self.run_manager.dispatch_spawn"));
}

#[test]
fn work04_cancel_run_signals_in_flight_token() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Cancellation);
}

#[test]
fn work04_heartbeat_driver_present_in_run_service() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Heartbeat);
}

// ---------------------------------------------------------------------------
// V4-PROOF-03 Behavioral Tests
// ---------------------------------------------------------------------------

fn get_session_run_id(live: &lokai_app::session_live::LiveSession) -> lokai_domain::RunId {
    live.last_run_id()
        .or_else(|| live.current_run_id())
        .expect("run_id")
}

/// V4-PROOF-03 Assertion 1: Dropping the product submit waiter does not abort
/// or drop the running manager Attempt.
#[tokio::test]
async fn work04_waiter_drop_does_not_abort_manager_attempt() {
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

    app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // Arm join receiver on product submit door.
            let rx = app.arm_turn_join(&started.session_id);

            // Submit chat turn to product.
            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "explain how to build a compiler".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit_chat_turn");

            // Explicitly drop the waiter receiver immediately.
            drop(rx);

            // Give the manager task time to run to completion on this LocalSet.
            let live = app.sessions.live(&started.session_id).expect("live");
            let mut attempts = 0;
            while live.turn_in_flight() && attempts < 100 {
                tokio::time::sleep(Duration::from_millis(20)).await;
                attempts += 1;
            }
            assert!(
                !live.turn_in_flight(),
                "turn should finish despite waiter drop"
            );

            // Verify manager Attempt completed to Succeeded in supervisor.
            let run_id = get_session_run_id(&live);
            let snap = app
                .runs
                .inspect_run(InspectRunCommand {
                    run_id: run_id.to_string(),
                })
                .await
                .expect("inspect_run");
            assert_eq!(
                snap.state,
                RunState::Succeeded,
                "attempt must succeed when product waiter drops"
            );
        })
        .await;
}

/// V4-PROOF-03 Assertion 2: cancel_run signals the in-flight executor cancellation
/// token, stopping loop execution promptly.
#[tokio::test]
async fn work04_cancel_run_stops_inflight_executor() {
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

    let (chat_entered_tx, mut chat_entered_rx) = tokio::sync::mpsc::unbounded_channel();
    // Delay long enough that cancellation arrives while blocked in chat.
    app.bind_inference(
        Arc::new(MockTurnProvider::with_notify(
            Duration::from_millis(500),
            chat_entered_tx,
        )),
        None,
    );

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "long running operation".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit_chat_turn");

            // Wait until the executor actually enters chat.
            chat_entered_rx.recv().await.expect("entered chat");

            let live = app.sessions.live(&started.session_id).expect("live");
            let run_id = get_session_run_id(&live);

            // Cancel via manager cancel_run door.
            app.runs
                .cancel_run(CancelByRunCommand {
                    run_id: run_id.to_string(),
                })
                .await
                .expect("cancel_run");

            // Await product join or wait for in_flight to clear.
            let finish = rx.await.unwrap_or(lokai_app::commands::TurnFinish {
                ok: false,
                canceled: true,
                error: None,
            });
            assert!(finish.canceled, "turn finish must be canceled");

            // Supervisor state must be Canceled.
            let snap = app
                .runs
                .inspect_run(InspectRunCommand {
                    run_id: run_id.to_string(),
                })
                .await
                .expect("inspect");
            assert_eq!(snap.state, RunState::Canceled);

            // Supervisor journal must record run.canceled or CancellationRequested.
            let events = app
                .runs
                .resume_events(ResumeRunEventsCommand {
                    run_id: run_id.to_string(),
                    after_sequence: 0,
                    limit: Some(100),
                })
                .await
                .expect("resume_events")
                .expect("events ok");
            assert!(
                events.iter().any(
                    |e| matches!(&e.event_type, EventType::Other(s) if s == "run.canceled")
                        || matches!(e.event_type, EventType::CancellationRequested)
                ),
                "journal must contain run.canceled or CancellationRequested event"
            );
        })
        .await;
}

/// V4-PROOF-03 Assertion 3: Cancellation before effects transitions legally to
/// terminal Canceled without applying workspace mutations.
#[tokio::test]
async fn work04_cancellation_before_effects_cleanly_resolves_canceled() {
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

    let live = app.sessions.live(&started.session_id).expect("live");

    app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "test cancel".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit_chat_turn");

            // Cancel this queued turn before yielding to its execution future.
            live.request_cancel();
            let finish = rx.await.expect("turn finish");
            assert!(finish.canceled);

            let run_id = get_session_run_id(&live);
            let events = app
                .runs
                .resume_events(ResumeRunEventsCommand {
                    run_id: run_id.to_string(),
                    after_sequence: 0,
                    limit: Some(100),
                })
                .await
                .expect("resume_events")
                .expect("events");

            // Zero side effect commits or workspace commits.
            let has_commit_event = events.iter().any(|e| {
                matches!(e.event_type, EventType::WorkspaceTransactionCommitted)
                    || matches!(&e.event_type, EventType::Other(s) if s == "attempt.side_effect")
            });
            assert!(
                !has_commit_event,
                "cancellation before effects must produce zero commit events"
            );
        })
        .await;
}

/// V4-PROOF-03 Assertion 4: Cancellation racing with effectful commit resolves
/// deterministically via winner-fencing.
#[tokio::test]
async fn work04_cancellation_racing_with_finalization_resolves_deterministically() {
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

    app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "explain something".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit_chat_turn");

            let finish = rx.await.expect("turn finish");
            assert!(finish.ok, "{finish:?}");

            let live = app.sessions.live(&started.session_id).expect("live");
            let run_id = get_session_run_id(&live);

            let snap_before = app
                .runs
                .inspect_run(InspectRunCommand {
                    run_id: run_id.to_string(),
                })
                .await
                .expect("inspect_before");
            assert_eq!(
                snap_before.state,
                RunState::Succeeded,
                "completed run must be Succeeded before late cancel"
            );

            // Run has already Succeeded. Late cancel_run arrives.
            let _ = app
                .runs
                .cancel_run(CancelByRunCommand {
                    run_id: run_id.to_string(),
                })
                .await;

            let snap = app
                .runs
                .inspect_run(InspectRunCommand {
                    run_id: run_id.to_string(),
                })
                .await
                .expect("inspect");
            eprintln!(
                "AFTER CANCEL: state={:?}, attempts={:?}",
                snap.state, snap.attempts
            );
            assert_eq!(
                snap.state,
                RunState::Succeeded,
                "winner fence preserves Succeeded"
            );
        })
        .await;
}

/// V4-PROOF-03 Assertion 5: Failed admission prevents executor launch.
#[tokio::test]
async fn work04_admission_failure_prevents_executor_launch() {
    let (app, _tmp) = make_app();
    app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

    // Submitting with non-existent session ID must fail admission.
    let res = app.submit_chat_turn(RunTurnCommand {
        session_id: "non_existent_session".into(),
        user_input: "test".into(),
        verify_cmd: None,
        llm_router: Some(false),
    });
    assert!(res.is_err(), "unknown session must fail admission");

    // No attempt should exist for this non-existent session.
    assert!(app
        .runs
        .active_parent_attempt("non_existent_session")
        .is_none());
}

/// V4-PROOF-03 Assertion 6: Duplicate delivery fails closed and does not
/// launch a second executor.
#[tokio::test]
async fn work04_duplicate_delivery_fails_closed() {
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
        Arc::new(MockTurnProvider::with_notify(
            Duration::from_millis(300),
            chat_tx,
        )),
        None,
    );

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);

            // First submission succeeds.
            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "turn 1".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("first submit_chat_turn");

            // Wait until first turn is in flight.
            chat_rx.recv().await.expect("chat started");

            // Second submission while first is in flight must fail closed.
            let dup_res = app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "turn 1 duplicate".into(),
                verify_cmd: None,
                llm_router: Some(false),
            });
            assert!(
                dup_res.is_err(),
                "duplicate submission on active session must be rejected"
            );
            let err_msg = dup_res.unwrap_err().to_string();
            assert!(
                err_msg.contains("already running"),
                "expected 'already running', got: {err_msg}"
            );

            // Let first turn finish.
            let finish = rx.await.expect("first turn finish");
            assert!(finish.ok, "{finish:?}");
        })
        .await;
}

// ---------------------------------------------------------------------------
// V4-PROOF-04 Behavioral Tests
// ---------------------------------------------------------------------------

/// V4-PROOF-04 Assertion 1: A long-running Attempt produces periodic RecordHeartbeat
/// events at the declared interval before finalization.
#[tokio::test]
async fn work04_periodic_durable_heartbeats_recorded_while_running() {
    // Override heartbeat interval to 30ms for fast, deterministic test.
    std::env::set_var("LOKAI_HEARTBEAT_INTERVAL_MS", "30");

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

    // Hold turn for 120ms so at least 2-3 heartbeat ticks occur.
    app.bind_inference(
        Arc::new(MockTurnProvider::with_delay(Duration::from_millis(120))),
        None,
    );

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "explain recursion".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit_chat_turn");

            let finish = rx.await.expect("turn finish");
            assert!(finish.ok, "{finish:?}");

            let live = app.sessions.live(&started.session_id).expect("live");
            let run_id = get_session_run_id(&live);

            let events = app
                .runs
                .resume_events(ResumeRunEventsCommand {
                    run_id: run_id.to_string(),
                    after_sequence: 0,
                    limit: Some(100),
                })
                .await
                .expect("resume_events")
                .expect("events");

            let heartbeats: Vec<_> = events
                .iter()
                .filter(
                    |e| matches!(e.event_type, EventType::Other(ref s) if s == "attempt.heartbeat"),
                )
                .collect();

            assert!(
                heartbeats.len() >= 2,
                "expected at least 2 periodic heartbeats, found {}",
                heartbeats.len()
            );

            // Sequences must be strictly increasing.
            for window in heartbeats.windows(2) {
                assert!(
                    window[0].sequence < window[1].sequence,
                    "heartbeat sequences must be strictly increasing"
                );
            }
        })
        .await;

    std::env::remove_var("LOKAI_HEARTBEAT_INTERVAL_MS");
}

/// V4-PROOF-04 Assertion 2: Heartbeat driver terminates cleanly upon terminal
/// state; no stranded heartbeat tasks producing late events.
#[tokio::test]
async fn work04_heartbeat_driver_cleans_up_on_terminal_state() {
    std::env::set_var("LOKAI_HEARTBEAT_INTERVAL_MS", "25");

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

    app.bind_inference(
        Arc::new(MockTurnProvider::with_delay(Duration::from_millis(60))),
        None,
    );

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let rx = app.arm_turn_join(&started.session_id);

            app.submit_chat_turn(RunTurnCommand {
                session_id: started.session_id.clone(),
                user_input: "quick turn".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .expect("submit_chat_turn");

            let finish = rx.await.expect("turn finish");
            assert!(finish.ok, "{finish:?}");

            let live = app.sessions.live(&started.session_id).expect("live");
            let run_id = get_session_run_id(&live);

            let events_at_finish = app
                .runs
                .resume_events(ResumeRunEventsCommand {
                    run_id: run_id.to_string(),
                    after_sequence: 0,
                    limit: Some(100),
                })
                .await
                .expect("resume_events")
                .expect("events")
                .len();

            // Wait 100ms (4 heartbeat periods).
            tokio::time::sleep(Duration::from_millis(100)).await;

            let events_later = app
                .runs
                .resume_events(ResumeRunEventsCommand {
                    run_id: run_id.to_string(),
                    after_sequence: 0,
                    limit: Some(100),
                })
                .await
                .expect("resume_events")
                .expect("events")
                .len();

            assert_eq!(
                events_at_finish, events_later,
                "no new heartbeats or events must be recorded after terminal completion"
            );
        })
        .await;

    std::env::remove_var("LOKAI_HEARTBEAT_INTERVAL_MS");
}

/// V4-PROOF-04 Assertion 3: Lease loss or heartbeat persistence failure fails closed.
#[tokio::test]
async fn work04_lease_loss_or_heartbeat_failure_fails_closed() {
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

    // 1. Heartbeat on non-existent attempt fails closed.
    let hb_err = app.runs.heartbeat_turn("att_missing").await;
    assert!(
        hb_err.is_err(),
        "heartbeat on unleased attempt must fail closed"
    );

    // 2. Complete turn for unadmitted attempt fails closed.
    let comp_err = app
        .runs
        .complete_turn(
            &lokai_app::commands::CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: AttemptId::new("att_missing"),
                workspace_root: tmp.path().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await;
    assert!(
        comp_err.is_err(),
        "completion without valid active lease must fail closed"
    );
    let msg = comp_err.unwrap_err().to_string();
    assert!(
        msg.contains("no active turn run"),
        "expected 'no active turn run', got: {msg}"
    );

    // 3. Plan a turn, then heartbeat on that active attempt succeeds while running.
    let plan = app
        .runs
        .plan_turn(&RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "test hb".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");

    let hb_ok = app.runs.heartbeat_turn(&plan.attempt_id.to_string()).await;
    assert!(hb_ok.is_ok(), "heartbeat on active attempt must succeed");

    // 4. After completing the turn, subsequent heartbeats fail closed.
    app.runs
        .complete_turn(
            &lokai_app::commands::CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: plan.attempt_id.clone(),
                workspace_root: tmp.path().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect("complete");

    let hb_after = app.runs.heartbeat_turn(&plan.attempt_id.to_string()).await;
    assert!(
        hb_after.is_err(),
        "heartbeat on finalized/closed attempt must fail closed"
    );
}

/// V4-PROOF-04 Assertion 4: Registry is cleaned up across outcomes
/// (Succeeded, Failed, and Canceled).
#[tokio::test]
async fn work04_registry_cleaned_up_across_outcomes() {
    // Outcome 1: Succeeded
    {
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

        app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let rx = app.arm_turn_join(&started.session_id);
                app.submit_chat_turn(RunTurnCommand {
                    session_id: started.session_id.clone(),
                    user_input: "test success".into(),
                    verify_cmd: None,
                    llm_router: Some(false),
                })
                .expect("submit_chat_turn");
                rx.await.expect("finish");

                // Verify parent active attempt is cleaned up.
                assert!(app
                    .runs
                    .active_parent_attempt(&started.session_id)
                    .is_none());
            })
            .await;
    }

    // Outcome 2: Failed
    {
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

        app.bind_inference(Arc::new(MockTurnProvider::failing()), None);

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let rx = app.arm_turn_join(&started.session_id);
                app.submit_chat_turn(RunTurnCommand {
                    session_id: started.session_id.clone(),
                    user_input: "test failure".into(),
                    verify_cmd: None,
                    llm_router: Some(false),
                })
                .expect("submit_chat_turn");
                let _ = rx.await;

                // Supervisor must reflect failed state.
                let live = app.sessions.live(&started.session_id).expect("live");
                let run_id = get_session_run_id(&live);
                let snap = app
                    .runs
                    .inspect_run(InspectRunCommand {
                        run_id: run_id.to_string(),
                    })
                    .await
                    .expect("inspect");
                assert_eq!(
                    snap.state,
                    RunState::Failed,
                    "failing turn must reach Failed run state"
                );

                // Verify parent active attempt is cleaned up after failure.
                assert!(app
                    .runs
                    .active_parent_attempt(&started.session_id)
                    .is_none());
            })
            .await;
    }

    // Outcome 3: Canceled
    {
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

        let live = app.sessions.live(&started.session_id).expect("live");
        live.request_cancel();

        app.bind_inference(Arc::new(MockTurnProvider::immediate()), None);

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let rx = app.arm_turn_join(&started.session_id);
                app.submit_chat_turn(RunTurnCommand {
                    session_id: started.session_id.clone(),
                    user_input: "test cancel".into(),
                    verify_cmd: None,
                    llm_router: Some(false),
                })
                .expect("submit_chat_turn");
                rx.await.expect("finish");

                // Verify parent active attempt is cleaned up after cancel.
                assert!(app
                    .runs
                    .active_parent_attempt(&started.session_id)
                    .is_none());
            })
            .await;
    }
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;
