//! Provider-independent request sizing. This is a conservative heuristic, not
//! billing truth: provider tokenization, framing and hidden state can differ.
use crate::ChatRequest;

fn json_tokens(value: &impl serde::Serialize) -> u64 {
    serde_json::to_vec(value).map_or(u64::MAX / 2, |v| (v.len() as u64).div_ceil(3))
}

impl ChatRequest {
    /// Includes message/tool framing, schemas, tool arguments/results, structured
    /// output and private vendor continuation. No private content is exposed.
    /// UTF-8 bytes / 3 gives non-ASCII and code more headroom than chars / 4.
    pub fn estimated_input_tokens(&self) -> u64 {
        let mut tokens = 128_u64;
        for message in &self.messages {
            tokens = tokens.saturating_add(12 + json_tokens(message));
            if let Some(state) = &message.provider_state {
                tokens = tokens.saturating_add(json_tokens(&state.items));
            }
        }
        for tool in &self.tools {
            tokens = tokens.saturating_add(32 + json_tokens(tool));
        }
        if let Some(format) = &self.response_format {
            tokens = tokens.saturating_add(json_tokens(format));
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, ProviderMessageState};
    use serde_json::json;

    #[test]
    fn sizes_tools_results_non_ascii_and_private_continuation() {
        let mut request = ChatRequest {
            messages: vec![Message::user("Help")],
            ..Default::default()
        };
        let small = request.estimated_input_tokens();
        request
            .messages
            .push(Message::tool("read", "文件".repeat(100)));
        let result = request.estimated_input_tokens();
        assert!(result >= small + 200);
        request.tools.push(
            serde_json::from_value(json!({"type":"function","function":{
                "name":"read","description":"Read a file","parameters":{"type":"object"}
            }}))
            .unwrap(),
        );
        assert!(request.estimated_input_tokens() > result);
        let tools = request.estimated_input_tokens();
        request.response_format = Some(json!({"type":"object","required":["summary"]}));
        assert!(request.estimated_input_tokens() > tools);
        let formatted = request.estimated_input_tokens();
        request.messages[0].provider_state = Some(ProviderMessageState {
            protocol: "test",
            model: "model".into(),
            items: vec![json!({"signature":"x".repeat(300)})],
            call_ids: vec![],
        });
        assert!(request.estimated_input_tokens() >= formatted + 100);
    }
}
