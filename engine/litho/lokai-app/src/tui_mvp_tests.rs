//! Product-door tests with real management, tools, and brokered HTTP inference.
use crate::{commands::*, events::*, Application, ComputePlaneRequest};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn inference_server() -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            let header = String::from_utf8_lossy(&bytes[..header_end]);
            let response = if header.starts_with("POST /api/chat ") {
                let request: Value =
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                let mut requests = captured.lock().unwrap();
                requests.push(request);
                let message = if requests.len() == 1 {
                    json!({"role":"assistant", "content":"", "tool_calls":[{"function":{"name":"read_file","arguments":{"path":"fixture.txt"}}}]})
                } else {
                    json!({"role":"assistant", "content":"The fixture contains cobalt orchard."})
                };
                json!({"model":"qwen3.5:latest", "message":message, "done":true})
            } else if header.starts_with("POST /api/generate ") {
                json!({"model":"qwen3.5:latest", "done":true, "response":""})
            } else {
                json!({"models":[{"name":"qwen3.5:latest", "model":"qwen3.5:latest", "size":1, "size_vram":1}], "capabilities":["completion","tools"]})
            };
            let body = format!("{response}\n");
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (url, requests, task)
}

#[tokio::test]
async fn tui_mvp_read_tool_and_followup_through_compute_broker() {
    for mode in ["off", "auto"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("fixture.txt"), "cobalt orchard").unwrap();
        let store = lokai_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
        let (sink, events) = RecordingEventSink::new();
        let app =
            Application::bootstrap_mock_with_store(dir.path(), Some(store.clone()), sink, vec![]);
        let (url, requests, server) = inference_server().await;
        let guard = Arc::new(lokai_egress::EgressGuard::new());
        guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
        let plane = crate::build_compute_plane(ComputePlaneRequest {
            guard,
            ollama_base: url,
            policy: Arc::new(lokai_policy::PolicyEngine::default()),
            workspace_root: dir.path().into(),
            artifact_store: app.turn.runtime.artifact_store().clone(),
            store: Some(store),
            coordinator: None,
            placement_sink: None,
            previous_pooled: None,
        })
        .await;
        app.install_compute_services(&plane);
        let pooled = plane.pooled.as_ref().unwrap();
        let epoch = pooled
            .policy_epoch()
            .load(std::sync::atomic::Ordering::Relaxed);
        app.bump_policy_epoch();
        assert!(
            pooled
                .policy_epoch()
                .load(std::sync::atomic::Ordering::Relaxed)
                > epoch,
            "product compute controls must reference the installed inference plane"
        );
        let session = app
            .sessions
            .start_session(StartSessionCommand {
                workspace_root: dir.path().display().to_string(),
                briefing: Some(false),
                orchestration: Some(mode.into()),
                critic: Some(false),
                force_explain: Some(true),
                model_fast: Some("qwen3.5:latest".into()),
                model_hard: Some("qwen3.5:latest".into()),
                session_max_steps: Some(8),
                ..Default::default()
            })
            .await
            .unwrap();
        tokio::task::LocalSet::new()
            .run_until(async {
                for (index, input) in [
                    "Read fixture.txt and tell me its contents",
                    "What did the fixture contain?",
                ]
                .iter()
                .enumerate()
                {
                    // Like the TUI, observe events without registering a completion waiter.
                    app.submit_chat_turn(RunTurnCommand {
                        session_id: session.session_id.clone(),
                        user_input: input.to_string(),
                        verify_cmd: None,
                        llm_router: Some(false),
                    })
                    .unwrap();
                    tokio::time::timeout(std::time::Duration::from_secs(15), async {
                        loop {
                            if events
                                .lock()
                                .unwrap()
                                .iter()
                                .filter(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
                                .count()
                                > index
                            {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .expect("product must report completion");
                }
            })
            .await;
        server.abort();
        let events = events.lock().unwrap();
        let terminal: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let ApplicationEvent::TurnCompleted { status, error, .. } = e {
                    Some((status.as_str(), error))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(terminal.len(), 2);
        assert!(
            terminal.iter().all(|(status, _)| *status == "ok"),
            "{mode}: {terminal:?}"
        );
        assert!(events.iter().any(|e| matches!(e, ApplicationEvent::ToolResult { tool, ok: true, .. } if tool == "read_file")), "{mode}: {events:?}");
        let requests = requests.lock().unwrap();
        assert!(
            requests.len() >= 3,
            "tool result must cause another inference request"
        );
        assert!(requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool"
                && m["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("cobalt orchard")));
        assert!(requests.last().unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "assistant"
                && m["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("cobalt orchard")));
    }
}

#[tokio::test]
async fn tui_mvp_planning_failure_reports_error_and_releases_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.db");
    let store = lokai_memory::SharedStore::open(&path, 1).unwrap();
    let (sink, events) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(
        dir.path(),
        Some(store),
        sink,
        vec![crate::test_turn("Recovered", vec![])],
    );
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: dir.path().display().to_string(),
            briefing: Some(false),
            orchestration: Some("off".into()),
            force_explain: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_message BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT, 'injected planning failure'); END;").unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for index in 0..2 {
                app.submit_chat_turn(RunTurnCommand {
                    session_id: session.session_id.clone(),
                    user_input: "Hello".into(),
                    verify_cmd: None,
                    llm_router: Some(false),
                })
                .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        if events
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
                            .count()
                            > index
                        {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                if index == 0 {
                    connection
                        .execute_batch("DROP TRIGGER fail_message;")
                        .unwrap();
                }
            }
        })
        .await;
    let events = events.lock().unwrap();
    let terminal: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
        .collect();
    assert_eq!(terminal.len(), 2);
    assert!(
        matches!(terminal[0], ApplicationEvent::TurnCompleted { status, error: Some(error), run_id: None, .. } if status == "error" && error.contains("injected planning failure"))
    );
    assert!(
        matches!(terminal[1], ApplicationEvent::TurnCompleted { status, .. } if status == "ok"),
        "{terminal:?}"
    );
}

#[tokio::test]
async fn tui_mvp_execution_limit_reports_reason_without_a_join_waiter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fixture.txt"), "fixture").unwrap();
    let store = lokai_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
    let (sink, events) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(
        dir.path(),
        Some(store),
        sink,
        vec![crate::test_turn(
            "",
            vec![("read_file", serde_json::json!({"path": "fixture.txt"}))],
        )],
    );
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: dir.path().display().to_string(),
            briefing: Some(false),
            orchestration: Some("off".into()),
            critic: Some(false),
            force_explain: Some(true),
            session_max_steps: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "Read fixture.txt".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .unwrap();
            let reason = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    if let Some((status, error)) =
                        events.lock().unwrap().iter().find_map(|event| match event {
                            ApplicationEvent::TurnCompleted { status, error, .. } => {
                                Some((status.clone(), error.clone()))
                            }
                            _ => None,
                        })
                    {
                        assert_eq!(status, "error");
                        break error.expect("execution limits must include a reason");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            assert!(reason.contains("effort cap reached"), "{reason}");
            assert!(!app
                .sessions
                .live(&session.session_id)
                .unwrap()
                .turn_in_flight());
        })
        .await;
}

struct PanicProvider;
#[async_trait::async_trait]
impl lokai_inference::InferenceProvider for PanicProvider {
    async fn chat(
        &self,
        _: lokai_inference::ChatRequest,
        _: &mut lokai_inference::TokenSink<'_>,
    ) -> Result<lokai_inference::ChatResponse, lokai_inference::InferenceError> {
        panic!("injected provider panic")
    }
}

#[tokio::test]
async fn tui_mvp_dropped_execution_reports_failure_without_a_waiter() {
    let dir = tempfile::tempdir().unwrap();
    let store = lokai_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
    let (sink, events) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(dir.path(), Some(store), sink, vec![]);
    app.bind_inference(Arc::new(PanicProvider), None);
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: dir.path().display().to_string(),
            briefing: Some(false),
            orchestration: Some("off".into()),
            force_explain: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            app.submit_chat_turn(RunTurnCommand {
                session_id: session.session_id.clone(),
                user_input: "Hello".into(),
                verify_cmd: None,
                llm_router: Some(false),
            })
            .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if events
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
    let live = app.sessions.live(&session.session_id).unwrap();
    let conversation = live
        .take_conversation()
        .expect("panic must release conversation");
    live.restore_conversation(conversation);
    let events = events.lock().unwrap();
    let terminal: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, ApplicationEvent::TurnCompleted { .. }))
        .collect();
    assert_eq!(terminal.len(), 1);
    assert!(
        matches!(terminal[0], ApplicationEvent::TurnCompleted { status, error: Some(error), run_id: Some(_), .. } if status == "error" && error.contains("dropped"))
    );
}

struct WaitingProvider(tokio::sync::mpsc::UnboundedSender<()>);
#[async_trait::async_trait]
impl lokai_inference::InferenceProvider for WaitingProvider {
    async fn chat(
        &self,
        _: lokai_inference::ChatRequest,
        _: &mut lokai_inference::TokenSink<'_>,
    ) -> Result<lokai_inference::ChatResponse, lokai_inference::InferenceError> {
        self.0.send(()).unwrap();
        std::future::pending().await
    }
}

#[tokio::test]
async fn tui_mvp_cancel_retry_and_close_active_turn_through_product() {
    let dir = tempfile::tempdir().unwrap();
    let store = lokai_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
    let (sink, events) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(dir.path(), Some(store), sink, vec![]);
    let session = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: dir.path().display().to_string(),
            briefing: Some(false),
            orchestration: Some("off".into()),
            force_explain: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    let submit = || RunTurnCommand {
        session_id: session.session_id.clone(),
        user_input: "Hello".into(),
        verify_cmd: None,
        llm_router: Some(false),
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            let (tx, mut entered) = tokio::sync::mpsc::unbounded_channel();
            app.bind_inference(Arc::new(WaitingProvider(tx)), None);
            let joined = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(submit()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), entered.recv())
                .await
                .unwrap()
                .unwrap();
            app.cancel_chat_turn(&session.session_id).await.unwrap();
            let finished = tokio::time::timeout(std::time::Duration::from_secs(5), joined)
                .await
                .unwrap()
                .unwrap();
            assert!(finished.canceled);
            // Cancellation must not poison the next conversation turn.
            app.bind_inference(
                Arc::new(crate::MockProvider::new(vec![crate::test_turn(
                    "Hello again",
                    vec![],
                )])),
                None,
            );
            let joined = app.arm_turn_join(&session.session_id);
            app.submit_chat_turn(submit()).unwrap();
            let finished = tokio::time::timeout(std::time::Duration::from_secs(5), joined)
                .await
                .unwrap()
                .unwrap();
            assert!(finished.ok, "{finished:?}");
            // Close while inference is pending: product drains before SessionEnded.
            let (tx, mut entered) = tokio::sync::mpsc::unbounded_channel();
            app.bind_inference(Arc::new(WaitingProvider(tx)), None);
            app.submit_chat_turn(submit()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), entered.recv())
                .await
                .unwrap()
                .unwrap();
            let early_close = app.sessions.end_session(EndSessionCommand {
                session_id: session.session_id.clone(),
                workspace_root: dir.path().display().to_string(),
                status: Some("ok".into()),
                error: None,
            });
            assert!(
                early_close.is_err(),
                "low-level cleanup must not bypass draining"
            );
            assert!(app
                .sessions
                .live(&session.session_id)
                .unwrap()
                .turn_in_flight());
            assert!(!events
                .lock()
                .unwrap()
                .iter()
                .any(|event| matches!(event, ApplicationEvent::SessionEnded { .. })));
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                app.close_session(EndSessionCommand {
                    session_id: session.session_id.clone(),
                    workspace_root: dir.path().display().to_string(),
                    status: Some("ok".into()),
                    error: None,
                }),
            )
            .await
            .unwrap()
            .unwrap();
        })
        .await;
    assert!(app.sessions.live(&session.session_id).is_err());
    let events = events.lock().unwrap();
    let completions: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| matches!(e, ApplicationEvent::TurnCompleted { .. }).then_some(i))
        .collect();
    assert_eq!(completions.len(), 3);
    let ended = events
        .iter()
        .position(|e| matches!(e, ApplicationEvent::SessionEnded { .. }))
        .unwrap();
    assert!(completions[2] < ended);
}
