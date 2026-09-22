use lokai_app::approval::{ApprovalService, DefaultApprovalService};
use lokai_app::commands::{ApprovalResponseCommand, RegisterApprovalCommand};
use lokai_app::events::{ApplicationEvent, ApplicationEventSink};
use std::sync::{Arc, Mutex, Weak};

struct NoEvents;
impl ApplicationEventSink for NoEvents {
    fn send(&self, _: ApplicationEvent) {}
}
fn request(id: &str, session: &str) -> RegisterApprovalCommand {
    RegisterApprovalCommand {
        session_id: session.into(),
        approval_id: id.into(),
        call_id: format!("call_{id}"),
        kind: "run_shell".into(),
        detail: "echo original".into(),
        tool: "run_shell".into(),
        args: serde_json::json!({"command":"echo original"}),
        missing_controls: vec![],
        user_approval_required: false,
        auto_grant_approvals: false,
        attempt_id: Some("attempt_A".into()),
    }
}
fn reply(id: &str, session: &str) -> ApprovalResponseCommand {
    ApprovalResponseCommand {
        session_id: session.into(),
        approval_id: id.into(),
        approved: true,
        remember: true,
        kind: "forged_kind".into(),
        detail: "forged_detail".into(),
        channel_delivered: true,
        attempt_id: None,
    }
}
#[tokio::test]
async fn response_must_match_registered_session_and_optional_attempt() {
    let approvals = DefaultApprovalService::new(None, Arc::new(NoEvents));
    let mut rx = approvals
        .register_request(request("id", "session_A"))
        .unwrap();
    assert!(approvals.respond(reply("id", "session_B")).is_err());
    let mut mismatch = reply("id", "session_A");
    mismatch.attempt_id = Some("attempt_B".into());
    assert!(approvals.respond(mismatch).is_err());
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    assert!(approvals
        .register_request(request("id", "session_A"))
        .is_err());
    // Daemon-shaped reply: no Attempt ID and no trusted response metadata.
    assert!(approvals.respond(reply("id", "session_A")).unwrap());
    assert!(rx.await.unwrap());
    assert!(approvals.respond(reply("id", "session_A")).is_err());
    assert!(approvals.respond(reply("unknown", "session_A")).is_err());
    assert!(approvals
        .register_request(request("id", "session_A"))
        .is_err());
}
#[tokio::test]
async fn remembered_rule_comes_from_request_not_response() {
    let tmp = tempfile::tempdir().unwrap();
    let store = lokai_memory::SharedStore::open(tmp.path().join("approval.db"), 1).unwrap();
    let session = store
        .write(|db| db.start_session("workspace", "agent", "model"))
        .await
        .unwrap()
        .unwrap();
    let approvals = DefaultApprovalService::new(Some(store.clone()), Arc::new(NoEvents));
    let rx = approvals
        .register_request(request("remember", &session))
        .unwrap();
    assert!(approvals.respond(reply("remember", &session)).unwrap());
    assert!(rx.await.unwrap());
    store
        .read(|db| {
            assert!(db
                .approval_rule_matches("run_shell", "echo original")
                .unwrap());
            assert!(!db
                .approval_rule_matches("forged_kind", "forged_detail")
                .unwrap());
        })
        .await
        .unwrap();
}
struct ImmediateSink(Mutex<Weak<DefaultApprovalService>>);
impl ApplicationEventSink for ImmediateSink {
    fn send(&self, event: ApplicationEvent) {
        if let ApplicationEvent::ApprovalRequest {
            session_id,
            approval_id,
            ..
        } = event
        {
            let service = self.0.lock().unwrap().upgrade().unwrap();
            assert!(service.respond(reply(&approval_id, &session_id)).unwrap());
        }
    }
}
#[tokio::test]
async fn published_request_already_has_a_registered_waiter() {
    let sink = Arc::new(ImmediateSink(Mutex::new(Weak::new())));
    let service = Arc::new(DefaultApprovalService::new(None, sink.clone()));
    *sink.0.lock().unwrap() = Arc::downgrade(&service);
    let rx = service
        .register_request(request("immediate", "session_A"))
        .unwrap();
    assert!(rx.await.unwrap());
}
