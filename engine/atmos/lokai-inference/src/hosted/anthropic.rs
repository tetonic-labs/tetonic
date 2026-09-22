//! Anthropic Messages API (/v1/messages) wire adapter.
use serde_json::{json, Value};

use super::{error, HostedModelConfig};
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

    let mut system_parts = Vec::new();
    let mut anthropic_messages: Vec<Value> = Vec::new();

    // Map tool_call_ids for assistant tool_use <-> user tool_result correlation
    let mut pending_tool_ids = std::collections::VecDeque::new();

    for (index, msg) in req.messages.iter().enumerate() {
        match msg.role.as_str() {
            "system" => {
                if !msg.content.trim().is_empty() {
                    system_parts.push(msg.content.trim());
                }
            }
            "user" => {
                while let Some((expected_id, _)) = pending_tool_ids.pop_front() {
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
                let mut blocks = Vec::new();
                if !msg.content.is_empty() {
                    blocks.push(json!({"type": "text", "text": msg.content}));
                }
                if let Some(calls) = msg.tool_calls.as_ref().filter(|c| !c.is_empty()) {
                    for (ordinal, call) in calls.iter().enumerate() {
                        let id = format!("toolu_{index}_{ordinal}");
                        pending_tool_ids.push_back((id.clone(), call.function.name.clone()));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": call.function.name,
                            "input": call.function.arguments,
                        }));
                    }
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
                let (expected_id, expected_name) = pending_tool_ids
                    .pop_front()
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
        "max_tokens": config.max_output_tokens,
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
    if let Some(err) = value.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown Anthropic API error");
        return Err(error(&format!("Anthropic API error: {msg}")));
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

    for block in content_blocks {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text);
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| error("missing tool_use name in Anthropic response"))?;
                let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                tool_calls.push(ToolCall {
                    function: FunctionCall {
                        name: name.into(),
                        arguments: input,
                    },
                });
            }
            _ => {}
        }
    }

    let content = text_parts.join("");
    let mut message = Message::assistant(&content);
    if !tool_calls.is_empty() {
        message.tool_calls = Some(tool_calls);
    }

    let stop_reason = value
        .get("stop_reason")
        .and_then(|s| s.as_str())
        .unwrap_or("end_turn");
    if stop_reason == "max_tokens" {
        return Err(error("hosted completion truncated by max_tokens limit"));
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
                Message::tool("read_file", "fn main() {}").with_tool_call_id("call_0"),
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
