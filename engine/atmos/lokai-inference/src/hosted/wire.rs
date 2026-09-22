//! Wire protocol dispatcher for hosted inference backends.
use super::{anthropic, openai, HostedModelConfig, HostedWireProtocol};
use crate::{ChatRequest, ChatResponse, InferenceError};
use serde_json::Value;

pub(super) fn request(
    req: &ChatRequest,
    config: &HostedModelConfig,
) -> Result<Value, InferenceError> {
    match config.protocol {
        HostedWireProtocol::OpenAiChatCompletions => openai::request(req, config),
        HostedWireProtocol::AnthropicMessages => anthropic::request(req, config),
    }
}

pub(super) fn response(
    value: Value,
    config: &HostedModelConfig,
    req_model: &str,
) -> Result<ChatResponse, InferenceError> {
    match config.protocol {
        HostedWireProtocol::OpenAiChatCompletions => openai::response(value, req_model),
        HostedWireProtocol::AnthropicMessages => anthropic::response(&value, req_model),
    }
}
