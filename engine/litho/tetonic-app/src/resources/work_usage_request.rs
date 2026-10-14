//! Per-call budget feedback assembled by the host, using the existing ledger.
use tetonic_inference::{ChatRequest, Message};

pub(super) const MIN_OUTPUT: i64 = 256;

fn notice(remaining: i64, input: i64, output: u32) -> Message {
    Message::system(format!(
        "Current execution allowance: {remaining} tokens remain for this attempt, including repeated input and generated output. \
         This request reserves approximately {input} input tokens; output is capped at {output} tokens. \
         Before another action, account for reading this context again. If another request may not fit, finish through \
         the existing completion tool only when the requested deliverable is actually complete. Otherwise use ask_human \
         if available to report partial progress and the budget blocker; do not mark unfinished work complete. \
         This allowance does not grant additional permissions."
    ))
}

pub(super) fn estimate(request: &ChatRequest) -> i64 {
    // Reserve room for the longest numeric feedback before atomic admission.
    let mut with_feedback = request.clone();
    with_feedback
        .messages
        .push(notice(i64::MAX, i64::MAX, u32::MAX));
    i64::try_from(with_feedback.estimated_input_tokens()).unwrap_or(i64::MAX)
}

pub(super) fn apply(request: &mut ChatRequest, remaining: i64, input: i64) {
    let cap = u32::try_from(remaining.saturating_sub(input).max(0)).unwrap_or(u32::MAX);
    let cap = request.max_tokens.map_or(cap, |existing| existing.min(cap));
    request.max_tokens = Some(cap);
    // This modifies only the outgoing request, not the durable conversation.
    request.messages.push(notice(remaining, input, cap));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_input_headroom_and_preserves_smaller_output_caps() {
        let original = ChatRequest {
            messages: vec![Message::user("Write a short report")],
            ..Default::default()
        };
        let input = estimate(&original);
        let mut request = original.clone();
        apply(&mut request, input + 700, input);
        assert_eq!(request.max_tokens, Some(700));
        assert!(request.estimated_input_tokens() <= input as u64);
        assert_eq!(original.messages.len(), 1);
        assert!(request.messages.last().unwrap().content.contains("700"));
        let mut request = original.clone();
        request.max_tokens = Some(300);
        apply(&mut request, input + 700, input);
        assert_eq!(request.max_tokens, Some(300));
    }
}
