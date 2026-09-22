//! Regression coverage for the September 19 adversarial audit.
use lokai_rpc::outbound::{OutboundClass, OutboundQueue};

#[test]
fn coalescing_preserves_ordered_deltas() {
    let (queue, _wake) = OutboundQueue::new(8);
    for delta in ["Hello ", "world"] {
        queue.enqueue(
            OutboundClass::Coalesce,
            serde_json::json!({"params":{"delta":delta}}).to_string(),
            Some(("session".into(), "agent".into())),
            None,
        );
    }
    let frames = queue.drain_for_write();
    assert_eq!(frames.len(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&frames[0]).unwrap()["params"]["delta"],
        "Hello world"
    );
}

#[test]
fn standard_queue_and_wake_notifications_are_bounded() {
    let (queue, wake) = OutboundQueue::new(1);
    for _ in 0..1000 {
        queue.enqueue(OutboundClass::Standard, "{}".into(), None, None);
    }
    assert_eq!(queue.queued_count(), 1);
    assert_eq!(wake.len(), 1);
}

#[tokio::test]
async fn oversized_header_is_rejected_before_body() {
    let raw = format!(
        "X-Header: {}\r\nContent-Length: 2\r\n\r\n{{}}",
        "a".repeat(1024 * 1024)
    );
    let mut reader = tokio::io::BufReader::new(raw.as_bytes());
    assert!(lokai_rpc::framing::read_frame(&mut reader).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn yielding_turn_retains_its_own_trace() {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let a = lokai_telemetry::propagation::scope_context(Default::default(), async {
        lokai_telemetry::inject_turn_context("session_a", "run_a", None);
        ready_tx.send(()).unwrap();
        resume_rx.await.unwrap();
        lokai_telemetry::propagation::extract_context().unwrap()
    });
    let b = lokai_telemetry::propagation::scope_context(Default::default(), async {
        ready_rx.await.unwrap();
        lokai_telemetry::inject_turn_context("session_b", "run_b", None);
        resume_tx.send(()).unwrap();
    });
    let (observed, ()) = tokio::join!(a, b);
    assert_eq!(observed.session_id.as_deref(), Some("session_a"));
}
