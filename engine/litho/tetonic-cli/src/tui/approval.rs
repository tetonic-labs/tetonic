//! Consent dialog for gated tools (not a warning splash).

use tetonic_app::ConfinementWarning;

#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub session_id: String,
    pub approval_id: String,
    pub kind: String,
    pub detail: String,
    pub missing_controls: Vec<ConfinementWarning>,
    pub user_approval_required: bool,
}

pub fn dialog_text(pending: &PendingApproval) -> String {
    let mut body = String::from("Approve this action\n\n");
    body.push_str(&pending.kind);
    body.push_str("\n\n");
    body.push_str(&pending.detail);
    body.push('\n');
    if !pending.missing_controls.is_empty() {
        body.push_str("\nRisks\n");
        for c in &pending.missing_controls {
            body.push_str(&format!(
                "  [{}] {}\n    {}\n",
                c.risk_level, c.control, c.reason
            ));
        }
    }
    if pending.user_approval_required {
        body.push_str(
            "\nAlways allow is off — this command can reach the network and files this user can.\n\n",
        );
    }
    body
}

#[cfg(test)]
mod tests {
    use super::super::{deny_pending, events, respond_approval, App};
    use crate::app_kernel::TerminalApprovalCoordinator;
    use std::sync::Arc;
    use tetonic_app::approval::{ApprovalService, DefaultApprovalService};
    use tetonic_app::commands::RegisterApprovalCommand;
    use tetonic_app::events::{ApplicationEvent, ApplicationEventSink};

    struct Sink(crate::event_queue::Sender);
    impl ApplicationEventSink for Sink {
        fn send(&self, event: ApplicationEvent) {
            self.0.send(event).unwrap();
        }
    }

    fn setup() -> (
        App,
        Arc<TerminalApprovalCoordinator>,
        Arc<DefaultApprovalService>,
        crate::event_queue::Receiver,
    ) {
        let (tx, rx) = crate::event_queue::channel();
        let svc = Arc::new(DefaultApprovalService::new(None, Arc::new(Sink(tx))));
        let coordinator = TerminalApprovalCoordinator::new();
        coordinator.bind_approvals(svc.clone());
        (App::test_stub(), coordinator, svc, rx)
    }

    fn register(
        app: &App,
        svc: &DefaultApprovalService,
        id: &str,
    ) -> tokio::sync::oneshot::Receiver<bool> {
        svc.register_request(RegisterApprovalCommand {
            session_id: app.session_id.clone(),
            approval_id: id.into(),
            call_id: id.into(),
            kind: "run_shell".into(),
            detail: format!("command {id}"),
            tool: "run_shell".into(),
            args: serde_json::json!({}),
            missing_controls: vec![],
            user_approval_required: true,
            auto_grant_approvals: false,
            attempt_id: None,
        })
        .unwrap()
    }

    #[test]
    fn concurrent_requests_preserve_visible_identity_and_cancel_all_remaining_waits() {
        let (mut app, coordinator, svc, mut rx) = setup();
        let mut first = register(&app, &svc, "first");
        let mut second = register(&app, &svc, "second");
        let mut third = register(&app, &svc, "third");
        while let Some(event) = rx.try_recv() {
            events::apply(&mut app, event, &coordinator);
        }
        assert_eq!(app.pending_approval.as_ref().unwrap().approval_id, "first");
        assert_eq!(app.queued_approvals.len(), 2);
        app.approval_selection = 2;
        respond_approval(&mut app, &coordinator, true, false);
        assert!(first.try_recv().unwrap());
        assert!(second.try_recv().is_err());
        assert_eq!(app.pending_approval.as_ref().unwrap().approval_id, "second");
        assert_eq!(app.approval_selection, 0);
        deny_pending(&mut app, &coordinator);
        assert!(!second.try_recv().unwrap());
        assert!(!third.try_recv().unwrap());
        assert!(app.pending_approval.is_none());
        assert!(app.queued_approvals.is_empty());
    }

    #[test]
    fn failed_response_keeps_visible_request_and_reports_error() {
        let (mut app, coordinator, svc, mut rx) = setup();
        let mut result = register(&app, &svc, "stale");
        events::apply(&mut app, rx.try_recv().unwrap(), &coordinator);
        svc.fail_session_waits(&app.session_id);
        respond_approval(&mut app, &coordinator, true, false);
        assert!(!result.try_recv().unwrap());
        assert_eq!(app.pending_approval.as_ref().unwrap().approval_id, "stale");
        assert!(app
            .status_hint
            .as_ref()
            .unwrap()
            .contains("Approval response failed"));
    }

    #[test]
    fn approval_overflow_fails_closed_and_resolves_every_waiter_as_denied() {
        let (mut app, coordinator, svc, mut rx) = setup();
        let mut results = (0..34)
            .map(|i| register(&app, &svc, &i.to_string()))
            .collect::<Vec<_>>();
        while let Some(event) = rx.try_recv() {
            events::apply(&mut app, event, &coordinator);
        }
        assert!(app.portal_error.is_some());
        assert!(app.pending_approval.is_none());
        assert!(app.queued_approvals.is_empty());
        for result in &mut results {
            assert!(!result.try_recv().unwrap());
        }
    }
}
