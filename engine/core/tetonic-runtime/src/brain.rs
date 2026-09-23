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
            pathway: BrainPathway::Single {
                model: self.model.clone(),
            },
            finish_reason,
            cost,
        })
    }

    async fn perceive(
        &self,
        perception: tetonic_domain::Perception,
    ) -> Result<Option<tetonic_domain::WorldAction>, BrainError> {
        // If background urgency and no events occurred, avoid expensive model forward pass
        if perception.urgency == tetonic_domain::Urgency::Background && perception.events.is_empty() {
            return Ok(None);
        }

        let system_msg = Message::system(
            "You are an autonomous agent perceiving a live environment. If an action is required, output a JSON object with 'kind' and 'payload'. If no action is needed, return empty content.",
        );
        let perception_summary =
            serde_json::to_string(&perception).unwrap_or_else(|_| "{}".into());
        let user_msg = Message::user(format!("Current Perception:\n{perception_summary}"));

        let chat_req = ChatRequest {
            model: self.model.clone(),
            messages: vec![system_msg, user_msg],
            num_ctx: Some(self.num_ctx as u32),
            ..Default::default()
        };

        let mut noop = |_: &str| {};
        let resp = self
            .provider
            .chat(chat_req, &mut noop)
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
        *self.last_cost.lock().unwrap() = cost;

        let content = resp.message.content.trim();
        if content.is_empty() {
            return Ok(None);
        }

        // Attempt to parse JSON action from model output
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(kind) = val.get("kind").and_then(|k| k.as_str()) {
                let payload = val.get("payload").cloned().unwrap_or(serde_json::Value::Null);
                return Ok(Some(tetonic_domain::WorldAction::with_payload(
                    kind,
                    payload,
                    BrainPathway::Single {
                        model: self.model.clone(),
                    },
                )));
            }
        }

        // If not structured JSON, default to speech/log action
        Ok(Some(tetonic_domain::WorldAction::with_payload(
            "speech",
            serde_json::json!({ "text": content }),
            BrainPathway::Single {
                model: self.model.clone(),
            },
        )))
    }

    fn describe(&self) -> &str {
        &self.description
    }

    fn last_cost(&self) -> BrainCost {
        self.last_cost.lock().unwrap().clone()
    }
}

// ── ScriptedBrain ───────────────────────────────────────────────────────────

/// A deterministic, zero-LLM brain implementation.
///
/// Executes a pure function or heuristic on each perception.
/// Returns immediately with zero token cost. Ideal for testing,
/// heuristic controllers, and deterministic simulation actors.
pub struct ScriptedBrain {
    handler: Arc<dyn Fn(&tetonic_domain::Perception) -> Option<tetonic_domain::WorldAction> + Send + Sync>,
    description: String,
}

impl ScriptedBrain {
    pub fn new<F>(name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(&tetonic_domain::Perception) -> Option<tetonic_domain::WorldAction> + Send + Sync + 'static,
    {
        let name = name.into();
        Self {
            description: format!("scripted:{name}"),
            handler: Arc::new(handler),
        }
    }
}

#[async_trait]
impl Brain for ScriptedBrain {
    async fn complete(
        &self,
        _req: BrainRequest,
        _on_token: &mut BrainTokenSink<'_>,
    ) -> Result<BrainResponse, BrainError> {
        Ok(BrainResponse {
            content: "scripted response".into(),
            tool_calls: None,
            pathway: BrainPathway::Single {
                model: "scripted".into(),
            },
            finish_reason: BrainFinishReason::Stop,
            cost: BrainCost::default(),
        })
    }

    async fn perceive(
        &self,
        perception: tetonic_domain::Perception,
    ) -> Result<Option<tetonic_domain::WorldAction>, BrainError> {
        Ok((self.handler)(&perception))
    }

    fn describe(&self) -> &str {
        &self.description
    }

