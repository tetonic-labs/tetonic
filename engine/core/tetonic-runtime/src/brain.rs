//! Concrete Brain implementations assembled at the runtime layer.
//!
//! These live here — not in `tetonic-domain` or `tetonic-core` — because they
//! compose `InferenceProvider` handles from `tetonic-inference` (atmos layer).
//! The `Brain` trait itself lives in `tetonic-domain` and carries no inference dep.

use std::sync::Arc;

use async_trait::async_trait;
use tetonic_domain::{
    Brain, BrainCost, BrainError, BrainFinishReason, BrainMessage, BrainPathway, BrainRequest,
    BrainResponse, BrainRole, BrainTokenSink,
};
use tetonic_inference::{ChatRequest, InferenceProvider, Message, ToolSchema};

// Continuous brains bypass the turn loop, so they must perform its outbound
// redaction step themselves before calling a real inference provider.
fn scan_request(req: &mut ChatRequest) -> Result<(), BrainError> {
    let input = serde_json::json!({"messages": req.messages, "tools": req.tools});
    let (clean, found) =
        tetonic_secrets::redact_json_value(tetonic_secrets::shared_scanner(), &input).map_err(
            |detail| BrainError::Inference {
                pathway: "outbound_scan".into(),
                detail,
            },
        )?;
    req.messages =
        serde_json::from_value(clean["messages"].clone()).map_err(|e| BrainError::Inference {
            pathway: "outbound_scan".into(),
            detail: e.to_string(),
        })?;
    req.tools =
        serde_json::from_value(clean["tools"].clone()).map_err(|e| BrainError::Inference {
            pathway: "outbound_scan".into(),
            detail: e.to_string(),
        })?;
    req.outbound_scan = tetonic_inference::OutboundScan::from_scan(found);
    Ok(())
}

// ── SingleModelBrain ─────────────────────────────────────────────────────────

/// The default brain: exactly one model, one call, same behaviour as the
/// pre-Brain agent loop. Every existing agent gets this by default; there is
/// no behaviour change until you opt into a different brain architecture.
pub type InferenceObserver = Arc<dyn Fn(&str, &str, serde_json::Value) + Send + Sync>;

pub struct SingleModelBrain {
    provider: Arc<dyn InferenceProvider>,
    model: String,
    num_ctx: usize,
    description: String,
    last_cost: std::sync::Mutex<BrainCost>,
    observer: Option<InferenceObserver>,
}

impl SingleModelBrain {
    pub fn new(
        provider: Arc<dyn InferenceProvider>,
        model: impl Into<String>,
        num_ctx: usize,
    ) -> Self {
        let model = model.into();
        let description = format!("single:{model}");
        Self {
            provider,
            model,
            num_ctx,
            description,
            last_cost: std::sync::Mutex::new(BrainCost::default()),
            observer: None,
        }
    }
    pub fn with_observer(mut self, observer: InferenceObserver) -> Self {
        self.observer = Some(observer);
        self
    }
}

