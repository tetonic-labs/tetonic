//! Explicit serializer for protected executor checkpoints. Ordinary `Message`
//! serialization MUST continue to exclude provider protocol state from logs,
//! shared transcripts and unrelated providers. Use only inside an integrity-
//! checked, scope-bound checkpoint with a pinned harness/model configuration.
use crate::{Message, ProviderMessageState};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedMessage {
    message: Message,
    continuation: Option<Continuation>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Continuation {
    protocol: String,
    model: String,
    items: Vec<serde_json::Value>,
    call_ids: Vec<String>,
}

pub fn serialize<S: Serializer>(messages: &[Message], serializer: S) -> Result<S::Ok, S::Error> {
    let saved: Vec<_> = messages
        .iter()
        .map(|message| SavedMessage {
            message: message.clone(),
            continuation: message.provider_state.as_ref().map(|state| Continuation {
                protocol: state.protocol.into(),
                model: state.model.clone(),
                items: state.items.clone(),
                call_ids: state.call_ids.clone(),
            }),
        })
        .collect();
    saved.serialize(serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Message>, D::Error> {
    let saved = Vec::<SavedMessage>::deserialize(deserializer)?;
    saved
        .into_iter()
        .map(|entry| {
            let mut message = entry.message;
            if let Some(state) = entry.continuation {
                let protocol = match state.protocol.as_str() {
                    "openai-responses" => "openai-responses",
                    "anthropic-messages" => "anthropic-messages",
                    "google-generate-content" => "google-generate-content",
                    _ => return Err(serde::de::Error::custom("unsupported checkpoint protocol")),
                };
                if state.model.is_empty()
                    || message.role != "assistant"
                    || state.call_ids.len() != message.tool_calls.as_ref().map_or(0, Vec::len)
                    || state.call_ids.iter().any(String::is_empty)
                {
                    return Err(serde::de::Error::custom("invalid checkpoint continuation"));
                }
                message.provider_state = Some(ProviderMessageState {
                    protocol,
                    model: state.model,
                    items: state.items,
                    call_ids: state.call_ids,
                });
            }
            Ok(message)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Checkpoint {
        #[serde(with = "super")]
        messages: Vec<Message>,
    }

    #[test]
    fn continuation_survives_only_explicit_checkpoint_serialization() {
        for protocol in [
            "openai-responses",
            "anthropic-messages",
            "google-generate-content",
        ] {
            let mut message = Message::assistant("").with_tool_calls(vec![crate::ToolCall {
                function: crate::FunctionCall {
                    name: "ask_human".into(),
                    arguments: serde_json::json!({}),
                },
            }]);
            message.provider_state = Some(ProviderMessageState {
                protocol,
                model: "pinned-model".into(),
                items: vec![serde_json::json!({"opaque":"PRIVATE_PROTOCOL_CANARY"})],
                call_ids: vec!["provider-call-123".into()],
            });
            let bytes = serde_json::to_vec(&Checkpoint {
                messages: vec![message],
            })
            .unwrap();
            let restored: Checkpoint = serde_json::from_slice(&bytes).unwrap();
            let message = &restored.messages[0];
            assert_eq!(message.provider_tool_call_ids(), ["provider-call-123"]);
            assert_eq!(message.provider_state.as_ref().unwrap().protocol, protocol);
            assert_eq!(
                message.provider_state.as_ref().unwrap().items[0]["opaque"],
                "PRIVATE_PROTOCOL_CANARY"
            );
            assert!(!serde_json::to_string(message)
                .unwrap()
                .contains("PRIVATE_PROTOCOL_CANARY"));
            assert!(!format!("{message:?}").contains("PRIVATE_PROTOCOL_CANARY"));
        }
    }

    #[test]
    fn unknown_protocol_is_not_silently_dropped() {
        let json = serde_json::json!({"messages":[{"message":{"role":"assistant"},
            "continuation":{"protocol":"future-protocol","model":"x","items":[],"call_ids":[]}}]});
        assert!(serde_json::from_value::<Checkpoint>(json).is_err());
    }
}
