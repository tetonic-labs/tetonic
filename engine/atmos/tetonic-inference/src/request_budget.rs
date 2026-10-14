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
            let portable = json_tokens(message);
            let content = message.provider_state.as_ref().map_or(portable, |state| {
                let native = json_tokens(&state.items);
                if message.role == "assistant"
                    && state.model == self.model
                    && matches!(
                        state.protocol,
                        "openai-responses" | "anthropic-messages" | "google-generate-content"
                    )
                {
                    // These adapters replace the portable assistant projection
                    // with native continuation; they do not send both copies.
                    // Keep the larger representation as conservative framing.
                    portable.max(native)
                } else {
                    portable.saturating_add(native)
                }
            });
            tokens = tokens.saturating_add(12 + content);
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
    fn native_assistant_continuation_is_not_charged_as_two_copies() {
        for protocol in [
            "openai-responses",
            "anthropic-messages",
            "google-generate-content",
        ] {
            let text = "A substantial saved plan. ".repeat(800);
            let mut message = Message::assistant(text.clone());
            message.provider_state = Some(ProviderMessageState {
                protocol,
                model: "model".into(),
                items: vec![json!({"text": text, "opaque": "private".repeat(100)})],
                call_ids: vec![],
            });
            let portable = json_tokens(&message);
            let native = json_tokens(&message.provider_state.as_ref().unwrap().items);
            let mut request = ChatRequest {
                model: "model".into(),
                messages: vec![message],
                ..Default::default()
            };
            assert_eq!(
                request.estimated_input_tokens(),
                128 + 12 + portable.max(native)
            );
            // Unknown or mismatched continuations retain the conservative fallback.
            request.model = "another-model".into();
            assert_eq!(
                request.estimated_input_tokens(),
                128 + 12 + portable + native
            );
        }
    }

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
