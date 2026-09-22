use super::*;
use crate::{FabricCallMeta, FunctionCall, Message, OutboundScan, ToolCall, ToolSchema};
use serde_json::json;
use std::sync::Mutex;
use tetonic_domain::secrets::ScanOutcome;

fn config() -> HostedModelConfig {
    HostedModelConfig {
        protocol: HostedWireProtocol::OpenAiChatCompletions,
        model: "configured-model".into(),
        allowed_models: vec!["configured-model".into()],
        max_output_tokens: 512,
        output_limit_field: OutputLimitField::MaxTokens,
        supports_tools: true,
        supports_json_schema: false,
        send_temperature: false,
    }
}
fn request() -> ChatRequest {
    ChatRequest {
        model: "configured-model".into(),
        messages: vec![Message::user("hello")],
        fabric: Some(FabricCallMeta {
            data_class: DataClass::RepositorySource,
            ..Default::default()
        }),
        ..Default::default()
    }
}
fn answer() -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"hello back"}}],
        "usage":{"prompt_tokens":12,"completion_tokens":3}})
}
struct Scanner;
#[async_trait]
impl SecretScanner for Scanner {
    async fn scan_and_redact(&self, text: &str, _: Option<&str>) -> Result<ScanOutcome, String> {
        if text.contains("scan-failure") {
            return Err("untrusted error containing sensitive data".into());
        }
        Ok(text
            .contains("hidden-credential")
            .then(|| (vec![], "redacted".into())))
    }
}
struct Transport {
    requests: Mutex<Vec<Value>>,
    reply: Value,
}
#[async_trait]
impl HostedTransport for Transport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        self.requests.lock().unwrap().push(body);
        Ok(self.reply.clone())
    }
}
fn provider(policy: HostedInferencePolicy, reply: Value) -> (HostedChatProvider, Arc<Transport>) {
    let transport = Arc::new(Transport {
        requests: Mutex::new(vec![]),
        reply,
    });
    (
        HostedChatProvider::new(config(), policy, Arc::new(Scanner), transport.clone()).unwrap(),
        transport,
    )
}
fn allowed() -> HostedInferencePolicy {
    HostedInferencePolicy::allow_up_to(DataClass::RepositorySource)
}

#[tokio::test]
async fn hosted_scan_receives_request_local_authority() {
    struct ScopedScanner;
    #[async_trait]
    impl SecretScanner for ScopedScanner {
        async fn scan_and_redact(&self, _: &str, _: Option<&str>) -> Result<ScanOutcome, String> {
            panic!("hosted adapter must pass request scope");
        }
        async fn scan_and_redact_in_context(
            &self,
            _: &str,
            _: Option<&str>,
            context: ScanContext<'_>,
        ) -> Result<ScanOutcome, String> {
            assert_eq!(context.session_id, Some("request-session"));
            assert_eq!(context.project_id, None);
            Ok(None)
        }
    }
    let transport = Arc::new(Transport {
        requests: Mutex::new(vec![]),
        reply: answer(),
    });
    let provider =
        HostedChatProvider::new(config(), allowed(), Arc::new(ScopedScanner), transport).unwrap();
    let mut req = request();
    req.fabric.as_mut().unwrap().session_id = Some("request-session".into());
    provider.chat(req, &mut |_| {}).await.unwrap();
}

#[tokio::test]
async fn complete_chat_uses_bound_model_and_budget_without_local_options() {
    let (provider, transport) = provider(allowed(), answer());
    let mut req = request();
    req.num_ctx = Some(8192);
    req.keep_alive = Some("30m".into());
    let mut text = String::new();
    let response = provider.chat(req, &mut |s| text.push_str(s)).await.unwrap();
    assert_eq!(text, "hello back");
    assert_eq!(response.usage.prompt_tokens, Some(12));
    assert_eq!(response.usage.eval_tokens, Some(3));
    assert_eq!(response.provenance.provider_kind, "hosted_chat_completions");
    let requests = transport.requests.lock().unwrap();
    let body = &requests[0];
    assert_eq!(body["max_tokens"], 512);
    assert_eq!(body["stream"], false);
    assert!(body.get("temperature").is_none());
    assert!(body.get("num_ctx").is_none());
    assert!(body.get("keep_alive").is_none());
    assert!(body.get("fabric").is_none());
}

