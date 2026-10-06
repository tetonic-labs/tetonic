use super::*;

fn completion(parts: Value) -> Value {
    json!({"candidates":[{"finishReason":"STOP","content":{"role":"model","parts":parts}}],"usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":6,"thoughtsTokenCount":4}})
}

#[test]
fn functions_preserve_signatures_and_correlate_actual_results() {
    for with_id in [true, false] {
        let mut function = json!({"name":"read_file","args":{"path":"notes.txt"}});
        if with_id {
            function["id"] = json!("provider-call");
        }
        let parts = json!([{"text":"private thinking","thought":true},{"functionCall":function,"thoughtSignature":"opaque-signature"}]);
        let response = response(&completion(parts.clone()), "fixture-model").unwrap();
        assert_eq!(response.usage.eval_tokens, Some(10));
        assert!(response.message.content.is_empty());
        assert!(!serde_json::to_string(&response.message)
            .unwrap()
            .contains("private thinking"));
        assert!(!format!("{:?}", response.message).contains("opaque-signature"));
        let id = response.message.provider_state.as_ref().unwrap().call_ids[0].clone();
        let mut req = ChatRequest {
            model: "fixture-model".into(),
            messages: vec![Message::user("Read"), response.message],
            tools: vec![crate::ToolSchema::function(
                "read_file",
                "Read",
                json!({"type":"object"}),
            )],
            ..Default::default()
        };
        let config = HostedModelConfig::google("fixture-model", 128);
        assert!(
            request(&req, &config).is_err(),
            "no invented external results"
        );
        req.messages
            .push(Message::tool("read_file", "actual file contents").with_tool_call_id(&id));
        let body = request(&req, &config).unwrap();
        assert_eq!(body["contents"][1]["parts"], parts);
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]["response"]["output"],
            "actual file contents"
        );
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]
                .get("id")
                .is_some(),
            with_id
        );
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["type"],
            "object"
        );
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 128);
        req.messages.last_mut().unwrap().tool_call_id = Some("unknown".into());
        assert!(request(&req, &config).is_err());
        req.model = "other-model".into();
        assert!(request(&req, &HostedModelConfig::google("other-model", 128)).is_err());
    }
}

#[test]
fn incomplete_blocked_duplicate_and_unsupported_results_are_rejected() {
    let good = completion(json!([{"functionCall":{"id":"one","name":"read_file","args":{}}}]));
    for reason in ["MAX_TOKENS", "SAFETY", "MALFORMED_FUNCTION_CALL", "OTHER"] {
        let mut value = good.clone();
        value["candidates"][0]["finishReason"] = json!(reason);
        assert!(response(&value, "m").is_err());
    }
    assert!(response(&completion(json!([{"functionCall":{"id":"one","name":"read_file"}},{"functionCall":{"id":"one","name":"read_file"}}])),"m").is_err());
    assert!(response(
        &completion(json!([{"executableCode":{"code":"ungranted execution"}}])),
        "m"
    )
    .is_err());
    for parts in [
        json!([{"text":"invalid thought flag","thought":"true"}]),
        json!([{"functionCall":{"name":"read_file","args":{}},"executableCode":{"code":"hidden"}}]),
        json!([{"functionCall":{"name":"read_file","args":{}},"thought":true}]),
    ] {
        assert!(response(&completion(parts), "m").is_err());
    }
    assert!(response(&completion(json!([{"text":"private","thought":true}])), "m").is_err());
}

#[test]
fn parallel_same_name_calls_return_by_id_and_finish_is_internal() {
    let msg=response(&completion(json!([{"functionCall":{"id":"a","name":"read_file","args":{"path":"a"}}},{"functionCall":{"id":"b","name":"read_file","args":{"path":"b"}}}])),"m").unwrap().message;
    let req = ChatRequest {
        model: "m".into(),
        messages: vec![
            Message::user("Read"),
            msg,
            Message::tool("read_file", "B").with_tool_call_id("b"),
            Message::tool("read_file", "A").with_tool_call_id("a"),
        ],
        ..Default::default()
    };
    let body = request(&req, &HostedModelConfig::google("m", 64)).unwrap();
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"]["id"],
        "b"
    );
    let msg = response(
        &completion(
            json!([{"functionCall":{"id":"done","name":"finish","args":{"summary":"done"}}}]),
        ),
        "m",
    )
    .unwrap()
    .message;
    let req = ChatRequest {
        model: "m".into(),
        messages: vec![Message::user("Read"), msg, Message::user("Again")],
        ..Default::default()
    };
    assert_eq!(
        request(&req, &HostedModelConfig::google("m", 64)).unwrap()["contents"][2]["parts"][0]
            ["functionResponse"]["response"]["output"],
        "Completed."
    );
}
