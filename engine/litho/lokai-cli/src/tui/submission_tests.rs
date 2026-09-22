use super::*;

#[test]
fn saturated_queue_preserves_draft_and_does_not_start_a_turn() {
    let mut app = App::test_stub();
    let (tx, mut rx) = mpsc::channel(1);
    tx.try_send(TuiAction::Submit("/first".into())).unwrap();
    app.input_tx = tx;
    let lines = app.transcript.len();
    let draft = "Review this\nwith Unicode: café";
    on_submit(&mut app, draft.into());
    assert_eq!(app.input_buffer, draft);
    assert_eq!(app.input_cursor, draft.chars().count());
    assert_eq!(app.transcript.len(), lines);
    assert!(!app.thinking);
    assert!(app.status_hint.as_ref().unwrap().contains("queue is busy"));
    assert!(matches!(rx.try_recv(), Ok(TuiAction::Submit(text)) if text == "/first"));
    on_submit(&mut app, draft.into());
    assert!(app.thinking);
    assert!(matches!(rx.try_recv(), Ok(TuiAction::Submit(text)) if text == draft));
    assert_eq!(
        app.transcript
            .iter()
            .filter(|line| line.text == draft)
            .count(),
        1
    );
}

#[test]
fn closed_queue_preserves_draft_and_reports_unavailable_handler() {
    let mut app = App::test_stub();
    // The stub drops its receiver.
    on_submit(&mut app, "unsent".into());
    assert!(!app.thinking);
    assert_eq!(app.input_buffer, "unsent");
    assert!(app.status_hint.as_ref().unwrap().contains("unavailable"));
}

#[test]
fn oversized_submission_is_not_admitted() {
    let mut app = App::test_stub();
    let (tx, mut rx) = mpsc::channel(1);
    app.input_tx = tx;
    let draft = "x".repeat(1024 * 1024 + 1);
    on_submit(&mut app, draft.clone());
    assert_eq!(app.input_buffer, draft);
    assert!(!app.thinking);
    assert!(rx.try_recv().is_err());
    assert!(app.status_hint.as_ref().unwrap().contains("input limit"));
}