#[tokio::test]
async fn policy_scan_and_capability_failures_send_nothing() {
    let mut cases = vec![request(); 7];
    cases[0].fabric.as_mut().unwrap().data_class = DataClass::Secret;
    cases[1].fabric.as_mut().unwrap().context_data_class = Some(DataClass::SensitiveSource);
    cases[2].outbound_scan = OutboundScan::from_scan(true);
    cases[3].tools = vec![ToolSchema::function("read", "hidden-credential", json!({}))];
    cases[4].messages[0].content = "scan-failure".into();
    cases[5].model = "unapproved-model".into();
    cases[6].response_format = Some(json!({"type":"object"}));
    for req in cases {
        let (provider, transport) = provider(allowed(), answer());
        let err = provider
            .chat(req, &mut |_| panic!("no output on denial"))
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("sensitive data"));
        assert!(transport.requests.lock().unwrap().is_empty());
    }
    let (provider, transport) = provider(HostedInferencePolicy::default(), answer());
    assert!(provider.chat(request(), &mut |_| {}).await.is_err());
    assert!(transport.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn tools_round_trip_with_core_owned_ordered_history() {
    let reply = json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,
        "tool_calls":[{"id":"vendor_1","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.rs\"}"}},
                      {"id":"vendor_2","type":"function","function":{"name":"read","arguments":"{\"path\":\"b.rs\"}"}}]}}]});
    let (provider, _) = provider(allowed(), reply);
    let response = provider.chat(request(), &mut |_| {}).await.unwrap();
    let mut req = request();
    req.messages.push(response.message);
    req.messages.push(Message::tool("read", "file a"));
    req.messages.push(Message::tool("read", "file b"));
    let body = wire::request(&req, &config()).unwrap();
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["id"],
        body["messages"][2]["tool_call_id"]
    );
    assert_eq!(
        body["messages"][1]["tool_calls"][1]["id"],
        body["messages"][3]["tool_call_id"]
    );
    assert_ne!(
        body["messages"][2]["tool_call_id"],
        body["messages"][3]["tool_call_id"]
    );
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"a.rs\"}"
    );
    req.messages.pop();
    assert!(wire::request(&req, &config()).is_err());
}

#[tokio::test]
async fn truncated_or_malformed_responses_publish_no_output() {
    let mut truncated = answer();
    truncated["choices"][0]["finish_reason"] = json!("length");
    let mut refused = answer();
    refused["choices"][0]["message"]["refusal"] = json!("refused");
    for reply in [
        truncated,
        refused,
        json!({"error":{"message":"private diagnostic"}}),
        json!({}),
    ] {
        let (provider, _) = provider(allowed(), reply);
        let err = provider
            .chat(request(), &mut |_| panic!("invalid response published"))
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("private diagnostic"));
    }
}

#[test]
fn capability_options_and_malformed_history() {
    let mut cfg = config();
    cfg.output_limit_field = OutputLimitField::MaxCompletionTokens;
    cfg.supports_json_schema = true;
    let mut req = request();
    req.response_format = Some(json!({"type":"object"}));
    let body = wire::request(&req, &cfg).unwrap();
    assert_eq!(body["max_completion_tokens"], 512);
    assert_eq!(
        body["response_format"]["json_schema"]["schema"],
        req.response_format.clone().unwrap()
    );
    req.messages.push(Message::tool("orphan", "result"));
    assert!(wire::request(&req, &cfg).is_err());
    req.messages.pop();
    req.messages
        .push(Message::assistant("").with_tool_calls(vec![ToolCall {
            function: FunctionCall {
                name: "read".into(),
                arguments: json!("not json"),
            },
        }]));
    assert!(wire::request(&req, &cfg).is_err());
}

#[test]
fn allowed_models_and_reasoning_model_formatting() {
    let mut cfg = config();
    cfg.allowed_models = vec!["configured-model".into(), "o1-test".into()];
    cfg.send_temperature = true;

    // Allowed model "o1-test" succeeds and reasoning settings apply:
    let mut req = request();
    req.model = "o1-test".into();
    req.temperature = 0.7;
    let body = wire::request(&req, &cfg).unwrap();
    assert_eq!(body["model"], "o1-test");
    assert_eq!(body["max_completion_tokens"], 512);
    assert!(body.get("temperature").is_none());

    // Disallowed model is rejected:
    req.model = "unapproved".into();
    let err = wire::request(&req, &cfg).unwrap_err();
    assert!(err.to_string().contains("does not match hosted binding"));
}

#[test]
fn multi_turn_finish_synthesizes_tool_result_in_wire_request() {
    let cfg = config();
    let mut req = request();
    req.messages = vec![
        Message::user("howdy"),
        Message::assistant("").with_tool_calls(vec![ToolCall {
            function: FunctionCall {
                name: "finish".into(),
                arguments: json!({"summary": "Hello! How can I help you?"}),
            },
        }]),
        Message::user("can you tell me what this codebase does"),
    ];
    let body = wire::request(&req, &cfg).unwrap();
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "howdy");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[2]["role"], "tool");
    assert_eq!(msgs[2]["content"], "Completed.");
    assert_eq!(msgs[2]["tool_call_id"], msgs[1]["tool_calls"][0]["id"]);
    assert_eq!(msgs[3]["role"], "user");
    assert_eq!(
        msgs[3]["content"],
        "can you tell me what this codebase does"
    );
}

#[test]
fn finish_at_end_of_conversation_synthesizes_tool_result() {
    let cfg = config();
    let mut req = request();
    req.messages = vec![
        Message::user("howdy"),
        Message::assistant("").with_tool_calls(vec![ToolCall {
            function: FunctionCall {
                name: "finish".into(),
                arguments: json!({"summary": "Done"}),
            },
        }]),
    ];
    let body = wire::request(&req, &cfg).unwrap();
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[2]["role"], "tool");
    assert_eq!(msgs[2]["content"], "Completed.");
    assert_eq!(msgs[2]["tool_call_id"], msgs[1]["tool_calls"][0]["id"]);
}
