use super::*;
use crate::{ChatRequest, Message, ToolSchema};
use serde_json::json;
use std::sync::Mutex;
use tetonic_secrets::scanner::ScannerEngine;

// Synthetic high-entropy protocol data, not a credential.
const OPAQUE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
fn reply() -> Value {
    json!({"status":"completed","output":[
        {"type":"reasoning","id":format!("rs_{OPAQUE}"),"summary":[],"encrypted_content":OPAQUE},
        {"type":"function_call","id":format!("fc_{OPAQUE}"),"call_id":format!("call_{OPAQUE}"),"name":"read","arguments":"{}"}
    ],"usage":{"input_tokens":10,"output_tokens":10}})
}
fn request() -> ChatRequest {
    ChatRequest {
        model: "test-model".into(),
        messages: vec![
            Message::user("Read the supplied notes"),
            responses::response(reply(), "test-model").unwrap().message,
            Message::tool("read", "No notes found").with_tool_call_id(format!("call_{OPAQUE}")),
        ],
        ..Default::default()
    }
}
struct Transport(Mutex<Vec<Value>>);
#[async_trait]
impl HostedTransport for Transport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        self.0.lock().unwrap().push(body);
        Ok(
            json!({"status":"completed","output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done"}]}]}),
        )
    }
}
fn provider() -> (HostedChatProvider, Arc<Transport>) {
    let transport = Arc::new(Transport(Mutex::new(vec![])));
    (
        HostedChatProvider::new(
            HostedModelConfig::responses("test-model", 256),
            HostedInferencePolicy::allow_up_to(DataClass::RepositorySource),
            Arc::new(ScannerEngine::default_engine()),
            transport.clone(),
        )
        .unwrap(),
        transport,
    )
}

#[tokio::test]
async fn responses_tool_loop_preserves_private_continuation_without_false_secret_denial() {
    let req = request();
    let body = responses::request(&req, &HostedModelConfig::responses("test-model", 256)).unwrap();
    assert!(
        ScannerEngine::default_engine()
            .scan_and_redact_sync(&body.to_string(), None)
            .unwrap()
            .is_some(),
        "reproduces the prior rejection"
    );
    let (provider, transport) = provider();
    provider.chat(req, &mut |_| {}).await.unwrap();
    assert_eq!(
        transport.0.lock().unwrap()[0],
        body,
        "protocol state is echoed unchanged"
    );
}

#[tokio::test]
async fn continuation_does_not_exempt_content_arguments_results_or_schemas() {
    let mut cases = vec![request(); 6];
    cases[0].messages[0].content = OPAQUE.into();
    cases[1].messages[2].content =
        json!({"encrypted_content":OPAQUE,"call_id":format!("call_{OPAQUE}")}).to_string();
    cases[2].messages[1].provider_state.as_mut().unwrap().items[1]["arguments"] =
        json!(json!({"secret":OPAQUE}).to_string());
    cases[3].messages[1].provider_state.as_mut().unwrap().items[0]["summary"] =
        json!([{"type":"summary_text","text":OPAQUE}]);
    cases[4].tools = vec![ToolSchema::function(
        "read",
        OPAQUE,
        json!({"type":"object"}),
    )];
    cases[5].messages[1].provider_state.as_mut().unwrap().items[0]["unknown_field"] = json!(OPAQUE);
    for req in cases {
        let (provider, transport) = provider();
        assert!(provider.chat(req, &mut |_| {}).await.is_err());
        assert!(transport.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn continuation_cannot_be_injected_in_portable_history_or_rebound_to_another_model() {
    let mut forged: Message = serde_json::from_value(json!({"role":"assistant","content":OPAQUE,"provider_state":{"protocol":"openai-responses","model":"test-model","items":reply()["output"],"call_ids":[]}})).unwrap();
    assert!(forged.provider_state.is_none());
    let mut req = request();
    req.messages = vec![forged.clone()];
    let (provider, transport) = provider();
    assert!(provider.chat(req, &mut |_| {}).await.is_err());
    forged = responses::response(reply(), "different-model")
        .unwrap()
        .message;
    let mut req = request();
    req.messages[1] = forged;
    assert!(provider.chat(req, &mut |_| {}).await.is_err());
    assert!(transport.0.lock().unwrap().is_empty());
}