#[async_trait]
impl Brain for SingleModelBrain {
    async fn complete(
        &self,
        req: BrainRequest,
        on_token: &mut BrainTokenSink<'_>,
    ) -> Result<BrainResponse, BrainError> {
        let trace_id = req.trace_label.clone();
        let messages = brain_messages_to_inference(req.messages);
        let tools: Vec<ToolSchema> = req
            .tools
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        let mut chat_req = ChatRequest {
            model: self.model.clone(),
            messages,
            tools,
            num_ctx: Some(self.num_ctx as u32),
            max_tokens: req.max_tokens,
            ..Default::default()
        };

        scan_request(&mut chat_req)?;
        let emit = |stage: &str, data: serde_json::Value| {
            if let Some(observer) = &self.observer {
                observer(&trace_id, stage, data);
            }
        };
        emit(
            "inference_request",
            serde_json::json!({"model":chat_req.model,"messages":chat_req.messages,"tools":chat_req.tools,"num_ctx":chat_req.num_ctx,"max_tokens":chat_req.max_tokens,"temperature":chat_req.temperature,"boundary":"post-redaction request passed to inference provider"}),
        );
        let started = std::time::Instant::now();
        let mut stream = |chunk: &str| {
            emit("output_delta", serde_json::json!({"text":chunk}));
            on_token(chunk);
        };
        let resp = match self.provider.chat(chat_req, &mut stream).await {
            Ok(response) => response,
            Err(error) => {
                emit(
                    "inference_error",
                    serde_json::json!({"error":error.to_string(),"elapsed_ms":started.elapsed().as_millis()}),
                );
                return Err(BrainError::Inference {
                    pathway: self.model.clone(),
                    detail: error.to_string(),
                });
            }
        };
        emit(
            "inference_response",
            serde_json::json!({"message":resp.message,"finish_reason":resp.usage.finish_reason,"elapsed_ms":started.elapsed().as_millis(),"input_tokens":resp.usage.prompt_tokens,"output_tokens":resp.usage.eval_tokens,"prompt_eval_ms":resp.usage.prompt_eval_ms,"eval_ms":resp.usage.eval_ms}),
        );

        let cost = BrainCost {
            input_tokens: resp.usage.prompt_tokens.unwrap_or(0),
            output_tokens: resp.usage.eval_tokens.unwrap_or(0),
            model_calls: 1,
        };
        *self.last_cost.lock().unwrap() = cost.clone();

        let has_tool_calls = resp
            .message
            .tool_calls
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let finish_reason = if resp.usage.finish_reason.as_deref() == Some("length") {
            BrainFinishReason::Length
        } else if has_tool_calls {
            BrainFinishReason::ToolUse
        } else {
            BrainFinishReason::Stop
        };

        let tool_calls_json = resp
            .message
            .tool_calls
            .map(|tc| serde_json::to_value(tc).unwrap_or(serde_json::Value::Null));

        Ok(BrainResponse {
            content: resp.message.content,
            tool_calls: tool_calls_json,
            pathway: BrainPathway::Single {
                model: self.model.clone(),
            },
            finish_reason,
            cost,
        })
    }

    fn describe(&self) -> &str {
        &self.description
    }

