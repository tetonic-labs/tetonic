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
                    return Err(error("Responses stream failed or was incomplete"))
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

#[cfg(test)]
mod tests {
    use super::*;
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