    fn last_cost(&self) -> BrainCost {
        BrainCost::default()
    }
}

// ── DualProcessBrain ────────────────────────────────────────────────────────

/// An advanced dual-process cognitive architecture (System 1 + System 2).
///
/// Composes two pluggable brains:
/// - `reflexive`: High-frequency, low-latency sensory processing (System 1).
/// - `deliberative`: Deep-reasoning, long-horizon synthesis (System 2).
///
/// Escalates to the deliberative brain when perception urgency meets or exceeds
/// `escalation_threshold` or when discrete events require reasoning.
pub struct DualProcessBrain {
    reflexive: Arc<dyn Brain>,
    deliberative: Arc<dyn Brain>,
    escalation_threshold: tetonic_domain::Urgency,
    description: String,
    last_cost: std::sync::Mutex<BrainCost>,
}

impl DualProcessBrain {
    pub fn new(
        reflexive: Arc<dyn Brain>,
        deliberative: Arc<dyn Brain>,
        escalation_threshold: tetonic_domain::Urgency,
    ) -> Self {
        let description = format!(
            "dual:{}+{}",
            reflexive.describe(),
            deliberative.describe()
        );
        Self {
            reflexive,
            deliberative,
            escalation_threshold,
            description,
            last_cost: std::sync::Mutex::new(BrainCost::default()),
        }
    }
}

#[async_trait]
impl Brain for DualProcessBrain {
    async fn complete(
        &self,
        req: BrainRequest,
        on_token: &mut BrainTokenSink<'_>,
    ) -> Result<BrainResponse, BrainError> {
        // Turn-based reasoning delegates directly to the deliberative system
        let resp = self.deliberative.complete(req, on_token).await?;
        *self.last_cost.lock().unwrap() = resp.cost.clone();
        Ok(resp)
    }

    async fn perceive(
        &self,
        perception: tetonic_domain::Perception,
    ) -> Result<Option<tetonic_domain::WorldAction>, BrainError> {
        let should_deliberate =
            perception.urgency >= self.escalation_threshold || !perception.events.is_empty();

        if should_deliberate {
            let action = self.deliberative.perceive(perception).await?;
            let mut total_cost = self.deliberative.last_cost();
            total_cost.add(&self.reflexive.last_cost());
            *self.last_cost.lock().unwrap() = total_cost;
            Ok(action)
        } else {
            let action = self.reflexive.perceive(perception).await?;
            *self.last_cost.lock().unwrap() = self.reflexive.last_cost();
            Ok(action)
        }
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
    use chrono::Utc;
    use tetonic_domain::{Perception, Urgency, WorldAction, WorldState};

    #[tokio::test]
    async fn test_scripted_brain_perceives_and_acts() {
        let brain = ScriptedBrain::new("patrol", |p| {
            if p.urgency >= Urgency::Medium {
                Some(WorldAction::bare("alert", BrainPathway::Single { model: "scripted".into() }))
            } else {
                None
            }
        });

        assert_eq!(brain.describe(), "scripted:patrol");

        let p_low = Perception {
            when: Utc::now(),
            sequence: 1,
            urgency: Urgency::Low,
            signals: vec![],
            events: vec![],
            state: WorldState {
                schema_id: "test".into(),
                data: serde_json::Value::Null,
            },
        };
        assert!(brain.perceive(p_low).await.unwrap().is_none());

        let p_med = Perception {
            when: Utc::now(),
            sequence: 2,
            urgency: Urgency::Medium,
            signals: vec![],
            events: vec![],
            state: WorldState {
                schema_id: "test".into(),
                data: serde_json::Value::Null,
            },
        };
        let action = brain.perceive(p_med).await.unwrap().expect("action emitted");
        assert_eq!(action.kind, "alert");
    }

    #[tokio::test]
    async fn test_dual_process_brain_routes_between_reflex_and_deliberation() {
        let reflex = Arc::new(ScriptedBrain::new("reflex", |_| {
            Some(WorldAction::bare("reflex_step", BrainPathway::Reflexive { model: "fast".into() }))
        }));

        let deliberative = Arc::new(ScriptedBrain::new("planner", |_| {
            Some(WorldAction::bare("deep_plan", BrainPathway::Deliberative { model: "slow".into() }))
        }));

        let dual = DualProcessBrain::new(reflex, deliberative, Urgency::High);

        // Low urgency routes to reflex
        let p_low = Perception {
            when: Utc::now(),
            sequence: 1,
            urgency: Urgency::Low,
            signals: vec![],
            events: vec![],
            state: WorldState {
                schema_id: "test".into(),
                data: serde_json::Value::Null,
            },
        };
        let act_low = dual.perceive(p_low).await.unwrap().unwrap();
        assert_eq!(act_low.kind, "reflex_step");

        // High urgency routes to deliberative planner
        let p_high = Perception {
            when: Utc::now(),
            sequence: 2,
            urgency: Urgency::High,
            signals: vec![],
            events: vec![],
            state: WorldState {
                schema_id: "test".into(),
                data: serde_json::Value::Null,
            },
        };
        let act_high = dual.perceive(p_high).await.unwrap().unwrap();
        assert_eq!(act_high.kind, "deep_plan");
    }
}
