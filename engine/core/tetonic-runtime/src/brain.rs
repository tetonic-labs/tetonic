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

// ── SingleModelBrain ─────────────────────────────────────────────────────────

/// The default brain: exactly one model, one call, same behaviour as the
/// pre-Brain agent loop. Every existing agent gets this by default; there is
/// no behaviour change until you opt into a different brain architecture.
pub struct SingleModelBrain {
    provider: Arc<dyn InferenceProvider>,
    model: String,
    num_ctx: usize,
    description: String,
    last_cost: std::sync::Mutex<BrainCost>,
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
        }
    }
}

#[async_trait]
impl Brain for SingleModelBrain {
    async fn complete(
        &self,
        req: BrainRequest,
        on_token: &mut BrainTokenSink<'_>,
    ) -> Result<BrainResponse, BrainError> {
        let messages = brain_messages_to_inference(req.messages);
        let tools: Vec<ToolSchema> = req
            .tools
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        let chat_req = ChatRequest {
            model: self.model.clone(),
            messages,
            tools,
            num_ctx: Some(self.num_ctx as u32),
            ..Default::default()
        };

        let resp = self
            .provider
            .chat(chat_req, on_token)
            .await
            .map_err(|e| BrainError::Inference {
                pathway: self.model.clone(),
                detail: e.to_string(),
            })?;

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
        let finish_reason = if has_tool_calls {
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
            pathway: BrainPathway::Single { model: self.model.clone() },
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
                    // Tool messages require a name; use the tool_call_id as a fallback name.
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
