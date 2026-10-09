//! Incremental SSE framing. Tool execution waits for a validated final response.
use super::error;
use crate::{InferenceError, TokenSink};
use serde_json::Value;

#[derive(Default)]
pub(super) struct ResponsesStream {
    buffer: Vec<u8>,
    response: Option<Value>,
    text_seen: bool,
}

impl ResponsesStream {
    pub fn push(
        &mut self,
        bytes: &[u8],
        on_token: &mut TokenSink<'_>,
    ) -> Result<(), InferenceError> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 8 * 1024 * 1024 {
            return Err(error("Responses event too large"));
        }
        loop {
            let lf = self
                .buffer
                .windows(2)
                .position(|value| value == b"\n\n")
                .map(|p| (p, 2));
            let crlf = self
                .buffer
                .windows(4)
                .position(|value| value == b"\r\n\r\n")
                .map(|p| (p, 4));
            let Some((end, delimiter)) = lf.into_iter().chain(crlf).min_by_key(|(p, _)| *p) else {
                break;
            };
            let frame: Vec<_> = self.buffer.drain(..end + delimiter).collect();
            let text =
                std::str::from_utf8(&frame).map_err(|_| error("invalid Responses stream text"))?;
            let data = text
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|value| value.strip_prefix(' ').unwrap_or(value))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let value: Value =
                serde_json::from_str(&data).map_err(|_| error("invalid Responses event"))?;
            match value["type"].as_str() {
                Some("response.output_text.delta") => {
                    if self.response.is_some() {
                        return Err(error("text arrived after Responses completion"));
                    }
                    let delta = value["delta"]
                        .as_str()
                        .ok_or_else(|| error("invalid Responses text delta"))?;
                    self.text_seen |= !delta.is_empty();
                    on_token(delta);
                }
                Some("response.completed") => {
                    if self.response.is_some() || !value["response"].is_object() {
                        return Err(error("invalid Responses completion event"));
                    }
                    self.response = Some(value["response"].clone());
                }
                Some("error" | "response.failed" | "response.incomplete") => {
                    return Err(stream_failure(&value))
                }
                Some(_) => {} // Progress and partial tool arguments do not execute anything.
                None => return Err(error("Responses event missing type")),
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Value, InferenceError> {
        if self.buffer.iter().any(|byte| !byte.is_ascii_whitespace()) {
            return Err(error("truncated Responses event"));
        }
        self.response.ok_or(InferenceError::IncompleteStream {
            tokens_received: self.text_seen,
        })
    }
}

fn stream_failure(value: &Value) -> InferenceError {
    // Provider messages may contain submitted content. Expose fixed explanations
    // or a bounded code label, never the raw message or request.
    let code = value["code"]
        .as_str()
        .or_else(|| value["error"]["code"].as_str())
        .or_else(|| value["response"]["error"]["code"].as_str());
    if let Some(code) = code.filter(|code| {
        !code.is_empty()
            && code.len() <= 64
            && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    }) {
        if !matches!(
            code,
            "insufficient_quota"
                | "credit_balance_exhausted"
                | "rate_limit_exceeded"
                | "invalid_api_key"
                | "authentication_error"
                | "permission_denied"
                | "model_not_found"
                | "invalid_request_error"
                | "server_error"
        ) {
            return error(&format!("Responses API rejected the request ({code})"));
        }
    }
    error(match code {
        Some("insufficient_quota" | "credit_balance_exhausted") => {
            "Responses API quota unavailable; check API billing and project limits"
        }
        Some("rate_limit_exceeded") => "Responses API rate limit reached; retry later",
        Some("invalid_api_key" | "authentication_error" | "permission_denied") => {
            "Responses API access rejected; check the API key and model permissions"
        }
        Some("model_not_found" | "invalid_request_error") => {
            "Responses API rejected the model request"
        }
        Some("server_error") => "Responses API server error; retry later",
        _ if value["type"] == "response.incomplete" => {
            match value["response"]["incomplete_details"]["reason"].as_str() {
                Some("max_output_tokens") => {
                    "Responses output token limit reached before completing the reply"
                }
                Some("content_filter") => "Responses reply stopped by the provider content filter",
                _ => "Responses reply was incomplete",
            }
        }
        _ => "Responses API reported a failed reply",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_safe_provider_failures_without_copying_response_content() {
        for (value, expected) in [
            (
                serde_json::json!({"type":"error","code":"insufficient_quota","message":"PRIVATE"}),
                "API quota unavailable",
            ),
            (
                serde_json::json!({"type":"response.failed","response":{"error":{"code":"rate_limit_exceeded","message":"PRIVATE"}}}),
                "API rate limit reached",
            ),
            (
                serde_json::json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"},"output":"PRIVATE"}}),
                "output token limit reached",
            ),
        ] {
            let mut parser = ResponsesStream::default();
            let message = parser
                .push(format!("data: {value}\n\n").as_bytes(), &mut |_| {})
                .unwrap_err()
                .to_string();
            assert!(message.contains(expected));
            assert!(!message.contains("PRIVATE"));
        }
    }
    #[test]
    fn handles_split_utf8_and_crlf_without_exposing_private_or_partial_tool_events() {
        let bytes =
            concat!("data: {\"type\":\"response.output_text.delta\",\"delta\":\"héllo\"}\r\n\r\n",
            "data: {\"type\":\"response.reasoning.delta\",\"delta\":\"private\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n")
            .as_bytes();
        let mut parser = ResponsesStream::default();
        let mut output = String::new();
        for byte in bytes {
            parser
                .push(&[*byte], &mut |text| output.push_str(text))
                .unwrap();
        }
        assert_eq!(output, "héllo");
        assert_eq!(parser.finish().unwrap()["status"], "completed");
    }
    #[test]
    fn missing_completion_and_explicit_failures_do_not_succeed() {
        let mut parser = ResponsesStream::default();
        parser
            .push(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
                &mut |_| {},
            )
            .unwrap();
        assert!(matches!(
            parser.finish(),
            Err(InferenceError::IncompleteStream {
                tokens_received: true
            })
        ));
        for kind in ["error", "response.failed", "response.incomplete"] {
            let mut parser = ResponsesStream::default();
            assert!(parser
                .push(
                    format!("data: {{\"type\":\"{kind}\"}}\n\n").as_bytes(),
                    &mut |_| {}
                )
                .is_err());
        }
    }
}
