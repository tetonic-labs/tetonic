//! Gemini generateContent: client-executed functions, private continuation.
//! Native search/code-execution tools are deliberately not enabled by this adapter.
use super::{error, HostedModelConfig};
use crate::{
    ChatRequest, ChatResponse, FunctionCall, GenUsage, InferenceError, InferenceProvenance,
    Message, ProviderMessageState, ToolCall,
};
use serde_json::{json, Value};

struct Pending {
    id: String,
    wire_id: Option<String>,
    name: String,
}

fn append(contents: &mut Vec<Value>, role: &str, parts: Vec<Value>) {
    if let Some(last) = contents.last_mut().filter(|m| m["role"] == role) {
        last["parts"].as_array_mut().unwrap().extend(parts);
    } else {
        contents.push(json!({"role":role,"parts":parts}));
    }
}

fn result(call: &Pending, output: &str) -> Value {
    let mut value = json!({"name":call.name,"response":{"output":output}});
    if let Some(id) = &call.wire_id {
        value["id"] = json!(id);
    }
    json!({"functionResponse":value})
}

fn finish_pending(
    pending: &mut Vec<Pending>,
    contents: &mut Vec<Value>,
) -> Result<(), InferenceError> {
    // Only the runtime's terminal pseudo-tool can synthesize a result.
    if pending.iter().any(|c| c.name != "finish") {
        return Err(error("missing Gemini tool results"));
    }
    for call in pending.drain(..) {
        append(contents, "user", vec![result(&call, "Completed.")]);
    }
    Ok(())
}

