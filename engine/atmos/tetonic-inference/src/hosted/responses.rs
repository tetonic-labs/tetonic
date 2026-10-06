//! OpenAI Responses with explicit history, private continuation and client tools.
use super::{error, HostedModelConfig};
use crate::{
    ChatRequest, ChatResponse, FunctionCall, GenUsage, InferenceError, InferenceProvenance,
    Message, ProviderMessageState, ToolCall,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

const PROTOCOL: &str = "openai-responses";

pub fn request(req: &ChatRequest, config: &HostedModelConfig) -> Result<Value, InferenceError> {
    if !config.is_model_allowed(&req.model)
        || req.model_digest.is_some()
        || req.draft_model.is_some()
        || req.draft_count.is_some()
    {
        return Err(error("request does not match the Responses model binding"));
    }
    if (!req.tools.is_empty() && !config.supports_tools)
        || (req.response_format.is_some() && !config.supports_json_schema)
    {
        return Err(error(
            "Responses profile does not support the requested tools or output format",
        ));
    }
    let mut input = Vec::new();
    let mut pending = BTreeMap::<String, String>::new();
    let mut seen = HashSet::new();
    for (index, message) in req.messages.iter().enumerate() {
        if message.role == "tool" {
            let id = message
                .tool_call_id
                .as_deref()
                .ok_or_else(|| error("Responses tool result is missing its call ID"))?;
            let name = pending
                .remove(id)
                .ok_or_else(|| error("Responses tool result does not match a pending call"))?;
            if message
                .tool_name
                .as_deref()
                .is_some_and(|value| value != name)
            {
                return Err(error("Responses tool result name does not match its call"));
            }
            input
                .push(json!({"type":"function_call_output","call_id":id,"output":message.content}));
            continue;
        }
        close_completion(&mut pending, &mut input)?;
        if let Some(state) = &message.provider_state {
            if message.role != "assistant" || state.protocol != PROTOCOL || state.model != req.model
            {
                return Err(error(
                    "provider continuation belongs to another model or protocol",
                ));
            }
            if let Some(calls) = &message.tool_calls {
                if calls.len() != state.call_ids.len() {
                    return Err(error("invalid Responses call correlation"));
                }
                for (call, id) in calls.iter().zip(&state.call_ids) {
                    if !seen.insert(id.clone()) {
                        return Err(error("duplicate Responses tool call ID"));
                    }
                    pending.insert(id.clone(), call.function.name.clone());
                }
            }
            input.extend(state.items.clone());
            continue;
        }
        if !matches!(message.role.as_str(), "system" | "user" | "assistant") {
            return Err(error("unsupported Responses message role"));
        }
        if !message.content.is_empty() {
            input.push(json!({"role": if message.role == "system" { "developer" } else { &message.role }, "content":message.content}));
        }
        if let Some(calls) = &message.tool_calls {
            if message.role != "assistant" {
                return Err(error("tool calls require an assistant message"));
            }
            for (ordinal, call) in calls.iter().enumerate() {
                let id = format!("call_{index}_{ordinal}");
                if !seen.insert(id.clone()) {
                    return Err(error("duplicate Responses tool call ID"));
                }
                pending.insert(id.clone(), call.function.name.clone());
                input.push(json!({"type":"function_call","call_id":id,"name":call.function.name,"arguments":arguments(&call.function.arguments)?.to_string()}));
            }
        }
    }
    close_completion(&mut pending, &mut input)?;
    let mut body = json!({"model":req.model,"input":input,"stream":true,"store":false,
        "include":["reasoning.encrypted_content"],
        "max_output_tokens":req.max_tokens.unwrap_or(config.max_output_tokens).min(config.max_output_tokens)});
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|tool| json!({
            "type":"function","name":tool.function.name,"description":tool.function.description,
            "parameters":tool.function.parameters,"strict":false
        })).collect());
    }
    if let Some(schema) = &req.response_format {
        body["text"] = json!({"format":{"type":"json_schema","name":"tetonic_response","schema":schema,"strict":true}});
    }
    Ok(body)
}

fn close_completion(
    pending: &mut BTreeMap<String, String>,
    input: &mut Vec<Value>,
) -> Result<(), InferenceError> {
    // Only the internal terminal marker has no ordinary tool-result message.
    // Missing external effects must never be fabricated as successful results.
    if pending.values().any(|name| name != "finish") {
        return Err(error("missing Responses tool results"));
    }
    for (id, _) in std::mem::take(pending) {
        input.push(json!({"type":"function_call_output","call_id":id,"output":"Completed."}));
    }
    Ok(())
}

fn arguments(value: &Value) -> Result<Value, InferenceError> {
    let parsed = if let Some(text) = value.as_str() {
        serde_json::from_str(text).map_err(|_| error("invalid Responses tool arguments"))?
    } else {
        value.clone()
    };
    if !parsed.is_object() {
        return Err(error("Responses tool arguments must be an object"));
    }
    Ok(parsed)
}

pub fn response(value: Value, model: &str) -> Result<ChatResponse, InferenceError> {
    if value["status"] != "completed" || !value["error"].is_null() {
        return Err(error(
            "Responses inference failed, was refused or did not complete",
        ));
    }
    let output = value["output"]
        .as_array()
        .ok_or_else(|| error("missing Responses output"))?;
    let mut message = Message::assistant("");
    let mut calls = Vec::new();
    let mut call_ids = Vec::new();
    let mut seen = HashSet::new();
    for item in output {
        match item["type"].as_str() {
            Some("message") => {
                if item["role"] != "assistant" || item["status"] != "completed" {
                    return Err(error("incomplete Responses message"));
                }
                for block in item["content"]
                    .as_array()
                    .ok_or_else(|| error("invalid Responses message content"))?
                {
                    if block["type"] != "output_text" {
                        return Err(error("refused or unsupported Responses content"));
                    }
                    message.content.push_str(
                        block["text"]
                            .as_str()
                            .ok_or_else(|| error("invalid Responses text"))?,
                    );
                }
            }
            Some("function_call") => {
                let id = item["call_id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 512)
                    .ok_or_else(|| error("missing Responses tool call ID"))?;
                if !seen.insert(id.to_owned()) {
                    return Err(error("duplicate Responses tool call ID"));
                }
                let name = item["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| error("missing Responses tool name"))?;
                calls.push(ToolCall {
                    function: FunctionCall {
                        name: name.into(),
                        arguments: arguments(&item["arguments"])?,
                    },
                });
                call_ids.push(id.into());
            }
            Some("reasoning") => {} // Preserve opaque continuation; never emit it as readable thought.
            _ => return Err(error("unsupported Responses output item")),
        }
    }
    if message.content.is_empty() && calls.is_empty() {
        return Err(error("empty Responses completion"));
    }
    if !calls.is_empty() {
        message.tool_calls = Some(calls);
    }
    message.provider_state = Some(ProviderMessageState {
        protocol: PROTOCOL,
        model: model.into(),
        items: output.clone(),
        call_ids,
    });
    Ok(ChatResponse {
        message,
        usage: GenUsage {
            prompt_tokens: value["usage"]["input_tokens"].as_u64(),
            eval_tokens: value["usage"]["output_tokens"].as_u64(),
            finish_reason: Some("completed".into()),
            ..Default::default()
        },
        provenance: InferenceProvenance {
            provider_kind: "hosted_responses".into(),
            model: value["model"].as_str().unwrap_or(model).into(),
            ..Default::default()
        },
    })
}

#[cfg(test)]
mod tests;
