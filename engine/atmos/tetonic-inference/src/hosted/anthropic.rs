//! Anthropic Messages API (/v1/messages) wire adapter.
use serde_json::{json, Value};

use super::{error, HostedModelConfig};
use crate::{
    ChatRequest, ChatResponse, FunctionCall, GenUsage, InferenceError, InferenceProvenance,
    Message, ProviderMessageState, ToolCall,
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
    if req.response_format.is_some() {
        return Err(error("Messages profile does not support structured output"));
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

    let mut system_parts = Vec::new();
    let mut anthropic_messages: Vec<Value> = Vec::new();

    // Map tool_call_ids for assistant tool_use <-> user tool_result correlation
    let mut pending_tool_ids = std::collections::VecDeque::new();

    for (index, msg) in req.messages.iter().enumerate() {
        if msg.provider_state.as_ref().is_some_and(|s| {
            msg.role != "assistant" || s.protocol != "anthropic-messages" || s.model != req.model
        }) {
            return Err(error(
                "provider continuation cannot be converted to Messages",
            ));
        }
        match msg.role.as_str() {
            "system" => {
                if !msg.content.trim().is_empty() {
                    system_parts.push(msg.content.trim());
                }
            }
            "user" => {
                while let Some((expected_id, name)) = pending_tool_ids.pop_front() {
                    if name != "finish" {
                        return Err(error("missing tool results in hosted conversation"));
                    }
                    let tool_result_block = json!({
                        "type": "tool_result",
                        "tool_use_id": expected_id,
                        "content": "Completed.",
                    });
                    if let Some(last) = anthropic_messages.last_mut() {
                        if last["role"] == "user" {
                            if let Some(arr) = last["content"].as_array_mut() {
                                arr.push(tool_result_block);
                                continue;
                            }
                        }
                    }
                    anthropic_messages.push(json!({
                        "role": "user",
                        "content": [tool_result_block],
                    }));
                }
                let content_block = json!({"type": "text", "text": msg.content});
                // If previous message was also a user message, append to its content array
                if let Some(last) = anthropic_messages.last_mut() {
                    if last["role"] == "user" {
                        if let Some(arr) = last["content"].as_array_mut() {
                            arr.push(content_block);
                            continue;
                        }
                    }
                }
                anthropic_messages.push(json!({
                    "role": "user",
                    "content": [content_block],
                }));
            }
            "assistant" => {
                if !pending_tool_ids.is_empty() {
                    return Err(error("missing tool results before assistant message"));
                }
                let mut blocks = Vec::new();
                if !msg.content.is_empty() {
                    blocks.push(json!({"type": "text", "text": msg.content}));
                }
                if let Some(calls) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
                    for (ordinal, call) in calls.iter().enumerate() {
                        let id = if let Some(state) = &msg.provider_state {
                            if state.call_ids.len() != calls.len() {
                                return Err(error("invalid Messages call correlation"));
                            }
                            state.call_ids[ordinal].clone()
                        } else {
                            format!("toolu_{index}_{ordinal}")
                        };
                        if pending_tool_ids.iter().any(|(existing, _)| existing == &id) {
                            return Err(error("duplicate Messages tool ID"));
                        }
                        pending_tool_ids.push_back((id.clone(), call.function.name.clone()));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": call.function.name,
                            "input": call.function.arguments,
                        }));
                    }
                }
                if let Some(state) = &msg.provider_state {
                    blocks = state.items.clone();
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": ""}));
                }
                anthropic_messages.push(json!({
                    "role": "assistant",
                    "content": blocks,
                }));
            }
            "tool" => {
                let position = if let Some(id) = &msg.tool_call_id {
                    pending_tool_ids
                        .iter()
                        .position(|(pending, _)| pending == id)
                        .ok_or_else(|| error("tool result ID does not match a pending call"))?
                } else {
                    0
                };
                let (expected_id, expected_name) = pending_tool_ids
                    .remove(position)
                    .ok_or_else(|| error("orphan tool result in hosted conversation"))?;
                if msg.tool_name.as_deref().is_some()
                    && msg.tool_name.as_deref() != Some(&expected_name)
                {
                    return Err(error("out-of-order tool result in hosted conversation"));
                }
                let tool_result_block = json!({
                    "type": "tool_result",
                    "tool_use_id": expected_id,
                    "content": msg.content,
                });
                // In Anthropic API, tool results go into a user turn
                if let Some(last) = anthropic_messages.last_mut() {
                    if last["role"] == "user" {
                        if let Some(arr) = last["content"].as_array_mut() {
                            arr.push(tool_result_block);
                            continue;
                        }
                    }
                }
                anthropic_messages.push(json!({
                    "role": "user",
                    "content": [tool_result_block],
                }));
            }
            _ => return Err(error("unsupported hosted message role")),
        }
    }

    while let Some((expected_id, expected_name)) = pending_tool_ids.pop_front() {
        if expected_name == "finish" {
            let tool_result_block = json!({
                "type": "tool_result",
                "tool_use_id": expected_id,
                "content": "Completed.",
            });
            if let Some(last) = anthropic_messages.last_mut() {
                if last["role"] == "user" {
                    if let Some(arr) = last["content"].as_array_mut() {
                        arr.push(tool_result_block);
                        continue;
                    }
                }
            }
            anthropic_messages.push(json!({
                "role": "user",
                "content": [tool_result_block],
            }));
        } else {
            return Err(error("missing tool results in hosted conversation"));
        }
    }

    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens.unwrap_or(config.max_output_tokens).min(config.max_output_tokens),
        "messages": anthropic_messages,
    });

    if !system_parts.is_empty() {
        body["system"] = json!(system_parts.join("\n\n"));
    }

    if config.send_temperature {
        body["temperature"] = json!(req.temperature);
    }

    if !req.tools.is_empty() {
        let mut tools = Vec::new();
        for t in &req.tools {
            tools.push(json!({
                "name": t.function.name,
                "description": t.function.description,
                "input_schema": t.function.parameters,
            }));
        }
        body["tools"] = json!(tools);
    }

    Ok(body)
}