pub fn request(req: &ChatRequest, config: &HostedModelConfig) -> Result<Value, InferenceError> {
    // generateContent binds its model in the transport URL, not the body.
    if req.model != config.model {
        return Err(error("request model does not match hosted binding"));
    }
    if req.model_digest.is_some()
        || req.draft_model.is_some()
        || req.draft_count.is_some()
        || req.response_format.is_some()
    {
        return Err(error(
            "Gemini profile does not support speculative settings or structured output",
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
    let mut contents = Vec::new();
    let mut system = Vec::new();
    let mut pending: Vec<Pending> = Vec::new();
    for msg in &req.messages {
        if msg.provider_state.as_ref().is_some_and(|s| {
            msg.role != "assistant"
                || s.protocol != "google-generate-content"
                || s.model != req.model
        }) {
            return Err(error("provider continuation cannot be converted to Gemini"));
        }
        match msg.role.as_str() {
            "system" => {
                if !msg.content.is_empty() {
                    system.push(json!({"text":msg.content}));
                }
            }
            "user" => {
                finish_pending(&mut pending, &mut contents)?;
                append(&mut contents, "user", vec![json!({"text":msg.content})]);
            }
            "assistant" => {
                if !pending.is_empty() {
                    return Err(error(
                        "missing Gemini tool results before assistant message",
                    ));
                }
                let parts = if let Some(state) = &msg.provider_state {
                    let calls = msg.tool_calls.as_deref().unwrap_or_default();
                    let native: Vec<_> = state
                        .items
                        .iter()
                        .filter_map(|p| p.get("functionCall"))
                        .collect();
                    if state.call_ids.len() != calls.len() || native.len() != calls.len() {
                        return Err(error("invalid Gemini call correlation"));
                    }
                    for (index, call) in calls.iter().enumerate() {
                        pending.push(Pending {
                            id: state.call_ids[index].clone(),
                            wire_id: native[index]["id"].as_str().map(str::to_owned),
                            name: call.function.name.clone(),
                        });
                    }
                    state.items.clone()
                } else {
                    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
                        return Err(error("Gemini tool continuation is missing"));
                    }
                    vec![json!({"text":msg.content})]
                };
                append(&mut contents, "model", parts);
            }
            "tool" => {
                let id = msg
                    .tool_call_id
                    .as_deref()
                    .ok_or_else(|| error("Gemini result requires a call ID"))?;
                let index = pending
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or_else(|| error("Gemini result does not match a pending call"))?;
                let call = pending.remove(index);
                if msg.tool_name.as_ref().is_some_and(|n| n != &call.name) {
                    return Err(error("Gemini result name does not match call"));
                }
                append(&mut contents, "user", vec![result(&call, &msg.content)]);
            }
            _ => return Err(error("unsupported Gemini message role")),
        }
    }
    finish_pending(&mut pending, &mut contents)?;
    let mut body = json!({"contents":contents,"generationConfig":{"maxOutputTokens":req.max_tokens.unwrap_or(config.max_output_tokens).min(config.max_output_tokens),"candidateCount":1}});
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts":system});
    }
    if config.send_temperature {
        body["generationConfig"]["temperature"] = json!(req.temperature);
    }
    if !req.tools.is_empty() {
        body["tools"] = json!([{"functionDeclarations":req.tools.iter().map(|t| json!({"name":t.function.name,"description":t.function.description,"parametersJsonSchema":t.function.parameters})).collect::<Vec<_>>()}]);
    }
    Ok(body)
}

pub fn response(value: &Value, model: &str) -> Result<ChatResponse, InferenceError> {
    if value.get("error").is_some() || value["promptFeedback"].get("blockReason").is_some() {
        return Err(error("Gemini rejected or blocked the request"));
    }
    let candidates = value["candidates"]
        .as_array()
        .filter(|c| c.len() == 1)
        .ok_or_else(|| error("missing or ambiguous Gemini candidate"))?;
    let candidate = &candidates[0];
    if candidate["finishReason"] != "STOP" || candidate["content"]["role"] != "model" {
        return Err(error("Gemini completion was incomplete or unsupported"));
    }
    let parts = candidate["content"]["parts"]
        .as_array()
        .ok_or_else(|| error("missing Gemini content"))?;
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Reserve actual IDs before assigning internal IDs for older models that omit them.
    for part in parts {
        if let Some(id) = part["functionCall"].get("id") {
            let id = id
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512)
                .ok_or_else(|| error("invalid Gemini call ID"))?;
            if !seen.insert(id.to_owned()) {
                return Err(error("duplicate Gemini call ID"));
            }
        }
    }
    for (index, part) in parts.iter().enumerate() {
        let object = part
            .as_object()
            .ok_or_else(|| error("invalid Gemini part"))?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "text" | "functionCall" | "thought" | "thoughtSignature" | "partMetadata"
            )
        }) || part.get("thought").is_some_and(|v| !v.is_boolean())
            || part.get("thoughtSignature").is_some_and(|v| !v.is_string())
        {
            return Err(error("unsupported Gemini content part"));
        }
        if let Some(t) = part.get("text") {
            if part.get("functionCall").is_some() {
                return Err(error("ambiguous Gemini part"));
            }
            let t = t.as_str().ok_or_else(|| error("invalid Gemini text"))?;
            if part["thought"] != true {
                text.push_str(t);
            }
        } else if let Some(call) = part.get("functionCall") {
            if part["thought"] == true {
                return Err(error("Gemini thought cannot authorize a tool call"));
            }
            let name = call["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .ok_or_else(|| error("invalid Gemini function name"))?;
            let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            if !args.is_object() {
                return Err(error("invalid Gemini function arguments"));
            }
            let id = if let Some(id) = call["id"].as_str() {
                id.to_owned()
            } else {
                let mut id = format!("gemini-internal-{index}");
                while !seen.insert(id.clone()) {
                    id.push('_');
                }
                id
            };
            ids.push(id);
            calls.push(ToolCall {
                function: FunctionCall {
                    name: name.into(),
                    arguments: args,
                },
            });
        } else {
            return Err(error("unsupported Gemini content part"));
        }
    }
    if text.is_empty() && calls.is_empty() {
        return Err(error("empty Gemini completion"));
    }
    let mut message = Message::assistant(&text);
    if !calls.is_empty() {
        message.tool_calls = Some(calls);
    }
    message.provider_state = Some(ProviderMessageState {
        protocol: "google-generate-content",
        model: model.into(),
        items: parts.clone(),
        call_ids: ids,
    });
    let usage = &value["usageMetadata"];
    let eval_tokens = usage["candidatesTokenCount"]
        .as_u64()
        .map(|n| n.saturating_add(usage["thoughtsTokenCount"].as_u64().unwrap_or(0)));
    Ok(ChatResponse {
        message,
        usage: GenUsage {
            prompt_tokens: usage["promptTokenCount"].as_u64(),
            eval_tokens,
            ..Default::default()
        },
        provenance: InferenceProvenance {
            provider_kind: "hosted_google_generate_content".into(),
            model: model.into(),
            ..Default::default()
        },
    })
}

#[cfg(test)]
mod tests;
