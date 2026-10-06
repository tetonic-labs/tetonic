use super::*;

fn reply() -> Value {
    json!({"status":"completed","output":[
        {"type":"reasoning","id":"reason-1","summary":[],"encrypted_content":"opaque-state"},
        {"type":"function_call","call_id":"provider-read","name":"read_file","arguments":"{\"path\":\"notes.txt\"}"},
        {"type":"function_call","call_id":"provider-list","name":"list_dir","arguments":"{}"}
    ],"usage":{"input_tokens":11,"output_tokens":7}})
}

#[test]
fn tool_results_keep_provider_ids_and_opaque_continuation_including_out_of_order_results() {
    let assistant = response(reply(), "model").unwrap();
    assert_eq!(
        assistant.message.provider_tool_call_ids(),
        vec!["provider-read", "provider-list"]
    );
    assert_eq!(assistant.usage.eval_tokens, Some(7));
    let req = ChatRequest {
        model: "model".into(),
        messages: vec![
            Message::system("Use selected tools"),
            Message::user("Read notes"),
            assistant.message,
            Message::tool("list_dir", "notes.txt").with_tool_call_id("provider-list"),
            Message::tool("read_file", "actual contents").with_tool_call_id("provider-read"),
        ],
        ..Default::default()
    };
    let body = request(&req, &HostedModelConfig::responses("model", 100)).unwrap();
    assert_eq!(body["input"][0]["role"], "developer");
    assert_eq!(body["input"][2], reply()["output"][0]);
    assert_eq!(body["input"][5]["call_id"], "provider-list");
    assert_eq!(body["input"][6]["call_id"], "provider-read");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert!(body.get("temperature").is_none());
    assert!(!serde_json::to_string(&req.messages)
        .unwrap()
        .contains("opaque-state"));
    assert!(!format!("{:?}", req.messages).contains("opaque-state"));
}

#[test]
fn missing_or_wrong_results_and_cross_model_continuation_are_rejected() {
    for result in [
        Message::user("continue"),
        Message::tool("read_file", "answer").with_tool_call_id("wrong"),
    ] {
        let req = ChatRequest {
            model: "model".into(),
            messages: vec![response(reply(), "model").unwrap().message, result],
            ..Default::default()
        };
        assert!(request(&req, &HostedModelConfig::responses("model", 100)).is_err());
    }
    let req = ChatRequest {
        model: "other".into(),
        messages: vec![response(reply(), "model").unwrap().message],
        ..Default::default()
    };
    assert!(request(&req, &HostedModelConfig::responses("other", 100)).is_err());
}

#[test]
fn incomplete_refused_and_duplicate_call_responses_are_rejected() {
    let mut incomplete = reply();
    incomplete["status"] = json!("incomplete");
    let mut duplicate = reply();
    duplicate["output"][2]["call_id"] = json!("provider-read");
    let refused = json!({"status":"completed","output":[{"type":"message","status":"completed","role":"assistant","content":[{"type":"refusal","refusal":"no"}]}]});
    for value in [incomplete, duplicate, refused] {
        assert!(response(value, "model").is_err());
    }
}