pub fn response(value: &Value, model: &str) -> Result<ChatResponse, InferenceError> {
    if value.get("error").is_some() {
        return Err(error("Anthropic rejected the request"));
    }

    let role = value.get("role").and_then(|r| r.as_str()).unwrap_or("");
    if role != "assistant" {
        return Err(error(
            "invalid or missing hosted assistant role in Anthropic response",
        ));
    }

    let content_blocks = value
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| error("missing content array in Anthropic response"))?;

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut call_ids = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for block in content_blocks {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                text_parts.push(
                    block["text"]
                        .as_str()
                        .ok_or_else(|| error("invalid Messages text"))?,
                );
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| error("missing tool_use name in Anthropic response"))?;
                let input = block
                    .get("input")
                    .cloned()
                    .ok_or_else(|| error("missing Messages tool input"))?;
                let id = block["id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 512)
                    .ok_or_else(|| error("missing Messages tool ID"))?;
                if name.is_empty() || !input.is_object() || !seen.insert(id.to_owned()) {
                    return Err(error("invalid Messages tool call"));
                }
                call_ids.push(id.to_owned());
                tool_calls.push(ToolCall {
                    function: FunctionCall {
                        name: name.into(),
                        arguments: input,
                    },
                });
            }
            Some("thinking") if block["thinking"].is_string() && block["signature"].is_string() => {
            }
            Some("redacted_thinking") if block["data"].is_string() => {}
            _ => return Err(error("unsupported Messages content")),
        }
    }

    let content = text_parts.join("");
    let mut message = Message::assistant(&content);
    if !tool_calls.is_empty() {
        message.tool_calls = Some(tool_calls);
    }
    message.provider_state = Some(ProviderMessageState {
        protocol: "anthropic-messages",
        model: model.into(),
        items: content_blocks.clone(),
        call_ids,
    });

    let stop_reason = value
        .get("stop_reason")
        .and_then(|s| s.as_str())
        .ok_or_else(|| error("missing Messages stop reason"))?;
    if !matches!(stop_reason, "end_turn" | "tool_use" | "stop_sequence")
        || (stop_reason == "tool_use") != message.tool_calls.is_some()
        || (message.content.is_empty() && message.tool_calls.is_none())
    {
        return Err(error(
            "hosted completion was refused, incomplete or unsupported",
        ));
    }

    let usage = value.get("usage");
    let prompt_tokens = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|t| t.as_u64());
    let eval_tokens = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|t| t.as_u64());

    Ok(ChatResponse {
        message,
        usage: GenUsage {
            prompt_tokens,
            eval_tokens,
            ..Default::default()
        },
        provenance: InferenceProvenance {
            provider_kind: "hosted_anthropic_messages".into(),
            model: model.into(),
            ..Default::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FunctionCall, ToolCall, ToolSchema};

    #[test]
    fn signed_blocks_and_real_ids_survive_parallel_tool_round_trip_privately() {
        let parts = json!([
            {"type":"thinking","thinking":"private deliberation","signature":"opaque"},
            {"type":"redacted_thinking","data":"encrypted"},
            {"type":"tool_use","id":"real-a","name":"read_file","input":{"path":"a"}},
            {"type":"tool_use","id":"real-b","name":"read_file","input":{"path":"b"}}
        ]);
        let value = json!({"role":"assistant","content":parts,"stop_reason":"tool_use"});
        let model = "claude-3-5-sonnet-20241022";
        let msg = response(&value, model).unwrap().message;
        assert!(msg.content.is_empty());
        assert!(!serde_json::to_string(&msg)
            .unwrap()
            .contains("private deliberation"));
        assert!(!format!("{msg:?}").contains("opaque"));
        let mut req = ChatRequest {
            model: model.into(),
            messages: vec![Message::user("Read"), msg],
            ..Default::default()
        };
        assert!(request(&req, &test_config()).is_err());
        req.messages.extend([
            Message::tool("read_file", "B").with_tool_call_id("real-b"),
            Message::tool("read_file", "A").with_tool_call_id("real-a"),
        ]);
        let body = request(&req, &test_config()).unwrap();
        assert_eq!(body["messages"][1]["content"], parts);
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "real-b");
        req.messages.last_mut().unwrap().tool_call_id = Some("unknown".into());
        assert!(request(&req, &test_config()).is_err());
        for reason in ["max_tokens", "refusal", "pause_turn"] {
            let mut bad = value.clone();
            bad["stop_reason"] = json!(reason);
            assert!(response(&bad, model).is_err());
        }
        let mut bad = value;
        bad["content"][3]["id"] = json!("real-a");
        assert!(response(&bad, model).is_err());
    }

    fn test_config() -> HostedModelConfig {
        HostedModelConfig {
            protocol: super::super::HostedWireProtocol::AnthropicMessages,
            model: "claude-3-5-sonnet-20241022".into(),
            allowed_models: vec!["claude-3-5-sonnet-20241022".into()],
            max_output_tokens: 4096,
            output_limit_field: super::super::OutputLimitField::MaxTokens,
            supports_tools: true,
            supports_json_schema: false,
            send_temperature: true,
        }
    }

    #[test]
    fn completion_limit_respects_request_and_provider_ceiling() {
        let mut req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            messages: vec![Message::user("Hello")],
            max_tokens: Some(64),
            ..Default::default()
        };
        assert_eq!(request(&req, &test_config()).unwrap()["max_tokens"], 64);
        req.max_tokens = Some(8192);
        assert_eq!(request(&req, &test_config()).unwrap()["max_tokens"], 4096);
    }

    #[test]
    fn formats_system_and_user_messages() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            messages: vec![
                Message::system("You are a helpful coding assistant."),
                Message::user("Hello!"),
            ],
            temperature: 0.5,
            ..Default::default()
        };
        let body = request(&req, &test_config()).unwrap();
        assert_eq!(body["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["system"], "You are a helpful coding assistant.");
        assert_eq!(body["temperature"], 0.5);

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"][0]["text"], "Hello!");
    }

    #[test]
    fn formats_tools_and_tool_results() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            messages: vec![
                Message::user("Read main.rs"),
                Message::assistant("Reading file").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: "read_file".into(),
                        arguments: json!({"path": "src/main.rs"}),
                    },
                }]),
                Message::tool("read_file", "fn main() {}").with_tool_call_id("toolu_1_0"),
            ],
            tools: vec![ToolSchema::function(
                "read_file",
                "Reads a file",
                json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"]
                }),
            )],
            ..Default::default()
        };
        let body = request(&req, &test_config()).unwrap();
        assert!(body["tools"].as_array().is_some());
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "read_file");
        assert_eq!(tools[0]["input_schema"]["type"], "object");

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        // assistant turn with tool_use
        assert_eq!(messages[1]["role"], "assistant");
        let assistant_blocks = messages[1]["content"].as_array().unwrap();
        assert_eq!(assistant_blocks[1]["type"], "tool_use");
        assert_eq!(assistant_blocks[1]["name"], "read_file");

        // user turn with tool_result
        assert_eq!(messages[2]["role"], "user");
        let user_blocks = messages[2]["content"].as_array().unwrap();
        assert_eq!(user_blocks[0]["type"], "tool_result");
        assert_eq!(user_blocks[0]["content"], "fn main() {}");
    }

    #[test]
    fn parses_anthropic_response_with_tool_call() {
        let resp_json = json!({
            "id": "msg_01X9DPWvTFvFvB8U7G5xY",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "I will read the file."},
                {
                    "type": "tool_use",
                    "id": "toolu_01A09q90tc1qHookC8UZGFZb",
                    "name": "read_file",
                    "input": {"path": "src/main.rs"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 120,
                "output_tokens": 45
            }
        });
        let parsed = response(&resp_json, "claude-3-5-sonnet-20241022").unwrap();
        assert_eq!(parsed.message.content, "I will read the file.");
        let calls = parsed.message.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[0].function.arguments["path"], "src/main.rs");
        assert_eq!(parsed.usage.prompt_tokens, Some(120));
        assert_eq!(parsed.usage.eval_tokens, Some(45));
        assert_eq!(parsed.provenance.provider_kind, "hosted_anthropic_messages");
    }

    #[test]
    fn multi_turn_finish_synthesizes_tool_result() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            messages: vec![
                Message::user("howdy"),
                Message::assistant("").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: "finish".into(),
                        arguments: json!({"summary": "Hello! How can I help you?"}),
                    },
                }]),
                Message::user("can you tell me what this codebase does"),
            ],
            ..Default::default()
        };
        let body = request(&req, &test_config()).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "user");
        let user_blocks = messages[2]["content"].as_array().unwrap();
        assert_eq!(user_blocks[0]["type"], "tool_result");
        assert_eq!(user_blocks[0]["tool_use_id"], "toolu_1_0");
        assert_eq!(user_blocks[0]["content"], "Completed.");
        assert_eq!(user_blocks[1]["type"], "text");
        assert_eq!(
            user_blocks[1]["text"],
            "can you tell me what this codebase does"
        );
    }

    #[test]
    fn finish_at_end_of_conversation_synthesizes_tool_result() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            messages: vec![
                Message::user("howdy"),
                Message::assistant("").with_tool_calls(vec![ToolCall {
                    function: FunctionCall {
                        name: "finish".into(),
                        arguments: json!({"summary": "Done"}),
                    },
                }]),
            ],
            ..Default::default()
        };
        let body = request(&req, &test_config()).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "user");
        let user_blocks = messages[2]["content"].as_array().unwrap();
        assert_eq!(user_blocks[0]["type"], "tool_result");
        assert_eq!(user_blocks[0]["tool_use_id"], "toolu_1_0");
        assert_eq!(user_blocks[0]["content"], "Completed.");
    }
}
