//! R4-3: ScannerEngine on tool/event edges and context seal wiring.

use serde_json::json;
use tetonic_core::Step;
use tetonic_secrets::ScannerEngine;

use crate::events::{ApplicationEvent, ApplicationEventSink, RecordingEventSink};

#[test]
fn tool_summary_secret_is_redacted() {
    let scanner = ScannerEngine::default_engine();
    let plant = "shell said AKIAIOSFODNN7EXAMPLE in stdout";
    let (out, hit) = tetonic_secrets::redact_text_sync(&scanner, plant).expect("scan");
    assert!(hit, "AWS key fixture must hit ScannerEngine");
    assert!(
        !out.contains("AKIAIOSFODNN7EXAMPLE"),
        "raw key must not survive redaction: {out}"
    );
}

#[test]
fn outbound_event_scanner_pair_builds() {
    let (scanner, _sink) = crate::turn_execution::outbound_event_scanner(&None);
    let (out, hit) = tetonic_secrets::redact_text_sync(
        &scanner,
        "password=SuperSecretRandomBase64StringWithManyChars123!",
    )
    .expect("scan");
    assert!(hit);
    assert!(!out.contains("SuperSecretRandomBase64StringWithManyChars123!"));
}

#[test]
fn missing_scanner_does_not_emit_payload() {
    let (rec, events) = RecordingEventSink::new();
    let sink: std::sync::Arc<dyn ApplicationEventSink> = rec;
    let secret = "AKIAIOSFODNN7EXAMPLE";
    crate::turn_execution::step_to_events(
        &sink,
        "sess",
        "a0",
        Step::Note(format!("leaked {secret}")),
        None,
        None,
        None,
    );
    crate::turn_execution::step_to_events(
        &sink,
        "sess",
        "a0",
        Step::ToolCall {
            call_id: "c1".into(),
            name: "write_file".into(),
            args: json!({ "body": secret }),
        },
        None,
        None,
        None,
    );
    let captured = events.lock().unwrap();
    assert!(
        captured.is_empty(),
        "missing scanner must not emit payload: {captured:?}"
    );
}

#[test]
fn tool_call_args_and_thoughts_go_through_scanner() {
    let scanner = ScannerEngine::default_engine();
    let (rec, events) = RecordingEventSink::new();
    let sink: std::sync::Arc<dyn ApplicationEventSink> = rec;
    let secret = "AKIAIOSFODNN7EXAMPLE";
    crate::turn_execution::step_to_events(
        &sink,
        "sess",
        "a0",
        Step::Thought(format!("remember {secret}")),
        Some(&scanner),
        None,
        None,
    );
    crate::turn_execution::step_to_events(
        &sink,
        "sess",
        "a0",
        Step::ToolCall {
            call_id: "c1".into(),
            name: "write_file".into(),
            args: json!({ "body": secret }),
        },
        Some(&scanner),
        None,
        None,
    );
    let captured = events.lock().unwrap().clone();
    let blob = format!("{captured:?}");
    assert!(
        !blob.contains(secret),
        "ToolCall.args / thoughts must not carry the secret: {blob}"
    );
    assert!(captured
        .iter()
        .any(|e| matches!(e, ApplicationEvent::ThoughtToken { .. })));
    assert!(captured
        .iter()
        .any(|e| matches!(e, ApplicationEvent::ToolCall { .. })));
}

#[test]
fn stopped_reason_does_not_repeat_a_secret() {
    let scanner = ScannerEngine::default_engine();
    let (rec, events) = RecordingEventSink::new();
    let sink: std::sync::Arc<dyn ApplicationEventSink> = rec;
    let secret = "AKIAIOSFODNN7EXAMPLE";
    crate::turn_execution::step_to_events(
        &sink,
        "sess",
        "a0",
        Step::Stopped(format!("finished: the key is {secret}")),
        Some(&scanner),
        None,
        None,
    );
    crate::turn_execution::step_to_events(
        &sink,
        "sess",
        "a0",
        Step::Stopped(format!("finished: the key is {secret}")),
        None,
        None,
        None,
    );
    let captured = events.lock().unwrap().clone();
    let blob = format!("{captured:?}");
    assert!(
        !blob.contains(secret),
        "stopped diagnostic repeated the secret: {blob}"
    );
    assert!(captured.iter().any(|event| matches!(
        event,
        ApplicationEvent::LogDiagnostic { message, .. } if message.starts_with("stopped:")
    )));
}

#[test]
fn failure_text_does_not_repeat_a_secret() {
    let scanner = ScannerEngine::default_engine();
    let secret = "AKIAIOSFODNN7EXAMPLE";
    let redacted = crate::turn_execution::redact_failure_text(
        Some(&scanner),
        None,
        "sess",
        format!("provider failed while echoing {secret}"),
    );
    assert!(
        !redacted.contains(secret),
        "failure text repeated the secret: {redacted}"
    );
    assert_eq!(
        crate::turn_execution::redact_failure_text(None, None, "sess", "file not found".into()),
        "file not found"
    );
}
