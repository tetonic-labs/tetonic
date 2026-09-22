//! OpenAI-compatible Chat Completions (/v1/chat/completions) wire adapter.
use serde_json::{json, Value};
use std::collections::VecDeque;

use super::{error, HostedModelConfig, OutputLimitField};
use crate::{
    ChatRequest, ChatResponse, FunctionCall, GenUsage, InferenceError, InferenceProvenance,
    Message, ToolCall,
};

pub fn request(req: &ChatRequest, config: &HostedModelConfig) -> Result<Value, InferenceError> {
    if !config.is_model_allowed(&req.model) {
        return Err(error("request model does not match hosted binding"));
    }
    if req.model_digest.is_some() || req.draft_model.is_some() || req.draft_count.is_some() {
        return Err(error(
            "hosted adapter does not support model digests or speculative draft settings",
        ));
    }
    if !config.supports_tools
        && (!req.tools.is_empty()
            || req
                .messages
                .iter()
                .any(|m| m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())))
    {
        return Err(error("hosted model profile does not support tools"));
    }
    if req.response_format.is_some() && !config.supports_json_schema {
        return Err(error(
            "hosted model profile does not support JSON schema output",
        ));
    }
    let mut messages = Vec::new();
    let mut pending = VecDeque::new();
    for (index, msg) in req.messages.iter().enumerate() {
        if !matches!(msg.role.as_str(), "system" | "user" | "assistant" | "tool") {
            return Err(error("unsupported hosted message role"));
        }
        if msg.role != "tool" && !pending.is_empty() {
            while let Some((id, _name)) = pending.pop_front() {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": "Completed.",
                }));
            }
        }
        let mut value = json!({"role": msg.role, "content": msg.content});
        if msg.role == "tool" {
            let (id, name) = pending
                .pop_front()
                .ok_or_else(|| error("orphan tool result in hosted conversation"))?;
            if msg.tool_name.as_deref().is_some() && msg.tool_name.as_deref() != Some(name) {
                return Err(error("out-of-order tool result in hosted conversation"));
            }
            value["tool_call_id"] = Value::String(id);
        }
        if let Some(calls) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
            if msg.role != "assistant" {
                return Err(error("tool calls require assistant role"));
            }
            let mut wire_calls = Vec::new();
            for (ordinal, call) in calls.iter().enumerate() {
                // Core owns ordered calls, not vendor IDs. Reconstruct a consistent
                // pair of assistant/tool IDs for each complete outbound history.
                let id = format!("call_{index}_{ordinal}");
                pending.push_back((id.clone(), call.function.name.as_str()));
                let args = arguments(&call.function.arguments)?;
                wire_calls.push(json!({"id": id, "type": "function", "function": {
                    "name": call.function.name, "arguments": args.to_string()
                }}));
            }
            value["tool_calls"] = Value::Array(wire_calls);
        }
        messages.push(value);
    }
    while let Some((id, name)) = pending.pop_front() {
        if name == "finish" {
            messages.push(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": "Completed.",
            }));
        } else {
            return Err(error("missing tool results in hosted conversation"));
        }
    }
    let is_reasoning = is_openai_reasoning_model(&req.model);
    let mut body = json!({"model": req.model, "messages": messages, "stream": false});
    let field = if is_reasoning {
        "max_completion_tokens"
    } else {
        match config.output_limit_field {
            OutputLimitField::MaxTokens => "max_tokens",
            OutputLimitField::MaxCompletionTokens => "max_completion_tokens",
        }
    };
    body[field] = json!(config.max_output_tokens);
    if config.send_temperature && !is_reasoning {
        body["temperature"] = json!(req.temperature);
    }
    if !req.tools.is_empty() {
        body["tools"] =
            serde_json::to_value(&req.tools).map_err(|_| error("invalid tool schema"))?;
    }
    if let Some(schema) = &req.response_format {
        body["response_format"] = json!({"type": "json_schema", "json_schema": {"name": "lokai_response", "schema": schema}});
    }
    Ok(body)
}

fn arguments(value: &Value) -> Result<Value, InferenceError> {
    let value = if let Some(text) = value.as_str() {
        serde_json::from_str(text).map_err(|_| error("invalid hosted tool arguments"))?
    } else {
        value.clone()
    };
    if !value.is_object() {
        return Err(error("hosted tool arguments must be an object"));
    }
    Ok(value)
}

pub fn response(value: Value, model: &str) -> Result<ChatResponse, InferenceError> {
    if value.get("error").is_some() {
        return Err(error("hosted provider returned an error"));
    }
    let choices = value["choices"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or_else(|| error("hosted response requires exactly one choice"))?;
    let choice = &choices[0];
    let finish = choice["finish_reason"]
        .as_str()
        .ok_or_else(|| error("missing hosted completion status"))?;
    if !matches!(finish, "stop" | "tool_calls") {
        return Err(error(
            "hosted completion truncated, refused, or unsupported",
        ));
    }
    let msg = &choice["message"];
    if msg["role"] != "assistant" || !msg["refusal"].is_null() {
        return Err(error("invalid or refused hosted assistant response"));
    }
    let content = match &msg["content"] {
        Value::Null => "",
        Value::String(s) => s,
        _ => return Err(error("unsupported hosted content type")),
    };
    let mut message = Message::assistant(content);
    if let Some(calls) = msg.get("tool_calls").filter(|c| !c.is_null()) {
        let calls = calls
            .as_array()
            .ok_or_else(|| error("invalid hosted tool calls"))?;
        let mut result = Vec::new();
        for call in calls {
            if call["type"] != "function" {
                return Err(error("unsupported hosted tool type"));
            }
            let name = call["function"]["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| error("missing hosted tool name"))?;
            result.push(ToolCall {
                function: FunctionCall {
                    name: name.into(),
                    arguments: arguments(&call["function"]["arguments"])?,
                },
            });
        }
        if !result.is_empty() {
            message.tool_calls = Some(result);
        }
    }
    if (finish == "tool_calls") != message.tool_calls.is_some() {
        return Err(error("inconsistent hosted completion status"));
    }
    if message.content.is_empty() && message.tool_calls.is_none() {
        return Err(error("empty hosted completion"));
    }
    Ok(ChatResponse {
        message,
        usage: GenUsage {
            prompt_tokens: value["usage"]["prompt_tokens"].as_u64(),
            eval_tokens: value["usage"]["completion_tokens"].as_u64(),
            ..Default::default()
        },
        provenance: InferenceProvenance {
            provider_kind: "hosted_chat_completions".into(),
            model: model.into(),
            ..Default::default()
        },
    })
}

fn is_openai_reasoning_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.starts_with("o1") || lower.starts_with("o3")
}