    fn last_cost(&self) -> BrainCost {
        self.last_cost.lock().unwrap().clone()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert the domain-level brain message format into the inference layer's
/// `Message` type. Role is a plain string in the inference layer.
fn brain_messages_to_inference(msgs: Vec<BrainMessage>) -> Vec<Message> {
    msgs.into_iter()
        .map(|m| {
            let mut msg = match m.role {
                BrainRole::System => Message::system(m.content),
                BrainRole::User => Message::user(m.content),
                BrainRole::Assistant => Message::assistant(m.content),
                BrainRole::Tool => {
                    let name = m.tool_call_id.clone().unwrap_or_default();
                    Message::tool(name, m.content)
                }
            };
            if let Some(id) = m.tool_call_id {
                msg = msg.with_tool_call_id(id);
            }
            msg
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Public AWS documentation example, not a live credential.
    const EXAMPLE_SECRET: &str = "AKIAIOSFODNN7EXAMPLE";

    #[derive(Default)]
    struct RecordingProvider(std::sync::Mutex<Vec<ChatRequest>>);

    #[async_trait]
    impl InferenceProvider for RecordingProvider {
        async fn chat(
            &self,
            req: ChatRequest,
            _sink: &mut tetonic_inference::TokenSink<'_>,
        ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
            self.0.lock().unwrap().push(req);
            Ok(tetonic_inference::ChatResponse {
                message: Message::assistant(""),
                usage: Default::default(),
                provenance: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn completion_redacts_messages_and_tools_before_provider_and_observer() {
        let provider = Arc::new(RecordingProvider::default());
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = observed.clone();
        let brain = SingleModelBrain::new(provider.clone(), "test", 4096).with_observer(Arc::new(
            move |_, stage, data| {
                recorded.lock().unwrap().push((stage.to_string(), data));
            },
        ));
        let req = BrainRequest {
            messages: vec![BrainMessage {
                role: BrainRole::User,
                content: format!("Inspect this example key: {EXAMPLE_SECRET}"),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "inspect",
                    "description": format!("Example: {EXAMPLE_SECRET}"),
                    "parameters": {"type": "object", "properties": {
                        "key": {"type": "string", "description": EXAMPLE_SECRET}
                    }}
                }
            })],
            max_tokens: Some(256),
            trace_label: "redaction-test".into(),
        };
        brain.complete(req, &mut |_| {}).await.unwrap();

        let requests = provider.0.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let sent = &requests[0];
        assert!(sent.outbound_scan.is_scanned());
        assert!(sent.outbound_scan.blocks_remote());
        assert_eq!(
            sent.messages[0].content,
            "Inspect this example key: [REDACTED:aws-access-key]"
        );
        assert_eq!(
            sent.tools[0].function.description,
            "Example: [REDACTED:aws-access-key]"
        );
        assert_eq!(
            sent.tools[0].function.parameters["properties"]["key"]["description"],
            "[REDACTED:aws-access-key]"
        );
        let observed = observed.lock().unwrap();
        let request = observed
            .iter()
            .find(|(stage, _)| stage == "inference_request")
            .unwrap();
        assert_eq!(
            request.1["messages"],
            serde_json::to_value(&sent.messages).unwrap()
        );
        assert_eq!(
            request.1["tools"],
            serde_json::to_value(&sent.tools).unwrap()
        );
        assert!(!serde_json::to_string(&*observed)
            .unwrap()
            .contains(EXAMPLE_SECRET));
    }

    struct TraceProvider;
    #[async_trait]
    impl InferenceProvider for TraceProvider {
        async fn chat(
            &self,
            req: ChatRequest,
            sink: &mut tetonic_inference::TokenSink<'_>,
        ) -> Result<tetonic_inference::ChatResponse, tetonic_inference::InferenceError> {
            assert_eq!(req.messages[0].content, "local observation");
            assert_eq!(req.max_tokens, Some(256));
            sink("not ");
            sink("valid JSON");
            Ok(tetonic_inference::ChatResponse {
                message: Message::assistant("not valid JSON"),
                usage: tetonic_inference::GenUsage {
                    finish_reason: Some("length".into()),
                    ..Default::default()
                },
                provenance: Default::default(),
            })
        }
    }
    #[tokio::test]
    async fn observer_keeps_unparsed_response_and_real_chunks_in_order() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = events.clone();
        let brain = SingleModelBrain::new(Arc::new(TraceProvider), "test", 4096).with_observer(
            Arc::new(move |id, stage, data| {
                recorded
                    .lock()
                    .unwrap()
                    .push((id.to_string(), stage.to_string(), data))
            }),
        );
        let req = BrainRequest {
            messages: vec![BrainMessage {
                role: BrainRole::User,
                content: "local observation".into(),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: vec![],
            max_tokens: Some(256),
            trace_label: "decision-1".into(),
        };
        let mut chunks = String::new();
        let mut sink = |s: &str| chunks.push_str(s);
        let response = brain.complete(req, &mut sink).await.unwrap();
        assert_eq!(response.finish_reason, BrainFinishReason::Length);
        assert_eq!(response.content, "not valid JSON");
        assert_eq!(chunks, response.content);
        let events = events.lock().unwrap();
        assert_eq!(
            events.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(),
            vec![
                "inference_request",
                "output_delta",
                "output_delta",
                "inference_response"
            ]
        );
        assert!(events.iter().all(|e| e.0 == "decision-1"));
        assert_eq!(events[0].2["messages"][0]["content"], "local observation");
        assert_eq!(events[3].2["message"]["content"], "not valid JSON");
    }
}
