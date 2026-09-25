//! Golden JSON-RPC v1 fixtures — request/response/notification/framing compatibility.

use serde_json::{json, Value};
use tetonic_app::errors::AppError;
use tetonic_rpc::framing;
use tetonic_rpc::outbound::{
    classify_outbound, OutboundClass, OutboundQueue, DEFAULT_OUTBOUND_CAPACITY,
};
use tetonic_rpc::protocol::*;

fn load_fixture(name: &str) -> Value {
    let raw = match name {
        "initialize_params" => include_str!("fixtures/initialize_params.json"),
        "initialize_result" => include_str!("fixtures/initialize_result.json"),
        "session_start_params" => include_str!("fixtures/session_start_params.json"),
        "session_start_result" => include_str!("fixtures/session_start_result.json"),
        "chat_send_params" => include_str!("fixtures/chat_send_params.json"),
        "rpc_error_unknown_session" => include_str!("fixtures/rpc_error_unknown_session.json"),
        "notification_run_status_ok" => include_str!("fixtures/notification_run_status_ok.json"),
        "notification_token" => include_str!("fixtures/notification_token.json"),
        "notification_approval_request" => {
            include_str!("fixtures/notification_approval_request.json")
        }
        other => panic!("unknown fixture {other}"),
    };
    serde_json::from_str(raw).expect(name)
}

fn roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(fixture: &str) {
    let v = load_fixture(fixture);
    let typed: T = serde_json::from_value(v).expect(fixture);
    let again: Value = serde_json::to_value(&typed).expect(fixture);
    let retyped: T = serde_json::from_value(again.clone()).expect(fixture);
    let again2 = serde_json::to_value(&retyped).expect(fixture);
    assert_eq!(again, again2, "serde roundtrip drift for {fixture}");
}

#[test]
fn golden_request_params_roundtrip() {
    roundtrip::<InitializeParams>("initialize_params");
    roundtrip::<SessionStartParams>("session_start_params");
    roundtrip::<ChatSendParams>("chat_send_params");
}

#[test]
fn golden_result_types_roundtrip() {
    roundtrip::<InitializeResult>("initialize_result");
    roundtrip::<SessionStartResult>("session_start_result");
}

#[test]
fn golden_rpc_error_shape() {
    let v = load_fixture("rpc_error_unknown_session");
    let err: RpcError = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(err.code, ErrorCode::UnknownSession.code());
    assert_eq!(err.message, "unknown session_id");
    let mapped = crate::daemon::rpc::map::map_app_error(AppError::SessionNotFound(
        "unknown session_id".into(),
    ));
    assert_eq!(mapped.code, err.code);
    assert_eq!(mapped.message, err.message);
}

#[test]
fn golden_notification_wire_methods() {
    for (fixture, expected_method) in [
        ("notification_run_status_ok", events::RUN_STATUS),
        ("notification_token", events::TOKEN),
        ("notification_approval_request", events::APPROVAL_REQUEST),
    ] {
        let v = load_fixture(fixture);
        assert_eq!(
            v.get("method").and_then(|m| m.as_str()),
            Some(expected_method),
            "{fixture} method wire name"
        );
        assert_eq!(v.get("jsonrpc").and_then(|m| m.as_str()), Some("2.0"));
        let params = v.get("params").expect("params");
        assert!(params.get("session_id").is_some());
        assert!(params.get("agent_id").is_some());
        assert!(params.get("seq").is_some());
    }
}

#[test]
fn golden_framing_content_length_roundtrip() {
    let payload = load_fixture("notification_token");
    let body = serde_json::to_string(&payload).unwrap();
    let framed = framing::frame(body.as_bytes());
    assert!(
        framed.starts_with(b"Content-Length:"),
        "frame must use Content-Length header"
    );
    let header_end = framed
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("CRLF header terminator");
    let header = std::str::from_utf8(&framed[..header_end]).unwrap();
    let len: usize = header
        .strip_prefix("Content-Length:")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(len, body.len());
    assert_eq!(&framed[header_end + 4..], body.as_bytes());
}

#[test]
fn golden_schema_bundle_wire_names_stable() {
    let bundle = tetonic_rpc::schema_bundle();
    assert_eq!(bundle["protocol_version"], tetonic_rpc::PROTOCOL_VERSION);
    assert_eq!(bundle["methods"]["CHAT_SEND"], methods::CHAT_SEND);
    assert_eq!(bundle["methods"]["INITIALIZE"], methods::INITIALIZE);
    assert_eq!(bundle["methods"]["RUN_SNAPSHOT"], methods::RUN_SNAPSHOT);
    assert_eq!(bundle["methods"]["RUN_RESUME"], methods::RUN_RESUME);
    assert_eq!(bundle["methods"]["RUN_CANCEL"], methods::RUN_CANCEL);
    assert_eq!(bundle["events"]["RUN_STATUS"], events::RUN_STATUS);
    assert_eq!(bundle["events"]["TOKEN"], events::TOKEN);
}

#[test]
fn run_handlers_do_not_call_supervisor_snapshot() {
    let src = include_str!("../handlers/run.rs");
    assert!(
        !src.contains(".supervisor.snapshot"),
        "run/snapshot must go through Application inspect_run"
    );
    assert!(
        !src.contains(".supervisor.resume_from_sequence"),
        "run/resume must go through Application resume_events"
    );
}

#[test]
fn transport_outbound_backpressure_policy() {
    // Documented transport failure classes (AC2-10) — lossy drops, terminal never coalesced away.
    assert_eq!(classify_outbound(events::LOG, false), OutboundClass::Lossy);
    assert_eq!(
        classify_outbound(events::TOKEN, false),
        OutboundClass::Coalesce
    );
    assert_eq!(
        classify_outbound(methods::CHAT_SEND, true),
        OutboundClass::Terminal
    );

    let (q, _wake) = OutboundQueue::new(1);
    let lossy = json!({"jsonrpc":"2.0","method":events::LOG,"params":{}}).to_string();
    assert!(q.enqueue(OutboundClass::Lossy, lossy.clone(), None, None));
    assert!(
        !q.enqueue(OutboundClass::Lossy, lossy, None, None),
        "lossy class drops when queue full"
    );

    let (q2, _wake2) = OutboundQueue::new(DEFAULT_OUTBOUND_CAPACITY);
    let terminal = json!({"jsonrpc":"2.0","id":1,"result":{}}).to_string();
    assert!(q2.enqueue(OutboundClass::Terminal, terminal, None, None));
}
